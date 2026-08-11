//! 监听器绑定、连接任务监督与 HTTP/1.1 连接生命周期。

use super::{
    BoxIo, CancellationToken, ConnectionContext, Duration, ErrorCode, ForwardProxyService, JoinSet,
    ProxyError, Result, SocketAddr, SystemTime, TcpListener, TokioIo, Uuid, server_http1,
    service_fn,
};

impl ForwardProxyService {
    /// 在一条已接受的下游 TCP 连接上提供 HTTP/1.1 正向代理服务。
    pub async fn serve_connection(
        &self,
        io: BoxIo,
        peer: SocketAddr,
        cancellation: CancellationToken,
    ) -> Result<()> {
        let context = self.pipeline.as_ref().map(|pipeline| ConnectionContext {
            runtime_epoch: pipeline.runtime_epoch,
            connection_id: Uuid::new_v4(),
            channel: pipeline.channel.clone(),
            peer_addr: peer,
            accepted_at: SystemTime::now(),
            tls_peer: None,
        });
        let accepted = if let Some(acceptor) = &self.downstream_tls {
            let admission_context = context.clone().unwrap_or_else(|| ConnectionContext {
                runtime_epoch: Uuid::new_v4(),
                connection_id: Uuid::new_v4(),
                channel: crate::supervisor::ChannelId::new("forward-downstream-tls")
                    .expect("static channel id is valid"),
                peer_addr: peer,
                accepted_at: SystemTime::now(),
                tls_peer: None,
            });
            tokio::select! {
                () = cancellation.cancelled() => return Err(ProxyError::new(
                    ErrorCode::ProxyStopped,
                    "forward proxy stopped during downstream TLS handshake",
                )),
                result = tokio::time::timeout(
                    self.config.connect_timeout,
                    acceptor.accept(io, &admission_context),
                ) => result.map_err(|_| ProxyError::new(
                    ErrorCode::DownstreamTlsHandshakeFailed,
                    "forward downstream TLS handshake timed out",
                ))??,
            }
        } else {
            crate::transport::AcceptedConnection { io, tls_peer: None }
        };
        let context = context.map(|mut context| {
            context.tls_peer = accepted.tls_peer;
            context
        });
        if let (Some(pipeline), Some(context)) = (&self.pipeline, &context) {
            pipeline.ports.connection_opened(context).await;
        }
        let service = self.clone();
        let handler_context = context.clone();
        let handler_cancellation = cancellation.clone();
        let handler = service_fn(move |request| {
            let service = service.clone();
            let context = handler_context.clone();
            let cancellation = handler_cancellation.clone();
            async move { service.handle(request, peer, context, cancellation).await }
        });
        let connection = server_http1::Builder::new()
            .keep_alive(true)
            .serve_connection(TokioIo::new(accepted.io), handler)
            .with_upgrades();
        let result = tokio::select! {
            () = cancellation.cancelled() => Err(ProxyError::new(
                ErrorCode::ProxyStopped,
                "forward proxy stopped while a client connection was active",
            )),
            result = connection => result.map_err(|error| {
                ProxyError::new(
                    ErrorCode::Io,
                    format!("forward proxy HTTP/1 connection failed: {error}"),
                )
            }),
        };
        if let (Some(pipeline), Some(context)) = (&self.pipeline, &context) {
            pipeline.ports.connection_closed(context, &result).await;
        }
        result
    }

    /// 绑定配置中的地址并运行监听循环。
    ///
    /// 需要统一管理多监听器的 Host 可先自行绑定 `TcpListener`，再调用
    /// [`Self::serve_listener`]；这样能在启动 epoch 发布前完成“全部端口先绑定”的事务式
    /// 准备。单监听器 CLI/测试则可直接使用本方法。
    pub async fn bind_and_serve(&self, cancellation: CancellationToken) -> Result<()> {
        let listener = TcpListener::bind(self.config.bind_addr)
            .await
            .map_err(|error| {
                let code = if error.kind() == std::io::ErrorKind::AddrInUse {
                    ErrorCode::PortInUse
                } else {
                    ErrorCode::Io
                };
                ProxyError::new(
                    code,
                    format!(
                        "cannot bind forward proxy listener {}: {error}",
                        self.config.bind_addr
                    ),
                )
            })?;
        self.serve_listener(listener, cancellation).await
    }

    /// 在已经绑定的 listener 上运行，直到取消或 accept 失败。
    pub async fn serve_listener(
        &self,
        listener: TcpListener,
        cancellation: CancellationToken,
    ) -> Result<()> {
        let mut connections = JoinSet::new();
        loop {
            tokio::select! {
                biased;
                () = cancellation.cancelled() => break,
                completed = connections.join_next(), if !connections.is_empty() => {
                    if let Some(Err(error)) = completed
                        && !error.is_cancelled()
                    {
                        tracing::warn!(?error, "forward proxy connection task panicked");
                    }
                }
                accepted = listener.accept() => {
                    let (stream, peer) = accepted
                        .map_err(|error| ProxyError::io("accept forward proxy client", &error))?;
                    stream
                        .set_nodelay(true)
                        .map_err(|error| ProxyError::io("configure forward proxy client", &error))?;
                    let service = self.clone();
                    let connection_cancellation = cancellation.clone();
                    connections.spawn(async move {
                        let result = service
                            .serve_connection(Box::new(stream), peer, connection_cancellation)
                            .await;
                        if let Err(error) = &result
                            && error.code != ErrorCode::ProxyStopped.as_str()
                        {
                            tracing::debug!(
                                code = error.code,
                                message = %error.message,
                                %peer,
                                "forward proxy client connection ended"
                            );
                        }
                        result
                    });
                }
            }
        }

        // 所有子任务共享同一取消令牌。正常情况下会立即结束；超出短暂宽限期时强制
        // abort，保证监听器 stop 不会被静默客户端无限拖住。
        let graceful = async { while connections.join_next().await.is_some() {} };
        if tokio::time::timeout(Duration::from_secs(5), graceful)
            .await
            .is_err()
        {
            connections.abort_all();
            while connections.join_next().await.is_some() {}
        }
        Ok(())
    }
}
