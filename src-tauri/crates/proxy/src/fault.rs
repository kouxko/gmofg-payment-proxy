//! Network fault primitives (`ACTION-001` through `ACTION-013`).

use std::time::Duration;

use bytes::Bytes;
use http::{HeaderMap, StatusCode};
use tokio_util::sync::CancellationToken;

use crate::codec;
use crate::message::Message;
use crate::traffic::{
    IntermittentProfile, JitterProfile, JitterScope, ThrottleProfile, TrafficDirection,
    TrafficSchedule,
};
use crate::{ErrorCode, ProxyError, Result};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FaultAction {
    RejectTls,
    DisconnectBeforeUpstream,
    UpstreamConnectTimeout(Duration),
    UpstreamWriteTimeout(Duration),
    UpstreamReadTimeout(Duration),
    DropResponse {
        read_upstream: bool,
    },
    MockResponse {
        status: StatusCode,
        headers: HeaderMap,
        shift_jis_body: String,
    },
    InvalidJson {
        shift_jis_text: String,
    },
    ContentLengthOffset(i64),
    TruncateResponse(usize),
    Delay(Duration),
    Jitter {
        minimum: Duration,
        maximum: Duration,
        scope: JitterScope,
        seed: u64,
    },
    Throttle {
        bytes_per_second: u64,
        chunk_bytes: usize,
        direction: TrafficDirection,
    },
    Intermittent {
        available: Duration,
        blocked: Duration,
        direction: TrafficDirection,
    },
    DisconnectDuringWrite {
        after_bytes: usize,
        direction: TrafficDirection,
    },
    CustomStatus(StatusCode),
}

#[derive(Debug, Clone)]
pub enum ResponseDisposition {
    Send {
        message: Message,
        schedule: TrafficSchedule,
    },
    Drop,
    Truncate {
        message: Message,
        bytes: usize,
        schedule: TrafficSchedule,
    },
}

pub async fn cancellable_delay(duration: Duration, cancellation: &CancellationToken) -> Result<()> {
    tokio::select! {
        () = cancellation.cancelled() => Err(ProxyError::new(
            ErrorCode::ProxyStopped,
            "proxy stopped during delay",
        )),
        () = tokio::time::sleep(duration) => Ok(()),
    }
}

pub fn mock_response(
    status: StatusCode,
    headers: &HeaderMap,
    shift_jis_body: &str,
) -> Result<Message> {
    let body = Bytes::from(codec::encode_strict(shift_jis_body)?);
    let mut message = Message::response(status, headers, body);
    message.body_modified = true;
    message.set_content_length(message.body.len());
    Ok(message)
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
            FaultAction::InvalidJson { shift_jis_text } => {
                message.body = Bytes::from(codec::encode_strict(shift_jis_text)?);
                message.body_modified = true;
                message.set_content_length(message.body.len());
            }
            FaultAction::ContentLengthOffset(offset) => {
                let actual = i64::try_from(message.body.len()).unwrap_or(i64::MAX);
                let declared = actual.checked_add(*offset).ok_or_else(|| {
                    ProxyError::new(ErrorCode::ConfigInvalid, "content-length offset overflow")
                })?;
                if declared < 0 {
                    return Err(ProxyError::new(
                        ErrorCode::ConfigInvalid,
                        "content-length cannot be negative",
                    ));
                }
                message.set_content_length(usize::try_from(declared).map_err(|_| {
                    ProxyError::new(ErrorCode::ConfigInvalid, "invalid content-length")
                })?);
            }
            FaultAction::TruncateResponse(bytes) => {
                if *bytes >= message.body.len() {
                    return Err(ProxyError::new(
                        ErrorCode::ConfigInvalid,
                        "truncate length must be in 0..body_len",
                    ));
                }
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
            FaultAction::CustomStatus(status) => {
                let headers = message.header_map()?;
                message = Message::response(*status, &headers, message.body);
                message.body_modified = true;
                message.set_content_length(message.body.len());
            }
            FaultAction::MockResponse {
                status,
                headers,
                shift_jis_body,
            } => {
                message = mock_response(*status, headers, shift_jis_body)?;
            }
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
            } => {
                return Err(ProxyError::new(
                    ErrorCode::ConfigInvalid,
                    "request-stage fault used during response processing",
                ));
            }
        }
    }
    Ok(ResponseDisposition::Send { message, schedule })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn invalid_json_is_encodable_and_length_is_rebuilt() {
        let source = Message::response(StatusCode::OK, &HeaderMap::new(), Bytes::new());
        let result = apply_response_actions(
            source,
            &[FaultAction::InvalidJson {
                shift_jis_text: "{broken".into(),
            }],
            &CancellationToken::new(),
        )
        .await
        .unwrap();
        let ResponseDisposition::Send { message, .. } = result else {
            panic!("expected response");
        };
        assert_eq!(message.declared_content_length(), Some(7));
        assert_eq!(message.decoded_shift_jis().unwrap(), "{broken");
    }

    #[tokio::test]
    async fn truncation_requires_strict_prefix() {
        let source = Message::response(
            StatusCode::OK,
            &HeaderMap::new(),
            Bytes::from_static(b"123"),
        );
        assert!(
            apply_response_actions(
                source,
                &[FaultAction::TruncateResponse(3)],
                &CancellationToken::new()
            )
            .await
            .is_err()
        );
    }

    // ACTION-008~010, ACTION-013, TEST-FAULT:
    // response mutations execute in order and the final declared length is observable on wire.
    #[tokio::test]
    async fn response_status_body_and_declared_length_faults_compose_in_order() {
        let source =
            Message::response(StatusCode::OK, &HeaderMap::new(), Bytes::from_static(b"{}"));
        let result = apply_response_actions(
            source,
            &[
                FaultAction::InvalidJson {
                    shift_jis_text: "{".into(),
                },
                FaultAction::CustomStatus(StatusCode::SERVICE_UNAVAILABLE),
                FaultAction::ContentLengthOffset(5),
            ],
            &CancellationToken::new(),
        )
        .await
        .expect("compose response faults");
        let ResponseDisposition::Send { message, .. } = result else {
            panic!("expected response to be sent");
        };
        assert_eq!(message.start_line, "HTTP/1.1 503 Service Unavailable");
        assert_eq!(message.decoded_shift_jis().expect("Shift-JIS"), "{");
        assert_eq!(message.body.len(), 1);
        assert_eq!(message.declared_content_length(), Some(6));
    }

    // ACTION-006, ENGINE-006, TEST-FAULT:
    // a terminal response disposition prevents every later mutation.
    #[tokio::test]
    async fn drop_response_short_circuits_later_response_actions() {
        let source = Message::response(
            StatusCode::OK,
            &HeaderMap::new(),
            Bytes::from_static(b"original"),
        );
        let result = apply_response_actions(
            source,
            &[
                FaultAction::DropResponse {
                    read_upstream: true,
                },
                FaultAction::InvalidJson {
                    shift_jis_text: "{later".into(),
                },
            ],
            &CancellationToken::new(),
        )
        .await
        .expect("drop response");
        assert!(matches!(result, ResponseDisposition::Drop));
    }

    // ACTION-012, STATE-012, TEST-FAULT:
    // delays remain cancellable and never keep shutdown waiting for the full configured duration.
    #[tokio::test]
    async fn delay_observes_proxy_cancellation() {
        let cancellation = CancellationToken::new();
        cancellation.cancel();
        let source = Message::response(StatusCode::OK, &HeaderMap::new(), Bytes::new());
        let error = apply_response_actions(
            source,
            &[FaultAction::Delay(Duration::from_mins(1))],
            &cancellation,
        )
        .await
        .expect_err("cancelled delay");
        assert_eq!(error.code, ErrorCode::ProxyStopped.as_str());
    }
}
