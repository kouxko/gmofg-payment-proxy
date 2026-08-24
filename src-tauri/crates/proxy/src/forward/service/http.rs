//! 非升级 HTTP/1.1 请求的目标解析、转发与应用管线接入。

use super::{
    CancellationToken, ConnectionContext, ConnectionTaskScope, ErrorCode, ForwardPipelineRuntime,
    ForwardProxyService, HOST, HeaderValue, HttpTarget, Incoming, ProxyBody, ProxyError, Request,
    Response, Result, TokioIo, Uri, absolute_http_target, absolute_uri_to_origin_form,
    client_http1, collect_pipeline_body, config_error, connect_target, incoming_body,
    is_websocket_upgrade, spawn_connection_task, strip_hop_by_hop_headers, text_response,
    timeout_or_cancel,
};

impl ForwardProxyService {
    pub(super) async fn forward_http(
        &self,
        request: Request<Incoming>,
        context: Option<&ConnectionContext>,
        cancellation: CancellationToken,
        task_scope: &ConnectionTaskScope,
        exchange: &super::SharedForwardExchange,
    ) -> Result<Response<ProxyBody>> {
        if is_websocket_upgrade(&request) {
            return Ok(text_response(
                http::StatusCode::NOT_IMPLEMENTED,
                "HTTP Upgrade is not supported by the Exchange runtime",
            ));
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
                    exchange,
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
        parts: http::request::Parts,
        body: Incoming,
        captured_uri: Uri,
        target: HttpTarget,
        pipeline: &ForwardPipelineRuntime,
        context: &ConnectionContext,
        cancellation: &CancellationToken,
        task_scope: &ConnectionTaskScope,
        connection_exchange: &super::SharedForwardExchange,
    ) -> Result<Response<ProxyBody>> {
        let body = collect_pipeline_body(
            body,
            pipeline.limits,
            cancellation,
            self.config.read_timeout,
        )
        .await?;
        let message =
            crate::message::Message::request(&parts.method, &captured_uri, &parts.headers, body);
        message.validate(pipeline.limits)?;
        let endpoint = target.connect_authority.clone();
        let exchange = {
            let mut connection_exchange = connection_exchange.lock().await;
            if let Some(connection_exchange) = connection_exchange.as_ref() {
                std::sync::Arc::clone(&connection_exchange.exchange)
            } else {
                let connector = super::ForwardHttpExchangeConnector {
                    connect_authority: target.connect_authority,
                    host_header: target.host_header,
                    connect_timeout: self.config.connect_timeout,
                    write_timeout: self.config.write_timeout,
                    read_timeout: self.config.read_timeout,
                    limits: pipeline.limits,
                    task_scope: task_scope.clone(),
                };
                let exchange = std::sync::Arc::new(
                    crate::http::HttpExchangeRuntime {
                        context: context.clone(),
                        ports: std::sync::Arc::clone(&pipeline.ports),
                        upstream: std::sync::Arc::new(connector),
                        clock: std::sync::Arc::new(crate::SystemClock),
                        cancellation: cancellation.clone(),
                        informational: None,
                        capabilities: std::sync::Arc::clone(&pipeline.capabilities),
                        endpoint: endpoint.clone(),
                    }
                    .start(task_scope)?,
                );
                *connection_exchange = Some(super::ForwardConnectionExchange {
                    exchange: std::sync::Arc::clone(&exchange),
                });
                exchange
            }
        };
        let output = exchange
            .exchange(
                endpoint,
                crate::http::HttpExchangeRequest {
                    method: parts.method,
                    uri: parts.uri,
                    message,
                },
            )
            .await?;
        super::response_from_pipeline_disposition(output.disposition, cancellation)?.ok_or_else(
            || ProxyError::new(ErrorCode::ClientDisconnected, "forward response dropped"),
        )
    }
}
