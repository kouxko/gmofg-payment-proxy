//! HTTPS MITM 连接、HTTP/1.1 会话与 WebSocket 升级编排。

use super::{
    Arc, AsyncRead, AsyncWrite, BodyExt, BoxIo, Bytes, CancellationToken, ConnectionContext,
    DropResponseMode, ErrorCode, ForwardMitmRuntime, ForwardPipelineRuntime, HOST, HeaderValue,
    Incoming, ProxyBody, ProxyError, Request, Response, Result, StatusCode, TokioIo,
    TrafficDirection, client_http1, collect_pipeline_body, completion_body, config_error,
    connect_authority, drain_upstream_body, drop_response_mode, ensure_websocket_upgrade_headers,
    error_response, finish_pipeline_response, full_body, incoming_body, intentional_drop_error,
    intentional_response_drop, is_websocket_upgrade, prepare_pipeline_request,
    record_websocket_response, reject_websocket_drop, request_terminal_response, run_tunnel,
    scheduled_body, send_request_then_drop_after_write, server_http1, service_fn,
    strip_hop_by_hop_headers, strip_hop_by_hop_headers_preserving_upgrade, timeout_or_cancel,
    tls_config_error, traffic_schedule,
};
use std::time::Duration;
use tokio::sync::Mutex;

#[path = "mitm/certificates.rs"]
mod certificates;
#[path = "mitm/websocket.rs"]
mod websocket;

use websocket::forward_mitm_websocket;
// 此函数是单个 MITM 会话的装配边界；超时、管线、连接上下文和取消令牌均属于会话配置。
#[allow(clippy::too_many_arguments)]
pub(super) async fn serve_mitm_http1<D>(
    downstream: D,
    upstream: BoxIo,
    authority: String,
    read_timeout: Duration,
    write_timeout: Duration,
    idle_timeout: Duration,
    pipeline: Option<Arc<ForwardPipelineRuntime>>,
    context: Option<ConnectionContext>,
    cancellation: CancellationToken,
) -> Result<()>
where
    D: AsyncRead + AsyncWrite + Unpin + Send + 'static,
{
    let (sender, upstream_connection) = client_http1::handshake(TokioIo::new(upstream))
        .await
        .map_err(|error| {
            ProxyError::new(
                ErrorCode::Io,
                format!("MITM upstream HTTP/1 handshake failed: {error}"),
            )
        })?;
    let sender = Arc::new(Mutex::new(sender));
    let upstream_shutdown = CancellationToken::new();
    let upstream_cancel = cancellation.clone();
    let upstream_drop = upstream_shutdown.clone();
    let upstream_task = tokio::spawn(async move {
        tokio::select! {
            () = upstream_cancel.cancelled() => Ok(()),
            () = upstream_drop.cancelled() => Ok(()),
            result = upstream_connection.with_upgrades() => result.map_err(|error| ProxyError::new(
                ErrorCode::Io,
                format!("MITM upstream HTTP/1 connection failed: {error}"),
            )),
        }
    });

    let handler_cancellation = cancellation.clone();
    let handler = service_fn(move |request: Request<Incoming>| {
        let sender = sender.clone();
        let authority = authority.clone();
        let pipeline = pipeline.clone();
        let context = context.clone();
        let cancellation = handler_cancellation.clone();
        let upstream_shutdown = upstream_shutdown.clone();
        async move {
            let result = forward_mitm_request(
                request,
                &authority,
                sender,
                read_timeout,
                write_timeout,
                idle_timeout,
                pipeline.as_deref(),
                context.as_ref(),
                &upstream_shutdown,
                &cancellation,
            )
            .await;
            match result {
                Ok(response) => Ok(response),
                Err(error) if intentional_response_drop(&error) => Err(error),
                Err(error) => Ok(error_response(&error)),
            }
        }
    });
    let downstream_connection = server_http1::Builder::new()
        .keep_alive(true)
        .serve_connection(TokioIo::new(downstream), handler)
        .with_upgrades();
    let downstream_result = tokio::select! {
        () = cancellation.cancelled() => Err(ProxyError::new(
            ErrorCode::ProxyStopped,
            "MITM session cancelled",
        )),
        result = downstream_connection => result.map_err(|error| {
            ProxyError::new(
                ErrorCode::Io,
                format!("MITM downstream HTTP/1 connection failed: {error}"),
            )
        }),
    };
    upstream_task.abort();
    let _ = upstream_task.await;
    downstream_result
}

// HTTP handler 必须显式携带连接复用 sender 以及会话级管线/超时上下文。
#[allow(clippy::too_many_arguments)]
async fn forward_mitm_request(
    request: Request<Incoming>,
    authority: &str,
    sender: Arc<Mutex<client_http1::SendRequest<ProxyBody>>>,
    read_timeout: Duration,
    write_timeout: Duration,
    idle_timeout: Duration,
    pipeline: Option<&ForwardPipelineRuntime>,
    context: Option<&ConnectionContext>,
    upstream_shutdown: &CancellationToken,
    cancellation: &CancellationToken,
) -> Result<Response<ProxyBody>> {
    if is_websocket_upgrade(&request) {
        return forward_mitm_websocket(
            request,
            authority,
            sender,
            read_timeout,
            write_timeout,
            idle_timeout,
            pipeline,
            context,
            cancellation,
        )
        .await;
    }
    let (mut parts, body) = request.into_parts();
    // CONNECT 内部客户端通常发送 origin-form。若它发送 absolute-form，只允许与 CONNECT
    // authority 相同的 https URI，防止一条已授权隧道被借来访问其他主机。
    if parts.uri.scheme().is_some() {
        let uri_authority = parts
            .uri
            .authority()
            .ok_or_else(|| config_error("MITM absolute URI is missing authority"))?
            .as_str()
            .to_owned();
        let normalized = connect_authority(&parts.uri)?;
        if !normalized.eq_ignore_ascii_case(authority) {
            return Err(config_error(
                "MITM request authority differs from CONNECT authority",
            ));
        }
        let origin = parts
            .uri
            .path_and_query()
            .map_or("/", http::uri::PathAndQuery::as_str);
        parts.uri = origin
            .parse()
            .map_err(|error| config_error(format!("invalid MITM origin-form URI: {error}")))?;
        if !parts.headers.contains_key(HOST) {
            parts.headers.insert(
                HOST,
                HeaderValue::from_str(&uri_authority)
                    .map_err(|error| config_error(format!("invalid MITM Host header: {error}")))?,
            );
        }
    }
    strip_hop_by_hop_headers(&mut parts.headers);
    if !parts.headers.contains_key(HOST) {
        parts.headers.insert(
            HOST,
            HeaderValue::from_str(authority)
                .map_err(|error| config_error(format!("invalid MITM Host header: {error}")))?,
        );
    }
    if let (Some(pipeline), Some(context)) = (pipeline, context) {
        let body = collect_pipeline_body(body, pipeline.limits, cancellation, read_timeout).await?;
        let (message, actions) = prepare_pipeline_request(
            pipeline,
            context,
            &parts.method,
            &parts.uri,
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
        parts.headers.remove(http::header::CONTENT_LENGTH);
        parts.headers.insert(
            http::header::CONTENT_LENGTH,
            HeaderValue::from_str(&message.body.len().to_string())
                .map_err(|error| config_error(format!("invalid content length: {error}")))?,
        );
        let schedule = traffic_schedule(&actions, TrafficDirection::Upstream)?;
        let effective_timeout = write_timeout
            .saturating_add(read_timeout)
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
        let mut sender = sender.lock().await;
        if let Some(body_written) = body_written {
            return send_request_then_drop_after_write(
                &mut sender,
                outgoing,
                body_written,
                upstream_shutdown,
                cancellation,
                effective_timeout,
                "MITM",
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
        .map_err(|error| {
            ProxyError::new(
                ErrorCode::Io,
                format!("MITM upstream HTTP request failed: {error}"),
            )
        })?;
        if mode == Some(DropResponseMode::AfterUpstreamBody) {
            drain_upstream_body(response.into_body(), cancellation, read_timeout).await?;
            upstream_shutdown.cancel();
            return Err(intentional_drop_error("MITM"));
        }
        return finish_pipeline_response(pipeline, context, response, cancellation, read_timeout)
            .await;
    }
    let outgoing = Request::from_parts(parts, incoming_body(body));
    let mut sender = sender.lock().await;
    let response = timeout_or_cancel(
        write_timeout.saturating_add(read_timeout),
        cancellation,
        sender.send_request(outgoing),
        ErrorCode::UpstreamReadTimeout,
    )
    .await?
    .map_err(|error| {
        ProxyError::new(
            ErrorCode::Io,
            format!("MITM upstream HTTP request failed: {error}"),
        )
    })?;
    let (mut parts, body) = response.into_parts();
    strip_hop_by_hop_headers(&mut parts.headers);
    // Incoming -> Incoming body adapter is streaming and performs no collect/decode/re-encode;
    // therefore an unmodified body is byte-for-byte forwarded with backpressure.
    Ok(Response::from_parts(parts, incoming_body(body)))
}
