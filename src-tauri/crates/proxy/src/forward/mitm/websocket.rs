//! MITM 内层 WebSocket 握手与升级后的透明帧隧道。

use super::{
    Arc, BodyExt, Bytes, CancellationToken, ConnectionContext, ErrorCode, ForwardPipelineRuntime,
    HOST, HeaderValue, Incoming, Mutex, ProxyBody, ProxyError, Request, Response, Result,
    StatusCode, TokioIo, TrafficDirection, client_http1, collect_pipeline_body, config_error,
    connect_authority, ensure_websocket_upgrade_headers, finish_pipeline_response, full_body,
    incoming_body, prepare_pipeline_request, record_websocket_response, reject_websocket_drop,
    request_terminal_response, run_tunnel, scheduled_body, strip_hop_by_hop_headers,
    strip_hop_by_hop_headers_preserving_upgrade, timeout_or_cancel, traffic_schedule,
};
use std::time::Duration;

#[allow(clippy::too_many_arguments)]
pub(super) async fn forward_mitm_websocket(
    mut request: Request<Incoming>,
    authority: &str,
    sender: Arc<Mutex<client_http1::SendRequest<ProxyBody>>>,
    read_timeout: Duration,
    write_timeout: Duration,
    idle_timeout: Duration,
    pipeline: Option<&ForwardPipelineRuntime>,
    context: Option<&ConnectionContext>,
    cancellation: &CancellationToken,
) -> Result<Response<ProxyBody>> {
    let downstream_upgrade = hyper::upgrade::on(&mut request);
    let (mut parts, body) = request.into_parts();
    if parts.uri.scheme().is_some() {
        let normalized = connect_authority(&parts.uri)?;
        if !normalized.eq_ignore_ascii_case(authority) {
            return Err(config_error(
                "MITM WebSocket authority differs from CONNECT authority",
            ));
        }
        let origin = parts
            .uri
            .path_and_query()
            .map_or("/", http::uri::PathAndQuery::as_str);
        parts.uri = origin
            .parse()
            .map_err(|error| config_error(format!("invalid WebSocket URI: {error}")))?;
    }
    strip_hop_by_hop_headers_preserving_upgrade(&mut parts.headers);
    if !parts.headers.contains_key(HOST) {
        parts.headers.insert(
            HOST,
            HeaderValue::from_str(authority)
                .map_err(|error| config_error(format!("invalid Host header: {error}")))?,
        );
    }

    let mut request_actions = Vec::new();
    let outgoing_body = if let (Some(pipeline), Some(context)) = (pipeline, context) {
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
    let schedule = traffic_schedule(&request_actions, TrafficDirection::Upstream)?;
    let outgoing = Request::from_parts(
        parts,
        scheduled_body(
            outgoing_body.clone(),
            outgoing_body.len(),
            schedule,
            cancellation,
        ),
    );
    let mut sender = sender.lock().await;
    let mut response = timeout_or_cancel(
        write_timeout.saturating_add(read_timeout),
        cancellation,
        sender.send_request(outgoing),
        ErrorCode::UpstreamReadTimeout,
    )
    .await?
    .map_err(|error| ProxyError::new(ErrorCode::Io, error.to_string()))?;
    drop(sender);
    if response.status() != StatusCode::SWITCHING_PROTOCOLS {
        if let (Some(pipeline), Some(context)) = (pipeline, context) {
            return finish_pipeline_response(
                pipeline,
                context,
                response,
                cancellation,
                read_timeout,
            )
            .await;
        }
        let (mut parts, body) = response.into_parts();
        strip_hop_by_hop_headers(&mut parts.headers);
        return Ok(Response::from_parts(parts, incoming_body(body)));
    }

    let upstream_upgrade = hyper::upgrade::on(&mut response);
    let (mut parts, _body) = response.into_parts();
    if let (Some(pipeline), Some(context)) = (pipeline, context) {
        parts = record_websocket_response(pipeline, context, parts, cancellation).await?;
    }
    ensure_websocket_upgrade_headers(&mut parts.headers);
    let tunnel_cancellation = cancellation.clone();
    tokio::spawn(async move {
        let result = async {
            let (downstream, upstream) = tokio::try_join!(downstream_upgrade, upstream_upgrade)
                .map_err(|error| {
                    ProxyError::new(
                        ErrorCode::Io,
                        format!("MITM WebSocket upgrade failed: {error}"),
                    )
                })?;
            run_tunnel(
                TokioIo::new(downstream),
                TokioIo::new(upstream),
                idle_timeout,
                tunnel_cancellation,
            )
            .await
        }
        .await;
        if let Err(error) = result {
            tracing::debug!(code = error.code, message = %error.message, "MITM WebSocket tunnel ended");
        }
    });
    Ok(Response::from_parts(parts, full_body(Bytes::new())))
}
