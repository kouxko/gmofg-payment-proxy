//! WebSocket HTTP 握手转发与升级后的透明双向隧道。

use super::{
    BodyExt, Bytes, CancellationToken, ConnectionContext, ConnectionTaskScope, ErrorCode,
    ForwardProxyService, HOST, HeaderValue, Incoming, ProxyBody, ProxyError, Request, Response,
    Result, StatusCode, TokioIo, TrafficDirection, absolute_http_target,
    absolute_uri_to_origin_form, client_http1, collect_pipeline_body, config_error, connect_target,
    ensure_websocket_upgrade_headers, finish_pipeline_response, full_body, incoming_body,
    prepare_pipeline_request, record_websocket_response, reject_websocket_drop,
    request_terminal_response, run_tunnel, scheduled_body, spawn_connection_task,
    strip_hop_by_hop_headers, strip_hop_by_hop_headers_preserving_upgrade, timeout_or_cancel,
    traffic_schedule,
};

impl ForwardProxyService {
    /// WebSocket 只把 HTTP Upgrade 握手交给通用管线；101 之后的帧保持字节流透明转发。
    pub(super) async fn forward_websocket(
        &self,
        mut request: Request<Incoming>,
        context: Option<&ConnectionContext>,
        cancellation: CancellationToken,
        task_scope: &ConnectionTaskScope,
    ) -> Result<Response<ProxyBody>> {
        let downstream_upgrade = hyper::upgrade::on(&mut request);
        let (mut parts, body) = request.into_parts();
        let captured_uri = parts.uri.clone();
        let target = absolute_http_target(&parts.uri)?;
        parts.uri = absolute_uri_to_origin_form(&parts.uri)?;
        strip_hop_by_hop_headers_preserving_upgrade(&mut parts.headers);
        if !parts.headers.contains_key(HOST) {
            parts.headers.insert(
                HOST,
                HeaderValue::from_str(&target.host_header)
                    .map_err(|error| config_error(format!("invalid Host header: {error}")))?,
            );
        }

        let mut request_actions = Vec::new();
        let outgoing_body = if let (Some(pipeline), Some(context)) = (&self.pipeline, context) {
            let body = collect_pipeline_body(
                body,
                pipeline.limits,
                &cancellation,
                self.config.read_timeout,
            )
            .await?;
            let (message, actions) = prepare_pipeline_request(
                pipeline,
                context,
                &parts.method,
                &captured_uri,
                &parts.headers,
                body,
                &cancellation,
            )
            .await?;
            if let Some(response) = request_terminal_response(&actions, &cancellation)? {
                return Ok(response);
            }
            reject_websocket_drop(&actions)?;
            parts.headers = message.header_map()?;
            ensure_websocket_upgrade_headers(&mut parts.headers);
            request_actions = actions;
            message.body
        } else {
            body.collect()
                .await
                .map_err(|error| ProxyError::new(ErrorCode::Io, error.to_string()))?
                .to_bytes()
        };
        parts.headers.remove(http::header::CONTENT_LENGTH);
        if !outgoing_body.is_empty() {
            parts.headers.insert(
                http::header::CONTENT_LENGTH,
                HeaderValue::from_str(&outgoing_body.len().to_string())
                    .map_err(|error| config_error(format!("invalid content length: {error}")))?,
            );
        }

        let stream = connect_target(
            &target.connect_authority,
            self.config.connect_timeout,
            &cancellation,
        )
        .await?;
        let (mut sender, connection) = client_http1::handshake(TokioIo::new(stream))
            .await
            .map_err(|error| ProxyError::new(ErrorCode::Io, error.to_string()))?;
        spawn_connection_task(task_scope, "WebSocket origin connection", async move {
            connection.with_upgrades().await.map_err(|error| {
                ProxyError::new(
                    ErrorCode::Io,
                    format!("WebSocket origin connection ended: {error}"),
                )
            })
        })?;
        let schedule = traffic_schedule(&request_actions, TrafficDirection::Upstream)?;
        let outgoing = Request::from_parts(
            parts,
            scheduled_body(
                outgoing_body.clone(),
                outgoing_body.len(),
                schedule,
                &cancellation,
            ),
        );
        let mut response = timeout_or_cancel(
            self.config
                .write_timeout
                .saturating_add(self.config.read_timeout),
            &cancellation,
            sender.send_request(outgoing),
            ErrorCode::UpstreamReadTimeout,
        )
        .await?
        .map_err(|error| ProxyError::new(ErrorCode::Io, error.to_string()))?;
        if response.status() != StatusCode::SWITCHING_PROTOCOLS {
            if let (Some(pipeline), Some(context)) = (&self.pipeline, context) {
                return finish_pipeline_response(
                    pipeline,
                    context,
                    response,
                    &cancellation,
                    self.config.read_timeout,
                )
                .await;
            }
            let (mut parts, body) = response.into_parts();
            strip_hop_by_hop_headers(&mut parts.headers);
            return Ok(Response::from_parts(parts, incoming_body(body)));
        }

        let upstream_upgrade = hyper::upgrade::on(&mut response);
        let (mut parts, _body) = response.into_parts();
        if let (Some(pipeline), Some(context)) = (&self.pipeline, context) {
            parts = record_websocket_response(pipeline, context, parts, &cancellation).await?;
        }
        ensure_websocket_upgrade_headers(&mut parts.headers);
        let idle_timeout = self.config.tunnel_idle_timeout;
        let tunnel_cancellation = cancellation.clone();
        spawn_connection_task(task_scope, "WebSocket upgraded tunnel", async move {
            let (downstream, upstream) = tokio::try_join!(downstream_upgrade, upstream_upgrade)
                .map_err(|error| {
                    ProxyError::new(ErrorCode::Io, format!("WebSocket upgrade failed: {error}"))
                })?;
            run_tunnel(
                TokioIo::new(downstream),
                TokioIo::new(upstream),
                idle_timeout,
                tunnel_cancellation,
            )
            .await
        })?;
        Ok(Response::from_parts(parts, full_body(Bytes::new())))
    }
}
