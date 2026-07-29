//! Network fault primitives (`ACTION-001` through `ACTION-013`).

use std::time::Duration;

use bytes::Bytes;
use http::{HeaderMap, StatusCode};
use tokio_util::sync::CancellationToken;

use crate::codec;
use crate::message::Message;
use crate::{ErrorCode, ProxyError, Result};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FaultAction {
    RejectTls,
    DisconnectBeforeUpstream,
    UpstreamConnectTimeout,
    UpstreamWriteTimeout,
    UpstreamReadTimeout,
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
    CustomStatus(StatusCode),
}

#[derive(Debug, Clone)]
pub enum ResponseDisposition {
    Send(Message),
    Drop,
    Truncate { message: Message, bytes: usize },
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
                });
            }
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
            | FaultAction::UpstreamConnectTimeout
            | FaultAction::UpstreamWriteTimeout
            | FaultAction::UpstreamReadTimeout => {
                return Err(ProxyError::new(
                    ErrorCode::ConfigInvalid,
                    "request-stage fault used during response processing",
                ));
            }
        }
    }
    Ok(ResponseDisposition::Send(message))
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
        let ResponseDisposition::Send(message) = result else {
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
}
