use crate::{
    ChannelId, DomainError, ErrorCode, MessageStage, Revision, RuleId, RuntimeEpoch,
    TerminalIdentity,
};
use serde::{Deserialize, Deserializer, Serialize};
use serde_json::Value;
use specta::Type;
use uuid::Uuid;

/// 一条规则组合允许累积的最大延迟：10 分钟。
pub const MAX_TOTAL_DELAY_MS: u64 = 600_000;
/// 限速动作允许设置的最高速率。
pub const MAX_THROTTLE_BYTES_PER_SECOND: u64 = 100 * 1024 * 1024;
/// 弱网动作处理的最大分块大小。
pub const MAX_TRAFFIC_CHUNK_BYTES: u64 = 1024 * 1024;

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize, Type)]
pub enum MatchField {
    TerminalIp,
    CertificateFingerprint,
    /// Exact HTTP method token.
    Method,
    /// Request origin-form target: path plus optional query.
    RequestTarget,
    /// One case-insensitive HTTP header name written as a single-segment pointer.
    Header(String),
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize, Type)]
pub enum MatchOperator {
    Equals(String),
    Contains(String),
    StartsWith(String),
    EndsWith(String),
    Wildcard(String),
}

/// Borrowed raw HTTP header used by the pure Domain matcher.
#[derive(Clone, Copy, Debug)]
pub struct HttpHeader<'a> {
    pub name: &'a [u8],
    pub value: &'a [u8],
}

impl<'a> HttpHeader<'a> {
    #[must_use]
    pub const fn new(name: &'a [u8], value: &'a [u8]) -> Self {
        Self { name, value }
    }
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize, Type)]
pub enum DropResponseMode {
    ReadCompleteResponse,
    CloseAfterRequestWrite,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize, Type)]
pub enum TrafficDirection {
    Upstream,
    Downstream,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize, Type)]
pub enum JitterScope {
    BeforeMessage,
    PerChunk,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Type)]
pub enum TerminalAction {
    DisconnectBeforeUpstream,
    UpstreamConnectTimeout {
        milliseconds: u64,
    },
    UpstreamWriteTimeout {
        milliseconds: u64,
    },
    UpstreamReadTimeout {
        milliseconds: u64,
    },
    DropUpstreamResponse {
        mode: DropResponseMode,
    },
    MockResponse {
        status: u16,
        headers: Vec<(String, String)>,
        body: String,
    },
    InvalidJson {
        body_bytes: Vec<u8>,
    },
    IncorrectContentLength {
        delta: i64,
    },
    TruncateResponse {
        bytes: u64,
    },
    DisconnectDuringUpstreamWrite {
        after_bytes: u64,
    },
    DisconnectDuringDownstreamWrite {
        after_bytes: u64,
    },
}

impl<'de> Deserialize<'de> for TerminalAction {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        #[derive(Deserialize)]
        enum Wire {
            DisconnectBeforeUpstream,
            UpstreamConnectTimeout { milliseconds: u64 },
            UpstreamWriteTimeout { milliseconds: u64 },
            UpstreamReadTimeout { milliseconds: u64 },
            DropUpstreamResponse { mode: DropResponseMode },
            MockResponse(MockResponseWire),
            InvalidJson { body_bytes: Vec<u8> },
            IncorrectContentLength { delta: i64 },
            TruncateResponse { bytes: u64 },
            DisconnectDuringUpstreamWrite { after_bytes: u64 },
            DisconnectDuringDownstreamWrite { after_bytes: u64 },
        }

        #[derive(Deserialize)]
        #[serde(deny_unknown_fields)]
        struct MockResponseWire {
            status: u16,
            headers: Vec<(String, String)>,
            #[serde(default)]
            body: Option<String>,
            #[serde(default)]
            body_bytes: Option<Vec<u8>>,
        }

        Ok(match Wire::deserialize(deserializer)? {
            Wire::DisconnectBeforeUpstream => Self::DisconnectBeforeUpstream,
            Wire::UpstreamConnectTimeout { milliseconds } => {
                Self::UpstreamConnectTimeout { milliseconds }
            }
            Wire::UpstreamWriteTimeout { milliseconds } => {
                Self::UpstreamWriteTimeout { milliseconds }
            }
            Wire::UpstreamReadTimeout { milliseconds } => {
                Self::UpstreamReadTimeout { milliseconds }
            }
            Wire::DropUpstreamResponse { mode } => Self::DropUpstreamResponse { mode },
            Wire::MockResponse(wire) => {
                let body = match (wire.body, wire.body_bytes) {
                    (Some(body), None) => body,
                    (None, Some(bytes)) => String::from_utf8(bytes).map_err(|_| {
                        serde::de::Error::custom(
                            "legacy MockResponse body_bytes is not valid UTF-8",
                        )
                    })?,
                    (Some(_), Some(_)) => {
                        return Err(serde::de::Error::custom(
                            "MockResponse cannot contain both body and body_bytes",
                        ));
                    }
                    (None, None) => {
                        return Err(serde::de::Error::missing_field("body"));
                    }
                };
                Self::MockResponse {
                    status: wire.status,
                    headers: wire.headers,
                    body,
                }
            }
            Wire::InvalidJson { body_bytes } => Self::InvalidJson { body_bytes },
            Wire::IncorrectContentLength { delta } => Self::IncorrectContentLength { delta },
            Wire::TruncateResponse { bytes } => Self::TruncateResponse { bytes },
            Wire::DisconnectDuringUpstreamWrite { after_bytes } => {
                Self::DisconnectDuringUpstreamWrite { after_bytes }
            }
            Wire::DisconnectDuringDownstreamWrite { after_bytes } => {
                Self::DisconnectDuringDownstreamWrite { after_bytes }
            }
        })
    }
}

impl TerminalAction {
    pub fn validate_for_body(&self, body_len: usize) -> Result<(), DomainError> {
        let (bytes, field, message) = match self {
            Self::TruncateResponse { bytes } => (*bytes, "bytes", "截断长度"),
            Self::DisconnectDuringUpstreamWrite { after_bytes }
            | Self::DisconnectDuringDownstreamWrite { after_bytes } => {
                (*after_bytes, "after_bytes", "断连偏移")
            }
            _ => return Ok(()),
        };
        let bytes = usize::try_from(bytes).map_err(|_| {
            DomainError::new(ErrorCode::RuleInvalid, format!("{message}超出平台范围"))
                .with_field_error(field, format!("{message}非法"))
        })?;
        if body_len == 0 || bytes >= body_len {
            return Err(DomainError::new(
                ErrorCode::RuleInvalid,
                format!("{message}必须小于 Body 长度"),
            )
            .with_field_error(field, "必须位于 0..body_len-1"));
        }
        Ok(())
    }
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize, Type)]
pub enum HttpAction {
    SetJsonField {
        path: String,
        #[specta(type = specta_typescript::Unknown<Value>)]
        value: Value,
    },
    ReplaceBodyText(String),
    SetHeader {
        name: String,
        value: String,
    },
    Delay {
        milliseconds: u64,
    },
    Jitter {
        minimum_milliseconds: u64,
        maximum_milliseconds: u64,
        scope: JitterScope,
    },
    Throttle {
        bytes_per_second: u64,
        chunk_bytes: u64,
        direction: TrafficDirection,
    },
    Intermittent {
        available_milliseconds: u64,
        blocked_milliseconds: u64,
        direction: TrafficDirection,
    },
    CustomHttpStatus {
        status: u16,
    },
    Terminal(TerminalAction),
}

impl HttpAction {
    #[must_use]
    pub const fn is_terminal(&self) -> bool {
        matches!(self, Self::Terminal(_))
    }
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, Ord, PartialEq, PartialOrd, Serialize, Type)]
pub struct RuleRevisionSignature {
    pub rule_id: RuleId,
    pub revision: Revision,
}

#[derive(Clone, Debug, Default, Deserialize, Eq, PartialEq, Serialize, Type)]
pub struct RuleSetSignature {
    pub entries: Vec<RuleRevisionSignature>,
}

impl RuleSetSignature {
    #[must_use]
    pub fn from_definitions(rules: &[crate::RuleDefinition]) -> Self {
        let mut entries = rules
            .iter()
            .map(|rule| RuleRevisionSignature {
                rule_id: rule.rule_id(),
                revision: rule.revision(),
            })
            .collect::<Vec<_>>();
        entries.sort_unstable();
        Self { entries }
    }
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize, Type)]
pub struct RuleRuntimeSnapshot {
    pub collection_id: Option<Uuid>,
    pub collection_revision: u64,
    pub signature: RuleSetSignature,
    pub rules: Vec<crate::RuleDefinition>,
    pub execution_order: Vec<RuleId>,
}

impl RuleRuntimeSnapshot {
    #[must_use]
    pub fn new(rules: Vec<crate::RuleDefinition>) -> Self {
        Self::with_collection_revision(0, rules)
    }

    #[must_use]
    pub fn with_collection_revision(
        collection_revision: u64,
        rules: Vec<crate::RuleDefinition>,
    ) -> Self {
        Self::with_collection_identity(None, collection_revision, rules)
    }

    #[must_use]
    pub fn with_collection_identity(
        collection_id: Option<Uuid>,
        collection_revision: u64,
        rules: Vec<crate::RuleDefinition>,
    ) -> Self {
        Self {
            collection_id,
            collection_revision,
            signature: RuleSetSignature::from_definitions(&rules),
            rules,
            execution_order: Vec::new(),
        }
    }

    #[must_use]
    pub fn with_collection_identity_and_order(
        collection_id: Option<Uuid>,
        collection_revision: u64,
        rules: Vec<crate::RuleDefinition>,
        execution_order: Vec<RuleId>,
    ) -> Self {
        Self {
            collection_id,
            collection_revision,
            signature: RuleSetSignature::from_definitions(&rules),
            rules,
            execution_order,
        }
    }
}

#[derive(Clone, Debug)]
pub struct MatchContext<'a> {
    pub runtime_epoch: RuntimeEpoch,
    pub channel: ChannelId,
    pub stage: MessageStage,
    pub terminal: &'a TerminalIdentity,
    pub method: Option<&'a str>,
    pub request_target: Option<&'a str>,
    pub headers: &'a [HttpHeader<'a>],
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize, Type)]
pub struct RuleTrace {
    pub rule_id: RuleId,
    pub matched: bool,
    pub reason: String,
    pub actions: Vec<HttpAction>,
}

#[derive(Clone, Debug, Default, Deserialize, Eq, PartialEq, Serialize, Type)]
pub struct RuleEvaluation {
    pub traces: Vec<RuleTrace>,
    pub composed_actions: Vec<HttpAction>,
    pub terminal_action: Option<TerminalAction>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize, Type)]
pub struct RuleConflictWarning {
    pub code: ErrorCode,
    pub shadowing_rule_id: RuleId,
    pub shadowed_rule_id: RuleId,
    pub message: String,
}
