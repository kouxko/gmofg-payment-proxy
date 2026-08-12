use super::{
    AsyncWriteExt, BodyExt, BoxIo, Bytes, CancellationToken, Duration, ErrorCode, FaultAction,
    Future, HeaderMap, Incoming, IntentionalWireFault, Message, MessageLimits, ProxyError,
    RawHeader, Response, ResponseDisposition, Result, StatusCode, StdMutex, WireBody,
};

pub(super) async fn finish_downstream_write(
    io: &mut BoxIo,
    tail: Option<Bytes>,
    write_timeout: Duration,
    cancellation: &CancellationToken,
    intentional_wire_fault: &StdMutex<Option<IntentionalWireFault>>,
) -> Result<()> {
    if let Some(tail) = tail
        && let Err(error) = timeout_stage(
            write_timeout,
            cancellation,
            io.write_all(&tail),
            ErrorCode::Io,
        )
        .await
        .and_then(|result| {
            result.map_err(|error| ProxyError::io("write short content-length tail", &error))
        })
    {
        return Err(intentional_fault_or(error, intentional_wire_fault));
    }
    if let Err(error) = timeout_stage(write_timeout, cancellation, io.flush(), ErrorCode::Io)
        .await
        .and_then(|result| {
            result.map_err(|error| ProxyError::io("flush client connection", &error))
        })
    {
        return Err(intentional_fault_or(error, intentional_wire_fault));
    }
    if let Err(error) = timeout_stage(write_timeout, cancellation, io.shutdown(), ErrorCode::Io)
        .await
        .and_then(|result| {
            result.map_err(|error| ProxyError::io("shutdown client connection", &error))
        })
    {
        return Err(intentional_fault_or(error, intentional_wire_fault));
    }
    if let Some(fault) = *intentional_wire_fault
        .lock()
        .expect("intentional wire fault mutex poisoned")
    {
        return Err(fault.error());
    }
    Ok(())
}

pub(super) fn intentional_fault_or(
    error: ProxyError,
    intentional_wire_fault: &StdMutex<Option<IntentionalWireFault>>,
) -> ProxyError {
    if error.code == ErrorCode::ProxyStopped.as_str() {
        return error;
    }
    intentional_wire_fault
        .lock()
        .expect("intentional wire fault mutex poisoned")
        .map_or(error, IntentionalWireFault::error)
}

pub(super) fn response_from_disposition(
    disposition: ResponseDisposition,
    canonical_response_head: &StdMutex<Option<Bytes>>,
    raw_tail: &StdMutex<Option<Bytes>>,
    intentional_wire_fault: &StdMutex<Option<IntentionalWireFault>>,
    cancellation: &CancellationToken,
) -> Result<Response<WireBody>> {
    let (mut message, mut body, mut schedule, disposition_fault) = match disposition {
        ResponseDisposition::Send { message, schedule } => {
            let body = message.body.clone();
            (message, body, schedule, None)
        }
        ResponseDisposition::Drop => {
            return Err(ProxyError::new(
                ErrorCode::ClientDisconnected,
                "response intentionally dropped",
            ));
        }
        ResponseDisposition::Truncate {
            message,
            bytes,
            schedule,
        } => {
            let body = message.body.slice(..bytes);
            (
                message,
                body,
                schedule,
                Some(IntentionalWireFault::TruncatedResponse),
            )
        }
    };
    let status = parse_response_status(&message.start_line)?;
    let claimed_length = message
        .declared_content_length()
        .unwrap_or(message.body.len());
    let scheduled_abort = if let Some(after_bytes) = schedule.disconnect_after_bytes.take() {
        if after_bytes >= body.len() {
            return Err(ProxyError::new(
                ErrorCode::ConfigInvalid,
                "downstream disconnect offset must be smaller than response body",
            ));
        }
        body = body.slice(..after_bytes);
        Some(IntentionalWireFault::StreamAborted)
    } else {
        None
    };
    let disposition_fault = disposition_fault.or(scheduled_abort).or_else(|| {
        (claimed_length != body.len()).then_some(IntentionalWireFault::IncorrectContentLength)
    });
    if let Some(fault) = disposition_fault {
        *intentional_wire_fault
            .lock()
            .expect("intentional wire fault mutex poisoned") = Some(fault);
    }
    let body = if claimed_length < body.len() {
        let tail = body.slice(claimed_length..);
        *raw_tail.lock().expect("raw tail mutex poisoned") = Some(tail);
        body.slice(..claimed_length)
    } else {
        body
    };
    message.remove_header("connection");
    message.headers.push(RawHeader::new(
        Bytes::from_static(b"Connection"),
        Bytes::from_static(b"close"),
    ));
    if message.declared_content_length().is_none() {
        message.set_content_length(message.body.len());
    }
    *canonical_response_head
        .lock()
        .expect("canonical HTTP response head mutex poisoned") = Some(message_wire_head(&message)?);
    let mut response = Response::builder()
        .status(status)
        .version(http::Version::HTTP_11)
        .body(WireBody::new(
            body,
            claimed_length,
            schedule,
            cancellation.clone(),
        ))
        .map_err(|error| ProxyError::new(ErrorCode::Internal, error.to_string()))?;
    *response.headers_mut() = message.header_map()?;
    Ok(response)
}

pub(super) fn parse_response_status(start_line: &str) -> Result<StatusCode> {
    let value = start_line
        .split_ascii_whitespace()
        .nth(1)
        .ok_or_else(|| ProxyError::new(ErrorCode::Internal, "response status is missing"))?;
    StatusCode::from_bytes(value.as_bytes())
        .map_err(|error| ProxyError::new(ErrorCode::Internal, error.to_string()))
}

pub(super) fn informational_status(head: &[u8]) -> Option<u16> {
    let line_end = head.windows(2).position(|window| window == b"\r\n")?;
    let line = std::str::from_utf8(&head[..line_end]).ok()?;
    let status = line.split_ascii_whitespace().nth(1)?.parse::<u16>().ok()?;
    (100..200).contains(&status).then_some(status)
}

pub(super) fn raw_head_capture_limit(limits: MessageLimits) -> usize {
    // `max_total_header_bytes` counts only names and values. Reserve the
    // delimiters plus a bounded start-line so the recorder can retain the
    // complete head that Hyper has already accepted.
    limits
        .max_total_header_bytes
        .saturating_add(limits.max_headers.saturating_mul(4))
        .saturating_add(8 * 1024)
        .saturating_add(4)
}

pub(super) fn message_wire_head(message: &Message) -> Result<Bytes> {
    let wire = message.reconstruct();
    let end = wire
        .windows(4)
        .position(|window| window == b"\r\n\r\n")
        .map(|position| position + 4)
        .ok_or_else(|| ProxyError::new(ErrorCode::Internal, "HTTP request head is incomplete"))?;
    Ok(wire.slice(..end))
}

pub(super) async fn collect_limited(mut body: Incoming, limit: usize) -> Result<Bytes> {
    let mut bytes = Vec::new();
    while let Some(frame) = body.frame().await {
        let frame = frame.map_err(|error| ProxyError::new(ErrorCode::Io, error.to_string()))?;
        if let Ok(data) = frame.into_data() {
            if bytes.len().saturating_add(data.len()) > limit {
                return Err(ProxyError::new(
                    ErrorCode::BodyTooLarge,
                    format!("body exceeds {limit} bytes"),
                ));
            }
            bytes.extend_from_slice(&data);
        }
    }
    Ok(Bytes::from(bytes))
}

pub(super) fn validate_headers(headers: &HeaderMap, limits: MessageLimits) -> Result<()> {
    if headers.len() > limits.max_headers {
        return Err(ProxyError::new(
            ErrorCode::HeaderLimitExceeded,
            "too many headers",
        ));
    }
    let mut total = 0usize;
    for (name, value) in headers {
        total = total.saturating_add(name.as_str().len() + value.as_bytes().len());
        if name.as_str().len() > limits.max_header_name_bytes
            || value.as_bytes().len() > limits.max_header_value_bytes
            || total > limits.max_total_header_bytes
        {
            return Err(ProxyError::new(
                ErrorCode::HeaderLimitExceeded,
                "header size limit exceeded",
            ));
        }
    }
    Ok(())
}

pub(super) async fn timeout_stage<F, T>(
    timeout: Duration,
    cancellation: &CancellationToken,
    future: F,
    code: ErrorCode,
) -> Result<T>
where
    F: Future<Output = T>,
{
    tokio::select! {
        () = cancellation.cancelled() => Err(ProxyError::new(
            ErrorCode::ProxyStopped,
            "proxy operation cancelled",
        )),
        result = tokio::time::timeout(timeout, future) => result.map_err(|_| {
            ProxyError::new(code, format!("operation timed out after {} ms", timeout.as_millis()))
        }),
    }
}

pub(super) async fn wait_for_timeout(
    timeout: Duration,
    cancellation: &CancellationToken,
    code: ErrorCode,
) -> Result<()> {
    tokio::select! {
        () = cancellation.cancelled() => Err(ProxyError::new(
            ErrorCode::ProxyStopped,
            "proxy operation cancelled",
        )),
        () = tokio::time::sleep(timeout) => Err(ProxyError::new(
            code,
            format!("injected timeout after {} ms", timeout.as_millis()),
        )),
    }
}

#[derive(Clone, Copy)]
pub(super) enum InjectedTimeoutStage {
    Connect,
    Write,
    Read,
}

impl InjectedTimeoutStage {
    const fn error_code(self) -> ErrorCode {
        match self {
            Self::Connect => ErrorCode::UpstreamConnectTimeout,
            Self::Write => ErrorCode::UpstreamWriteTimeout,
            Self::Read => ErrorCode::UpstreamReadTimeout,
        }
    }
}

pub(super) fn injected_timeout(
    actions: &[FaultAction],
    stage: InjectedTimeoutStage,
) -> Option<Duration> {
    actions.iter().find_map(|action| match (stage, action) {
        (InjectedTimeoutStage::Connect, FaultAction::UpstreamConnectTimeout(timeout))
        | (InjectedTimeoutStage::Write, FaultAction::UpstreamWriteTimeout(timeout))
        | (InjectedTimeoutStage::Read, FaultAction::UpstreamReadTimeout(timeout)) => Some(*timeout),
        _ => None,
    })
}

pub(super) async fn wait_for_injected_timeout(
    actions: &[FaultAction],
    stage: InjectedTimeoutStage,
    cancellation: &CancellationToken,
) -> Result<()> {
    let Some(timeout) = injected_timeout(actions, stage) else {
        return Ok(());
    };
    wait_for_timeout(timeout, cancellation, stage.error_code()).await
}
