//! 将领域规则动作翻译为传输运行时可执行的故障动作。
//!
//! 翻译保持规则声明顺序，并明确区分终止动作与可组合动作；无法表示或违反阶段边界的动作
//! 返回配置错误，不能静默忽略。真正的延迟、断开和字节改写由 proxy crate 执行。
//!
//! This module is intentionally stateless. Keeping mutation/encoding and fault
//! mapping separate from the pipeline coordinator makes the product boundary
//! visible and lets the coordinator focus on session and breakpoint lifecycle.

use std::{
    hash::{DefaultHasher, Hash, Hasher},
    time::Duration,
};

use bytes::Bytes;
use intercept_proxy_application::RuleSummaryViewModel;
use intercept_proxy_domain::{
    DropResponseMode, HttpAction, JitterScope as DomainJitterScope, JsonPath,
    MessageStage as DomainMessageStage, TerminalAction, TerminalIdentity,
    TrafficDirection as DomainTrafficDirection,
};
use intercept_proxy_product_api::BodyCodec;
use intercept_proxy_runtime::{
    ConnectionContext, ErrorCode, FaultAction, JitterScope, Message, ProxyError, RawHeader,
    Result as ProxyResult, TrafficDirection,
};

use super::{decode_json, encode_body};

macro_rules! runtime_status {
    ($status:expr) => {
        $status.try_into().map_err(|error| {
            ProxyError::new(
                ErrorCode::ConfigInvalid,
                format!("invalid HTTP status: {error}"),
            )
        })
    };
}

pub(crate) fn apply_rule_actions(
    body_codec: &dyn BodyCodec,
    message: &mut Message,
    actions: &[HttpAction],
    seed: u64,
) -> ProxyResult<(Vec<FaultAction>, bool)> {
    let mut faults = Vec::new();
    let mut pause = false;
    for action in actions {
        match action {
            HttpAction::SetJsonField { path, value } => {
                let mut json = decode_json(body_codec, &message.body)?;
                JsonPath::parse(path)
                    .and_then(|path| path.set(&mut json, value.clone()))
                    .map_err(|error| {
                        ProxyError::new(
                            ErrorCode::ConfigInvalid,
                            format!("JSON path `{path}` is invalid: {error}"),
                        )
                    })?;
                let text = serde_json::to_string(&json).map_err(|error| ProxyError {
                    code: "BODY_ENCODE_FAILED",
                    message: format!("failed to serialize structured body: {error}"),
                    external_package_call: None,
                })?;
                message.replace_body(Bytes::from(encode_body(body_codec, &text)?));
            }
            HttpAction::ReplaceBodyText(text) => {
                message.replace_body(Bytes::from(encode_body(body_codec, text)?));
            }
            HttpAction::SetHeader { name, value } => {
                message.remove_header(name);
                message.headers.push(RawHeader::new(
                    name.as_bytes().to_vec(),
                    value.as_bytes().to_vec(),
                ));
            }
            HttpAction::Delay { milliseconds } => {
                faults.push(FaultAction::Delay(Duration::from_millis(*milliseconds)));
            }
            HttpAction::Jitter {
                minimum_milliseconds,
                maximum_milliseconds,
                scope,
            } => faults.push(FaultAction::Jitter {
                minimum: Duration::from_millis(*minimum_milliseconds),
                maximum: Duration::from_millis(*maximum_milliseconds),
                scope: match scope {
                    DomainJitterScope::BeforeMessage => JitterScope::BeforeMessage,
                    DomainJitterScope::PerChunk => JitterScope::PerChunk,
                },
                seed,
            }),
            HttpAction::Throttle {
                bytes_per_second,
                chunk_bytes,
                direction,
            } => faults.push(FaultAction::Throttle {
                bytes_per_second: *bytes_per_second,
                chunk_bytes: usize::try_from(*chunk_bytes).map_err(|_| {
                    ProxyError::new(ErrorCode::ConfigInvalid, "traffic chunk exceeds platform")
                })?,
                direction: traffic_direction(*direction),
            }),
            HttpAction::Intermittent {
                available_milliseconds,
                blocked_milliseconds,
                direction,
            } => faults.push(FaultAction::Intermittent {
                available: Duration::from_millis(*available_milliseconds),
                blocked: Duration::from_millis(*blocked_milliseconds),
                direction: traffic_direction(*direction),
            }),
            HttpAction::Pause => pause = true,
            HttpAction::CustomHttpStatus { status } => {
                faults.push(FaultAction::CustomStatus(runtime_status!(*status)?));
            }
            HttpAction::Terminal(terminal) => faults.push(map_terminal_action(terminal)?),
        }
    }
    if message.body_modified {
        message.set_content_length(message.body.len());
    }
    Ok((faults, pause))
}

pub(super) fn weak_network_seed(
    context: &ConnectionContext,
    stage: DomainMessageStage,
    hit_rules: &[RuleSummaryViewModel],
) -> u64 {
    let mut hasher = DefaultHasher::new();
    context.runtime_epoch.hash(&mut hasher);
    context.connection_id.hash(&mut hasher);
    std::mem::discriminant(&stage).hash(&mut hasher);
    for rule in hit_rules {
        rule.rule_id.hash(&mut hasher);
        rule.revision.hash(&mut hasher);
        rule.hit_count.hash(&mut hasher);
    }
    hasher.finish()
}

pub(super) fn terminal_identity(context: &ConnectionContext) -> TerminalIdentity {
    TerminalIdentity {
        source_ip: context.peer_addr.ip().to_string(),
        certificate_sha256: context
            .tls_peer
            .as_ref()
            .map_or_else(String::new, |identity| identity.sha256_fingerprint.clone()),
    }
}

const fn traffic_direction(direction: DomainTrafficDirection) -> TrafficDirection {
    match direction {
        DomainTrafficDirection::Upstream => TrafficDirection::Upstream,
        DomainTrafficDirection::Downstream => TrafficDirection::Downstream,
    }
}

pub(super) fn map_terminal_action(action: &TerminalAction) -> ProxyResult<FaultAction> {
    Ok(match action {
        TerminalAction::RejectTlsHandshake => FaultAction::RejectTls,
        TerminalAction::DisconnectBeforeUpstream => FaultAction::DisconnectBeforeUpstream,
        TerminalAction::UpstreamConnectTimeout { milliseconds } => {
            FaultAction::UpstreamConnectTimeout(Duration::from_millis(*milliseconds))
        }
        TerminalAction::UpstreamWriteTimeout { milliseconds } => {
            FaultAction::UpstreamWriteTimeout(Duration::from_millis(*milliseconds))
        }
        TerminalAction::UpstreamReadTimeout { milliseconds } => {
            FaultAction::UpstreamReadTimeout(Duration::from_millis(*milliseconds))
        }
        TerminalAction::DropUpstreamResponse { mode } => FaultAction::DropResponse {
            read_upstream: *mode == DropResponseMode::ReadCompleteResponse,
        },
        TerminalAction::MockResponse {
            status,
            headers,
            body_bytes,
        } => FaultAction::MockResponse {
            status: runtime_status!(*status)?,
            headers: Message {
                start_line: String::new(),
                headers: headers
                    .iter()
                    .map(|(name, value)| {
                        RawHeader::new(name.as_bytes().to_vec(), value.as_bytes().to_vec())
                    })
                    .collect(),
                body: Vec::new().into(),
                body_modified: false,
            }
            .header_map()?,
            body: Bytes::copy_from_slice(body_bytes),
        },
        TerminalAction::InvalidJson { body_bytes } => FaultAction::ReplaceBody {
            body: Bytes::copy_from_slice(body_bytes),
        },
        TerminalAction::IncorrectContentLength { delta } => {
            FaultAction::ContentLengthOffset(*delta)
        }
        TerminalAction::TruncateResponse { bytes } => {
            FaultAction::TruncateResponse(usize::try_from(*bytes).map_err(|_| {
                ProxyError::new(
                    ErrorCode::ConfigInvalid,
                    "truncate size exceeds platform range",
                )
            })?)
        }
        TerminalAction::DisconnectDuringUpstreamWrite { after_bytes } => {
            FaultAction::DisconnectDuringWrite {
                after_bytes: usize::try_from(*after_bytes).map_err(|_| {
                    ProxyError::new(
                        ErrorCode::ConfigInvalid,
                        "upstream disconnect offset exceeds platform range",
                    )
                })?,
                direction: TrafficDirection::Upstream,
            }
        }
        TerminalAction::DisconnectDuringDownstreamWrite { after_bytes } => {
            FaultAction::DisconnectDuringWrite {
                after_bytes: usize::try_from(*after_bytes).map_err(|_| {
                    ProxyError::new(
                        ErrorCode::ConfigInvalid,
                        "downstream disconnect offset exceeds platform range",
                    )
                })?,
                direction: TrafficDirection::Downstream,
            }
        }
    })
}
