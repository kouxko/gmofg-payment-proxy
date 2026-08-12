//! CONNECT 隧道与显式允许列表 MITM 升级入口。

use super::{
    Arc, AsyncWriteExt, CancellationToken, ConnectionContext, ConnectionTaskScope, ErrorCode,
    ForwardMitmRuntime, ForwardProxyService, Incoming, PrefixIo, ProxyBody, ProxyError, Request,
    Response, Result, StatusCode, TlsAcceptor, TokioIo, authority_host, authority_is_allowed,
    client_hello_requires_tunnel, connect_authority, connect_target, empty_response, mitm,
    read_client_hello_prefix, run_tunnel, spawn_connection_task, timeout_or_cancel,
};

impl ForwardProxyService {
    pub(super) async fn handle_connect(
        &self,
        request: &mut Request<Incoming>,
        context: Option<ConnectionContext>,
        cancellation: CancellationToken,
        task_scope: &ConnectionTaskScope,
    ) -> Result<Response<ProxyBody>> {
        let authority = connect_authority(request.uri())?;
        let authority_host = authority_host(&authority)?;
        if let Some(mitm) = &self.mitm
            && authority_is_allowed(&authority_host, &mitm.config.authority_allowlist)
        {
            return self
                .handle_mitm_connect(
                    request,
                    authority,
                    authority_host,
                    context,
                    cancellation,
                    mitm.clone(),
                    task_scope,
                )
                .await;
        }
        let upstream =
            connect_target(&authority, self.config.connect_timeout, &cancellation).await?;
        let upgraded = hyper::upgrade::on(request);
        let idle_timeout = self.config.tunnel_idle_timeout;
        spawn_connection_task(task_scope, "CONNECT upgraded tunnel", async move {
            let upgraded = upgraded.await.map_err(|error| {
                ProxyError::new(ErrorCode::Io, format!("CONNECT upgrade failed: {error}"))
            })?;
            run_tunnel(TokioIo::new(upgraded), upstream, idle_timeout, cancellation).await
        })?;
        Ok(empty_response(StatusCode::OK))
    }

    // MITM upgrade assembly keeps the validated target, connection context, cancellation and
    // ownership scope explicit until the session task is registered.
    #[allow(clippy::too_many_arguments)]
    async fn handle_mitm_connect(
        &self,
        request: &mut Request<Incoming>,
        authority: String,
        authority_host: String,
        context: Option<ConnectionContext>,
        cancellation: CancellationToken,
        mitm: Arc<ForwardMitmRuntime>,
        task_scope: &ConnectionTaskScope,
    ) -> Result<Response<ProxyBody>> {
        // 在发送 200 前完成签发和上游 TCP 连接。这样 CA/配置错误能作为代理错误返回，而
        // 不是先承诺 tunnel 成功后再静默断开。
        let server_config = mitm.server_config_for(&authority_host).await?;
        let upstream =
            connect_target(&authority, self.config.connect_timeout, &cancellation).await?;
        let upgraded = hyper::upgrade::on(request);
        let read_timeout = self.config.read_timeout;
        let write_timeout = self.config.write_timeout;
        let idle_timeout = self.config.tunnel_idle_timeout;
        let pipeline = self.pipeline.clone();
        spawn_connection_task(task_scope, "MITM CONNECT session", async move {
            let upgraded = upgraded.await.map_err(|error| {
                ProxyError::new(
                    ErrorCode::Io,
                    format!("MITM CONNECT upgrade failed: {error}"),
                )
            })?;
            let mut downstream = TokioIo::new(upgraded);
            let client_hello =
                read_client_hello_prefix(&mut downstream, read_timeout, &cancellation).await?;
            if client_hello_requires_tunnel(&client_hello) {
                let mut upstream = upstream;
                timeout_or_cancel(
                    write_timeout,
                    &cancellation,
                    upstream.write_all(&client_hello),
                    ErrorCode::UpstreamWriteTimeout,
                )
                .await?
                .map_err(|error| ProxyError::io("forward h2/h3 ClientHello", &error))?;
                return run_tunnel(downstream, upstream, idle_timeout, cancellation).await;
            }
            let downstream = TlsAcceptor::from(server_config)
                .accept(PrefixIo::new(client_hello, downstream))
                .await
                .map_err(|error| {
                    ProxyError::new(
                        ErrorCode::TlsHandshakeFailed,
                        format!("MITM downstream TLS handshake failed: {error}"),
                    )
                })?;
            let upstream = mitm
                .upstream_connector
                .connect(&authority_host, upstream, &cancellation)
                .await?;
            mitm::serve_mitm_http1(
                downstream,
                upstream,
                authority,
                read_timeout,
                write_timeout,
                idle_timeout,
                pipeline,
                context,
                cancellation,
            )
            .await
        })?;
        Ok(empty_response(StatusCode::OK))
    }
}
