use super::{
    Arc, AsyncWriteExt, BodyExt, BoxIo, CancellationToken, ConnectionTask, Duration, ErrorCode,
    ForwardRequest, Incoming, InformationalResponseSink, Message, MessageLimits, PacedBody,
    PacedBodyError, ProxyError, RawHttp1HeadCapture, Request, RequestHeadPreservingIo,
    RequestWriteTracker, Response, Result, StdMutex, TokioIo, TrackedIo, TrackedRequestBody,
    TrafficSchedule, UpstreamExchange, client_http1, collect_limited, message_wire_head, mpsc,
    raw_head_capture_limit, timeout_stage,
};

pub(super) enum WriteStageOutcome {
    Flushed,
    Response(hyper::Result<Response<Incoming>>),
}

pub(super) struct Http1ExchangeConfig {
    pub(super) schedule: TrafficSchedule,
    pub(super) write_timeout: Duration,
    pub(super) read_timeout: Duration,
    pub(super) limits: MessageLimits,
}

struct Http1RequestState<'a> {
    config: Http1ExchangeConfig,
    tracker: Arc<RequestWriteTracker>,
    response_head: Arc<StdMutex<RawHttp1HeadCapture>>,
    informational_events: mpsc::UnboundedReceiver<()>,
    informational: Option<&'a InformationalResponseSink>,
    cancellation: &'a CancellationToken,
}

pub(super) async fn publish_new_informational_heads(
    response_head: &StdMutex<RawHttp1HeadCapture>,
    published_count: &mut usize,
    informational: Option<&InformationalResponseSink>,
    cancellation: &CancellationToken,
) -> Result<()> {
    let Some(informational) = informational else {
        return Ok(());
    };
    let heads = response_head
        .lock()
        .expect("raw HTTP response head capture mutex poisoned")
        .informational_heads("upstream response")?;
    for head in heads.iter().skip(*published_count) {
        informational.publish(head.clone(), cancellation).await?;
    }
    *published_count = heads.len();
    Ok(())
}

/// 完成一次上游 HTTP/1.1 请求，并同时保留原始报文头与 Hyper 的协议状态机。
///
/// 超时刻意分成两个阶段：请求体尚未 flush 前使用写超时（并加上弱网调度的预计延迟），
/// flush 后才切换为读超时，避免把“Server 尚未响应”误报成写入失败。Server 可能在请求体
/// 发送期间返回最终响应，因此写阶段也必须同时轮询 response future。
///
/// `TrackedIo` 会在 Hyper 解析前捕获所有 1xx 与最终响应头；循环收到捕获通知后立即发布
/// 新增的 1xx，最终再校验捕获状态码与 Hyper 结果一致，防止两套观察结果静默分叉。
pub(super) async fn send_http1_request(
    io: BoxIo,
    request: ForwardRequest,
    config: Http1ExchangeConfig,
    informational: Option<&InformationalResponseSink>,
    cancellation: &CancellationToken,
) -> Result<UpstreamExchange> {
    let canonical_head = message_wire_head(&request.message)?;
    let io: BoxIo = Box::new(RequestHeadPreservingIo::new(io, canonical_head));
    let tracker = Arc::new(RequestWriteTracker::default());
    let response_head = Arc::new(StdMutex::new(RawHttp1HeadCapture::final_response()));
    let (informational_ready, informational_events) = mpsc::unbounded_channel();
    let tracked_io = TrackedIo::new(
        io,
        tracker.clone(),
        Arc::clone(&response_head),
        informational_ready,
        raw_head_capture_limit(config.limits),
    );
    let mut http1 = client_http1::Builder::new();
    http1.title_case_headers(true);
    let (mut sender, connection) = http1
        .handshake(TokioIo::new(tracked_io))
        .await
        .map_err(|error| ProxyError::new(ErrorCode::Io, error.to_string()))?;
    let connection_task = ConnectionTask::spawn(connection);
    let result = execute_http1_request(
        &mut sender,
        request,
        Http1RequestState {
            config,
            tracker,
            response_head,
            informational_events,
            informational,
            cancellation,
        },
    )
    .await;

    connection_task.shutdown().await;
    result
}

async fn execute_http1_request(
    sender: &mut client_http1::SendRequest<TrackedRequestBody>,
    request: ForwardRequest,
    state: Http1RequestState<'_>,
) -> Result<UpstreamExchange> {
    let Http1RequestState {
        config:
            Http1ExchangeConfig {
                schedule,
                write_timeout,
                read_timeout,
                limits,
            },
        tracker,
        response_head,
        mut informational_events,
        informational,
        cancellation,
    } = state;
    let effective_write_timeout =
        write_timeout.saturating_add(schedule.estimated_delay(request.message.body.len()));

    timeout_stage(
        effective_write_timeout,
        cancellation,
        sender.ready(),
        ErrorCode::UpstreamWriteTimeout,
    )
    .await?
    .map_err(|error| ProxyError::new(ErrorCode::Io, error.to_string()))?;

    let outgoing = tracked_request(request, tracker.clone(), schedule, cancellation)?;

    let mut response_future = Box::pin(sender.send_request(outgoing));
    let mut published_informational = 0usize;
    let write_outcome = timeout_stage(
        effective_write_timeout,
        cancellation,
        async {
            loop {
                publish_new_informational_heads(
                    &response_head,
                    &mut published_informational,
                    informational,
                    cancellation,
                )
                .await?;
                tokio::select! {
                    response = &mut response_future => {
                        break Ok(WriteStageOutcome::Response(response));
                    }
                    () = tracker.wait_until_flushed() => {
                        break Ok(WriteStageOutcome::Flushed);
                    }
                    Some(()) = informational_events.recv() => {}
                }
            }
        },
        ErrorCode::UpstreamWriteTimeout,
    )
    .await??;
    let response = match write_outcome {
        WriteStageOutcome::Response(response) => response,
        WriteStageOutcome::Flushed => {
            timeout_stage(
                read_timeout,
                cancellation,
                async {
                    loop {
                        publish_new_informational_heads(
                            &response_head,
                            &mut published_informational,
                            informational,
                            cancellation,
                        )
                        .await?;
                        tokio::select! {
                            response = &mut response_future => break Ok(response),
                            Some(()) = informational_events.recv() => {}
                        }
                    }
                },
                ErrorCode::UpstreamReadTimeout,
            )
            .await??
        }
    }
    .map_err(|error| ProxyError::new(ErrorCode::Io, error.to_string()))?;
    publish_new_informational_heads(
        &response_head,
        &mut published_informational,
        informational,
        cancellation,
    )
    .await?;

    finalize_http1_response(
        response,
        &response_head,
        published_informational,
        read_timeout,
        limits,
        cancellation,
    )
    .await
}

fn tracked_request(
    request: ForwardRequest,
    tracker: Arc<RequestWriteTracker>,
    schedule: TrafficSchedule,
    cancellation: &CancellationToken,
) -> Result<Request<TrackedRequestBody>> {
    let headers = request.message.header_map()?;
    let mut outgoing = Request::builder()
        .method(request.method)
        .uri(request.uri)
        .version(http::Version::HTTP_11)
        .body(TrackedRequestBody::new(
            request.message.body,
            tracker,
            schedule,
            cancellation.clone(),
        ))
        .map_err(|error| ProxyError::new(ErrorCode::Internal, error.to_string()))?;
    *outgoing.headers_mut() = headers;
    Ok(outgoing)
}

async fn finalize_http1_response(
    response: Response<Incoming>,
    response_head: &StdMutex<RawHttp1HeadCapture>,
    published_informational: usize,
    read_timeout: Duration,
    limits: MessageLimits,
    cancellation: &CancellationToken,
) -> Result<UpstreamExchange> {
    let (parts, body) = response.into_parts();
    let body = timeout_stage(
        read_timeout,
        cancellation,
        collect_limited(body, limits.max_body_bytes),
        ErrorCode::UpstreamReadTimeout,
    )
    .await??;
    let raw_head = response_head
        .lock()
        .expect("raw HTTP response head capture mutex poisoned")
        .required_head("upstream response")?;
    let message = Message::from_raw_http1_head(&raw_head, body)?;
    if message.http_status() != Some(parts.status.as_u16()) {
        return Err(ProxyError::new(
            ErrorCode::Io,
            "captured upstream HTTP status does not match Hyper's final response",
        ));
    }
    message.validate(limits)?;
    let informational_heads = response_head
        .lock()
        .expect("raw HTTP response head capture mutex poisoned")
        .informational_heads("upstream response")?
        .into_iter()
        .skip(published_informational)
        .collect();
    Ok(UpstreamExchange {
        informational_heads,
        final_response: message,
    })
}

pub(super) async fn send_scheduled_upstream_abort(
    io: &mut BoxIo,
    message: &Message,
    schedule: TrafficSchedule,
    write_timeout: Duration,
    cancellation: &CancellationToken,
) -> Result<Message> {
    let after_bytes = schedule
        .disconnect_after_bytes
        .expect("disconnect schedule was checked");
    if after_bytes >= message.body.len() {
        return Err(ProxyError::new(
            ErrorCode::ConfigInvalid,
            "upstream disconnect offset must be smaller than request body",
        ));
    }
    let wire = message.reconstruct();
    let header_end = wire
        .windows(4)
        .position(|window| window == b"\r\n\r\n")
        .map(|index| index + 4)
        .ok_or_else(|| ProxyError::new(ErrorCode::Internal, "request headers are incomplete"))?;
    timeout_stage(
        write_timeout,
        cancellation,
        io.write_all(&wire[..header_end]),
        ErrorCode::UpstreamWriteTimeout,
    )
    .await?
    .map_err(|error| ProxyError::io("write upstream request headers", &error))?;

    let mut body = PacedBody::new(
        message.body.clone(),
        message.body.len(),
        schedule,
        cancellation.clone(),
    );
    while let Some(frame) = body.frame().await {
        match frame {
            Ok(frame) => {
                if let Ok(data) = frame.into_data() {
                    timeout_stage(
                        write_timeout,
                        cancellation,
                        io.write_all(&data),
                        ErrorCode::UpstreamWriteTimeout,
                    )
                    .await?
                    .map_err(|error| ProxyError::io("write paced upstream body", &error))?;
                }
            }
            Err(PacedBodyError::Disconnected) => {
                let _ = io.shutdown().await;
                return Err(ProxyError::new(
                    ErrorCode::FaultStreamAborted,
                    "request intentionally disconnected during upstream write",
                ));
            }
            Err(PacedBodyError::Cancelled) => {
                return Err(ProxyError::new(
                    ErrorCode::FaultExecutionCancelled,
                    "weak-network request cancelled",
                ));
            }
        }
    }
    Err(ProxyError::new(
        ErrorCode::Internal,
        "upstream disconnect schedule completed without disconnecting",
    ))
}
