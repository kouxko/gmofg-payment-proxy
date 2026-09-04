use std::time::Duration;

use bytes::Bytes;
use http::{HeaderMap, StatusCode};
use tokio_util::sync::CancellationToken;

use super::{FaultAction, ResponseDisposition};
use crate::message::Message;
use crate::traffic::{
    IntermittentProfile, JitterProfile, ThrottleProfile, TrafficDirection, TrafficSchedule,
};
use crate::{ErrorCode, ProxyError, Result};

pub async fn cancellable_delay(duration: Duration, cancellation: &CancellationToken) -> Result<()> {
    tokio::select! {
        () = cancellation.cancelled() => Err(ProxyError::new(
            ErrorCode::ProxyStopped,
            "proxy stopped during delay",
        )),
        () = tokio::time::sleep(duration) => Ok(()),
    }
}

pub fn mock_response(status: StatusCode, headers: &HeaderMap, body: Bytes) -> Message {
    let mut message = Message::response(status, headers, body);
    message.body_modified = true;
    message.set_content_length(message.body.len());
    message
}

/// 生成规则执行后客户端最终可观察到的响应快照，但不执行延迟、限速或断开。
///
/// Pipeline 在把 [`FaultAction`] 交给网络传输层之前需要记录会话详情。若直接保存
/// 上游 `Message`，客户端已经收到的自定义状态、Mock、截断或错误长度就会和详情页
/// 不一致。该函数复用与 wire action 相同的顺序，返回 `None` 表示响应被丢弃。
pub fn project_response_for_observation(
    mut message: Message,
    actions: &[FaultAction],
) -> Result<Option<Message>> {
    for action in actions {
        match action {
            FaultAction::DropResponse { .. } => return Ok(None),
            FaultAction::ReplaceBody { body } => message.replace_body(body.clone()),
            FaultAction::ContentLengthOffset(offset) => {
                let declared = declared_content_length(message.body.len(), *offset)?;
                message.set_content_length(declared);
            }
            FaultAction::TruncateResponse(bytes) => {
                validate_prefix(
                    *bytes,
                    message.body.len(),
                    "truncate length must be in 0..body_len",
                )?;
                message.body = message.body.slice(..*bytes);
                message.body_modified = true;
                return Ok(Some(message));
            }
            FaultAction::DisconnectDuringWrite {
                after_bytes,
                direction: TrafficDirection::Downstream,
            } => {
                validate_prefix(
                    *after_bytes,
                    message.body.len(),
                    "downstream disconnect offset must be smaller than response body",
                )?;
                message.body = message.body.slice(..*after_bytes);
                message.body_modified = true;
            }
            FaultAction::CustomStatus(status) => message = with_status(message, *status),
            FaultAction::MockResponse {
                status,
                headers,
                body,
            } => message = mock_response(*status, headers, body.clone()),
            FaultAction::Delay(_)
            | FaultAction::Jitter { .. }
            | FaultAction::Throttle {
                direction: TrafficDirection::Downstream,
                ..
            }
            | FaultAction::Intermittent {
                direction: TrafficDirection::Downstream,
                ..
            } => {}
            FaultAction::RejectTls
            | FaultAction::DisconnectBeforeUpstream
            | FaultAction::UpstreamConnectTimeout(_)
            | FaultAction::UpstreamWriteTimeout(_)
            | FaultAction::UpstreamReadTimeout(_)
            | FaultAction::Throttle {
                direction: TrafficDirection::Upstream,
                ..
            }
            | FaultAction::Intermittent {
                direction: TrafficDirection::Upstream,
                ..
            }
            | FaultAction::DisconnectDuringWrite {
                direction: TrafficDirection::Upstream,
                ..
            } => return Err(request_stage_fault("observation")),
        }
    }
    Ok(Some(message))
}

/// Applies response-stage wire faults in order.
pub async fn apply_response_actions(
    mut message: Message,
    actions: &[FaultAction],
    cancellation: &CancellationToken,
) -> Result<ResponseDisposition> {
    let mut schedule = TrafficSchedule::default();
    for action in actions {
        match action {
            FaultAction::Delay(duration) => cancellable_delay(*duration, cancellation).await?,
            FaultAction::DropResponse { .. } => return Ok(ResponseDisposition::Drop),
            FaultAction::ReplaceBody { body } => message.replace_body(body.clone()),
            FaultAction::ContentLengthOffset(offset) => {
                let declared = declared_content_length(message.body.len(), *offset)?;
                message.set_content_length(declared);
            }
            FaultAction::TruncateResponse(bytes) => {
                validate_prefix(
                    *bytes,
                    message.body.len(),
                    "truncate length must be in 0..body_len",
                )?;
                return Ok(ResponseDisposition::Truncate {
                    message,
                    bytes: *bytes,
                    schedule,
                });
            }
            FaultAction::Jitter {
                minimum,
                maximum,
                scope,
                seed,
            } => {
                schedule.jitter = Some(JitterProfile {
                    minimum: *minimum,
                    maximum: *maximum,
                    scope: *scope,
                });
                schedule.seed = *seed;
            }
            FaultAction::Throttle {
                bytes_per_second,
                chunk_bytes,
                direction: TrafficDirection::Downstream,
            } => {
                schedule.throttle = Some(ThrottleProfile {
                    bytes_per_second: *bytes_per_second,
                    chunk_bytes: *chunk_bytes,
                });
            }
            FaultAction::Intermittent {
                available,
                blocked,
                direction: TrafficDirection::Downstream,
            } => {
                schedule.intermittent = Some(IntermittentProfile {
                    available: *available,
                    blocked: *blocked,
                });
            }
            FaultAction::DisconnectDuringWrite {
                after_bytes,
                direction: TrafficDirection::Downstream,
            } => schedule.disconnect_after_bytes = Some(*after_bytes),
            FaultAction::CustomStatus(status) => message = with_status(message, *status),
            FaultAction::MockResponse {
                status,
                headers,
                body,
            } => message = mock_response(*status, headers, body.clone()),
            FaultAction::RejectTls
            | FaultAction::DisconnectBeforeUpstream
            | FaultAction::UpstreamConnectTimeout(_)
            | FaultAction::UpstreamWriteTimeout(_)
            | FaultAction::UpstreamReadTimeout(_)
            | FaultAction::Throttle {
                direction: TrafficDirection::Upstream,
                ..
            }
            | FaultAction::Intermittent {
                direction: TrafficDirection::Upstream,
                ..
            }
            | FaultAction::DisconnectDuringWrite {
                direction: TrafficDirection::Upstream,
                ..
            } => return Err(request_stage_fault("processing")),
        }
    }
    Ok(ResponseDisposition::Send { message, schedule })
}

fn declared_content_length(actual: usize, offset: i64) -> Result<usize> {
    let actual = i64::try_from(actual).unwrap_or(i64::MAX);
    let declared = actual.checked_add(offset).ok_or_else(|| {
        ProxyError::new(ErrorCode::ConfigInvalid, "content-length offset overflow")
    })?;
    if declared < 0 {
        return Err(ProxyError::new(
            ErrorCode::ConfigInvalid,
            "content-length cannot be negative",
        ));
    }
    usize::try_from(declared)
        .map_err(|_| ProxyError::new(ErrorCode::ConfigInvalid, "invalid content-length"))
}

fn validate_prefix(prefix: usize, body_len: usize, message: &'static str) -> Result<()> {
    if prefix >= body_len {
        return Err(ProxyError::new(ErrorCode::ConfigInvalid, message));
    }
    Ok(())
}

fn with_status(mut message: Message, status: StatusCode) -> Message {
    let reason = status.canonical_reason().unwrap_or("");
    message.start_line = format!("HTTP/1.1 {} {reason}", status.as_u16());
    if message.uses_transfer_encoding() {
        message.remove_header("content-length");
    }
    message
}

fn request_stage_fault(stage: &str) -> ProxyError {
    ProxyError::new(
        ErrorCode::ConfigInvalid,
        format!("request-stage fault used during response {stage}"),
    )
}
