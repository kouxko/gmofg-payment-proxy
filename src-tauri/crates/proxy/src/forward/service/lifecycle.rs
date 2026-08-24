//! 监听器绑定、连接任务监督与 HTTP/1.1 连接生命周期。

use super::{
    Arc, BoxIo, CancellationToken, ConnectionContext, ConnectionTaskScope, Duration, ErrorCode,
    ForwardProxyService, ProxyError, Result, SocketAddr, SystemTime, TcpListener, TokioIo, Uuid,
    server_http1, service_fn,
};
use async_trait::async_trait;
use tokio::io::AsyncWriteExt;

use crate::listener::{
    ConnectionHandler, ListenerCapacity, ListenerConfig, ListenerRejection, ListenerSupervisor,
    NoopConnectionLifecycleObserver, PrimaryConnectionOutcome, sealed,
};
use crate::transport::{SystemClock, TokioBoundListener, TokioListenerBinder};

const CONNECTION_CHILD_GRACE: Duration = Duration::from_secs(5);

impl ForwardProxyService {
    /// 在一条已接受的下游 TCP 连接上提供 HTTP/1.1 正向代理服务。
    pub async fn serve_connection(
        &self,
        io: BoxIo,
        peer: SocketAddr,
        cancellation: CancellationToken,
    ) -> Result<()> {
        self.serve_connection_in_scope(io, peer, cancellation, ConnectionTaskScope::new())
            .await
    }

    pub(super) async fn serve_connection_in_scope(
        &self,
        io: BoxIo,
        peer: SocketAddr,
        cancellation: CancellationToken,
        task_scope: ConnectionTaskScope,
    ) -> Result<()> {
        let context = self.connection_context(peer);
        let result = self
            .serve_connection_primary(io, context, cancellation, task_scope.clone())
            .await;
        drain_connection_scope(&task_scope, "forward connection").await;
        result
    }

    async fn serve_connection_primary(
        &self,
        io: BoxIo,
        mut context: ConnectionContext,
        cancellation: CancellationToken,
        task_scope: ConnectionTaskScope,
    ) -> Result<()> {
        let peer = context.peer_addr;
        let accepted = if let Some(acceptor) = &self.downstream_tls {
            tokio::select! {
                () = cancellation.cancelled() => return Err(ProxyError::new(
                    ErrorCode::ProxyStopped,
                    "forward proxy stopped during downstream TLS handshake",
                )),
                result = tokio::time::timeout(
                    self.config.connect_timeout,
                    acceptor.accept(io, &context),
                ) => result.map_err(|_| ProxyError::new(
                    ErrorCode::DownstreamTlsHandshakeFailed,
                    "forward downstream TLS handshake timed out",
                ))??,
            }
        } else {
            crate::transport::AcceptedConnection { io, tls_peer: None }
        };
        context.tls_peer = accepted.tls_peer;
        if let Some(pipeline) = &self.pipeline {
            pipeline.ports.connection_opened(&context).await;
        }
        let service = self.clone();
        let handler_context = Some(context.clone());
        let handler_cancellation = cancellation.clone();
        let handler_task_scope = task_scope;
        let exchange = Arc::new(tokio::sync::Mutex::new(None));
        let handler_exchange = Arc::clone(&exchange);
        let handler = service_fn(move |request| {
            let service = service.clone();
            let context = handler_context.clone();
            let cancellation = handler_cancellation.clone();
            let task_scope = handler_task_scope.clone();
            let exchange = Arc::clone(&handler_exchange);
            async move {
                service
                    .handle(request, peer, context, cancellation, task_scope, exchange)
                    .await
            }
        });
        let connection =
            server_http1::Builder::new().serve_connection(TokioIo::new(accepted.io), handler);
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
        let exchange_result = if let Some(exchange) = exchange.lock().await.take() {
            exchange.exchange.shutdown();
            Ok(())
        } else {
            Ok(())
        };
        let result = result.and(exchange_result);
        if let Some(pipeline) = &self.pipeline {
            pipeline.ports.connection_closed(&context, &result).await;
        }
        result
    }

    pub(super) fn connection_context(&self, peer: SocketAddr) -> ConnectionContext {
        self.pipeline.as_ref().map_or_else(
            || ConnectionContext {
                runtime_epoch: Uuid::new_v4(),
                connection_id: Uuid::new_v4(),
                channel: crate::supervisor::ChannelId::new("forward-http")
                    .expect("static channel id is valid"),
                peer_addr: peer,
                accepted_at: SystemTime::now(),
                tls_peer: None,
            },
            |pipeline| ConnectionContext {
                runtime_epoch: pipeline.runtime_epoch,
                connection_id: Uuid::new_v4(),
                channel: pipeline.channel.clone(),
                peer_addr: peer,
                accepted_at: SystemTime::now(),
                tls_peer: None,
            },
        )
    }

    /// 绑定配置中的地址并运行监听循环。
    ///
    /// 需要统一管理多监听器的 Host 可先自行绑定 `TcpListener`，再调用
    /// [`Self::serve_listener`]；这样能在启动 epoch 发布前完成“全部端口先绑定”的事务式
    /// 准备。单监听器 CLI/测试则可直接使用本方法。
    pub async fn bind_and_serve(&self, cancellation: CancellationToken) -> Result<()> {
        let supervisor = self.listener_supervisor(Uuid::new_v4())?;
        supervisor
            .bind_and_run(cancellation)
            .await?
            .into_result("forward listener stopped after a fatal lifecycle failure")
    }

    /// 在已经绑定的 listener 上运行，直到取消或 accept 失败。
    pub async fn serve_listener(
        &self,
        listener: TcpListener,
        cancellation: CancellationToken,
    ) -> Result<()> {
        let epoch = self
            .pipeline
            .as_ref()
            .map_or_else(Uuid::new_v4, |pipeline| pipeline.runtime_epoch);
        let supervisor = self.listener_supervisor(epoch)?;
        supervisor
            .run_bound(Arc::new(TokioBoundListener(listener)), cancellation)
            .await?
            .into_result("forward listener stopped after a fatal lifecycle failure")
    }

    fn listener_supervisor(&self, epoch: Uuid) -> Result<ListenerSupervisor<ForwardProxyService>> {
        let listener_id = self.pipeline.as_ref().map_or_else(
            || crate::supervisor::ChannelId::new("forward-http"),
            |pipeline| Ok(pipeline.channel.clone()),
        )?;
        ListenerSupervisor::new(
            ListenerConfig {
                bind_addr: self.config.bind_addr,
                runtime_epoch: epoch,
                listener_id,
                allowed_client_cidrs: self.config.allowed_client_cidrs.clone(),
                capacity: ListenerCapacity::new(tokio::sync::Semaphore::MAX_PERMITS)?,
                shutdown_grace: CONNECTION_CHILD_GRACE,
            },
            Arc::new(TokioListenerBinder),
            Arc::new(SystemClock),
            Arc::new(self.clone()),
            Arc::new(NoopConnectionLifecycleObserver),
        )
    }
}

impl sealed::Sealed for ForwardProxyService {}

#[async_trait]
impl ConnectionHandler for ForwardProxyService {
    async fn reject(
        &self,
        io: BoxIo,
        context: ConnectionContext,
        reason: ListenerRejection,
        cancellation: CancellationToken,
    ) {
        if reason != ListenerRejection::NetworkDenied {
            return;
        }
        let mut io = if let Some(acceptor) = &self.downstream_tls {
            tokio::select! {
                () = cancellation.cancelled() => return,
                accepted = tokio::time::timeout(
                    self.config.connect_timeout,
                    acceptor.accept(io, &context),
                ) => match accepted {
                    Ok(Ok(accepted)) => accepted.io,
                    Ok(Err(_)) | Err(_) => return,
                },
            }
        } else {
            io
        };
        let response = b"HTTP/1.1 403 Forbidden\r\nConnection: close\r\nContent-Type: text/plain; charset=utf-8\r\nContent-Length: 29\r\n\r\nclient address is not allowed";
        let write = async {
            io.write_all(response).await?;
            io.shutdown().await
        };
        tokio::select! {
            () = cancellation.cancelled() => {}
            _ = tokio::time::timeout(self.config.write_timeout, write) => {}
        }
    }

    async fn handle(
        &self,
        io: BoxIo,
        context: ConnectionContext,
        child_tasks: ConnectionTaskScope,
        cancellation: CancellationToken,
    ) -> PrimaryConnectionOutcome {
        self.serve_connection_primary(io, context, cancellation, child_tasks)
            .await
            .into()
    }
}

pub(super) async fn drain_connection_scope(task_scope: &ConnectionTaskScope, owner: &'static str) {
    task_scope.close();
    if tokio::time::timeout(CONNECTION_CHILD_GRACE, task_scope.drain())
        .await
        .is_err()
    {
        let aborted = task_scope.abort_live();
        tracing::warn!(
            aborted_count = aborted.len(),
            ?aborted,
            owner,
            "forward connection child-task grace period expired"
        );
        task_scope.drain().await;
    }
    if task_scope.snapshot().aggregate.panic_seen {
        tracing::warn!(owner, "forward connection child task panicked");
    }
}
