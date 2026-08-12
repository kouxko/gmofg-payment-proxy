//! 非升级 HTTP/1.1 请求的目标解析、转发与应用管线接入。

use super::{
    CancellationToken, ConnectionContext, ConnectionTaskScope, DropResponseMode, ErrorCode,
    ForwardPipelineRuntime, ForwardProxyService, HOST, HeaderValue, HttpTarget, Incoming,
    ProxyBody, ProxyError, Request, Response, Result, TokioIo, TrafficDirection, Uri,
    absolute_http_target, absolute_uri_to_origin_form, client_http1, collect_pipeline_body,
    completion_body, config_error, connect_target, drain_upstream_body, drop_response_mode,
    finish_pipeline_response, incoming_body, intentional_drop_error, is_websocket_upgrade,
    prepare_pipeline_request, request_terminal_response, scheduled_body,
    send_request_then_drop_after_write, spawn_connection_task, strip_hop_by_hop_headers,
    timeout_or_cancel, traffic_schedule,
};

impl ForwardProxyService {
    pub(super) async fn forward_http(
        &self,
        request: Request<Incoming>,
        context: Option<&ConnectionContext>,
        cancellation: CancellationToken,
        task_scope: &ConnectionTaskScope,
    ) -> Result<Response<ProxyBody>> {
        if is_websocket_upgrade(&request) {
            return self
                .forward_websocket(request, context, cancellation, task_scope)
                .await;
        }
        let (mut parts, body) = request.into_parts();
        let captured_uri = parts.uri.clone();
        let target = absolute_http_target(&parts.uri)?;
        parts.uri = absolute_uri_to_origin_form(&parts.uri)?;
        strip_hop_by_hop_headers(&mut parts.headers);
        if !parts.headers.contains_key(HOST) {
            parts.headers.insert(
                HOST,
                HeaderValue::from_str(&target.host_header).map_err(|error| {
                    config_error(format!("invalid target Host header: {error}"))
                })?,
            );
        }

        if let (Some(pipeline), Some(context)) = (&self.pipeline, context) {
            return self
                .forward_http_through_pipeline(
                    parts,
                    body,
                    captured_uri,
                    target,
                    pipeline,
                    context,
                    &cancellation,
                    task_scope,
                )
                .await;
        }

        let stream = connect_target(
            &target.connect_authority,
            self.config.connect_timeout,
            &cancellation,
        )
        .await?;
        let (mut sender, connection) = client_http1::handshake(TokioIo::new(stream))
            .await
            .map_err(|error| {
                ProxyError::new(
                    ErrorCode::Io,
                    format!("origin HTTP handshake failed: {error}"),
                )
            })?;
        spawn_connection_task(task_scope, "forward origin HTTP connection", async move {
            connection.await.map_err(|error| {
                ProxyError::new(
                    ErrorCode::Io,
                    format!("forward origin HTTP connection ended: {error}"),
                )
            })
        })?;

        let outgoing = Request::from_parts(parts, incoming_body(body));
        let response = timeout_or_cancel(
            self.config
                .write_timeout
                .saturating_add(self.config.read_timeout),
            &cancellation,
            sender.send_request(outgoing),
            ErrorCode::UpstreamReadTimeout,
        )
        .await?
        .map_err(|error| {
            ProxyError::new(
                ErrorCode::Io,
                format!("origin HTTP request failed: {error}"),
            )
        })?;
        let (mut parts, body) = response.into_parts();
        strip_hop_by_hop_headers(&mut parts.headers);
        Ok(Response::from_parts(parts, incoming_body(body)))
    }
}

impl ForwardProxyService {
    // 正向代理管线需要同时保留客户端捕获 URI 和已解析的上游目标；拆成更多薄函数只会
    // 隐藏这一协议边界，因此在此处显式保留完整参数集。
    #[allow(clippy::too_many_arguments)]
    async fn forward_http_through_pipeline(
        &self,
        mut parts: http::request::Parts,
        body: Incoming,
        captured_uri: Uri,
        target: HttpTarget,
        pipeline: &ForwardPipelineRuntime,
        context: &ConnectionContext,
        cancellation: &CancellationToken,
        task_scope: &ConnectionTaskScope,
    ) -> Result<Response<ProxyBody>> {
        let body = collect_pipeline_body(
            body,
            pipeline.limits,
            cancellation,
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
            cancellation,
        )
        .await?;
        if let Some(response) = request_terminal_response(&actions, cancellation)? {
            return Ok(response);
        }
        parts.headers = message.header_map()?;
        strip_hop_by_hop_headers(&mut parts.headers);
        if !parts.headers.contains_key(HOST) {
            parts.headers.insert(
                HOST,
                HeaderValue::from_str(&target.host_header).map_err(|error| {
                    config_error(format!("invalid target Host header: {error}"))
                })?,
            );
        }
        parts.headers.remove(http::header::CONTENT_LENGTH);
        parts.headers.insert(
            http::header::CONTENT_LENGTH,
            HeaderValue::from_str(&message.body.len().to_string())
                .map_err(|error| config_error(format!("invalid content length: {error}")))?,
        );
        let stream = connect_target(
            &target.connect_authority,
            self.config.connect_timeout,
            cancellation,
        )
        .await?;
        let (mut sender, connection) = client_http1::handshake(TokioIo::new(stream))
            .await
            .map_err(|error| ProxyError::new(ErrorCode::Io, error.to_string()))?;
        let upstream_shutdown = CancellationToken::new();
        let connection_shutdown = upstream_shutdown.clone();
        spawn_connection_task(
            task_scope,
            "forward pipeline origin connection",
            async move {
                tokio::select! {
                    () = connection_shutdown.cancelled() => Ok(()),
                    result = connection => result.map_err(|error| ProxyError::new(
                        ErrorCode::Io,
                        format!("forward pipeline origin connection ended: {error}"),
                    )),
                }
            },
        )?;
        let schedule = traffic_schedule(&actions, TrafficDirection::Upstream)?;
        let effective_timeout = self
            .config
            .write_timeout
            .saturating_add(self.config.read_timeout)
            .saturating_add(schedule.estimated_delay(message.body.len()));
        let mode = drop_response_mode(&actions);
        let body = scheduled_body(
            message.body.clone(),
            message.body.len(),
            schedule,
            cancellation,
        );
        let (body, body_written) = if mode == Some(DropResponseMode::AfterRequestWrite) {
            let (body, completed) = completion_body(body);
            (body, Some(completed))
        } else {
            (body, None)
        };
        let outgoing = Request::from_parts(parts, body);
        if let Some(body_written) = body_written {
            return send_request_then_drop_after_write(
                &mut sender,
                outgoing,
                body_written,
                &upstream_shutdown,
                cancellation,
                effective_timeout,
                "forward",
            )
            .await
            .map(|_| unreachable!("drop helper only returns errors"));
        }
        let response = timeout_or_cancel(
            effective_timeout,
            cancellation,
            sender.send_request(outgoing),
            ErrorCode::UpstreamReadTimeout,
        )
        .await?
        .map_err(|error| ProxyError::new(ErrorCode::Io, error.to_string()))?;
        if mode == Some(DropResponseMode::AfterUpstreamBody) {
            drain_upstream_body(response.into_body(), cancellation, self.config.read_timeout)
                .await?;
            upstream_shutdown.cancel();
            return Err(intentional_drop_error("forward"));
        }
        finish_pipeline_response(
            pipeline,
            context,
            response,
            cancellation,
            self.config.read_timeout,
        )
        .await
    }
}
