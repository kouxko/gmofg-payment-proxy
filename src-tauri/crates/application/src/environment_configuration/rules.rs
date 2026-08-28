use intercept_proxy_domain::{
    DocumentAction, DocumentCondition, DropResponseMode, JitterScope, MatchCondition, MatchField,
    MatchOperator, RuleAction, TerminalAction, TrafficDirection,
};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use uuid::Uuid;

use super::listener::ProtocolPackageExactRef;

mod projection;
mod validation;

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub(super) struct HttpRuleTemplate {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(super) existing_rule_id: Option<Uuid>,
    name: String,
    description: String,
    enabled: bool,
    priority: u32,
    listener_alias: String,
    stage: HttpRuleStage,
    conditions: Vec<HttpMatchConditionTemplate>,
    actions: Vec<HttpRuleActionTemplate>,
    #[serde(skip_serializing_if = "Option::is_none")]
    document: Option<HttpDocumentRuleTemplate>,
    one_shot: bool,
}

impl HttpRuleTemplate {
    pub(super) fn listener_alias(&self) -> &str {
        &self.listener_alias
    }
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
struct HttpDocumentRuleTemplate {
    package: ProtocolPackageExactRef,
    schema_version: u32,
    conditions: Vec<DocumentCondition>,
    actions: Vec<DocumentAction>,
}

#[derive(Clone, Copy, Debug, Deserialize, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
enum HttpRuleStage {
    AppToProxy,
    ProxyToUpstream,
    UpstreamToProxy,
    ProxyToApp,
    TlsHandshake,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
enum HttpMatchConditionTemplate {
    Field(StrictFieldCondition),
    NthHit(u64),
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
struct StrictFieldCondition {
    field: HttpMatchFieldTemplate,
    operator: HttpMatchOperatorTemplate,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
enum HttpMatchFieldTemplate {
    TerminalIp,
    CertificateFingerprint,
    PathOrRequestType,
    JsonPath(String),
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
enum HttpMatchOperatorTemplate {
    Equals(String),
    Contains(String),
    Regex(String),
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
enum HttpRuleActionTemplate {
    SetJsonField(StrictSetJsonField),
    ReplaceBodyText(String),
    SetHeader(StrictSetHeader),
    Delay(StrictMilliseconds),
    Jitter(StrictJitter),
    Throttle(StrictThrottle),
    Intermittent(StrictIntermittent),
    Pause,
    CustomHttpStatus(StrictHttpStatus),
    Terminal(HttpTerminalActionTemplate),
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
struct StrictSetJsonField {
    path: String,
    value: Value,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
struct StrictSetHeader {
    name: String,
    value: String,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
struct StrictMilliseconds {
    milliseconds: u64,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
struct StrictJitter {
    minimum_milliseconds: u64,
    maximum_milliseconds: u64,
    scope: HttpJitterScopeTemplate,
}

#[derive(Clone, Copy, Debug, Deserialize, PartialEq, Serialize)]
enum HttpJitterScopeTemplate {
    BeforeMessage,
    PerChunk,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
struct StrictThrottle {
    bytes_per_second: u64,
    chunk_bytes: u64,
    direction: HttpTrafficDirectionTemplate,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
struct StrictIntermittent {
    available_milliseconds: u64,
    blocked_milliseconds: u64,
    direction: HttpTrafficDirectionTemplate,
}

#[derive(Clone, Copy, Debug, Deserialize, PartialEq, Serialize)]
enum HttpTrafficDirectionTemplate {
    Upstream,
    Downstream,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
struct StrictHttpStatus {
    status: u16,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
enum HttpTerminalActionTemplate {
    RejectTlsHandshake,
    DisconnectBeforeUpstream,
    UpstreamConnectTimeout(StrictMilliseconds),
    UpstreamWriteTimeout(StrictMilliseconds),
    UpstreamReadTimeout(StrictMilliseconds),
    DropUpstreamResponse(StrictDropResponse),
    MockResponse(StrictMockResponse),
    InvalidJson(StrictBodyBytes),
    IncorrectContentLength(StrictContentLengthDelta),
    TruncateResponse(StrictResponseBytes),
    DisconnectDuringUpstreamWrite(StrictAfterBytes),
    DisconnectDuringDownstreamWrite(StrictAfterBytes),
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
struct StrictDropResponse {
    mode: HttpDropResponseModeTemplate,
}

#[derive(Clone, Copy, Debug, Deserialize, PartialEq, Serialize)]
enum HttpDropResponseModeTemplate {
    ReadCompleteResponse,
    CloseAfterRequestWrite,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
struct StrictMockResponse {
    status: u16,
    headers: Vec<(String, String)>,
    body_bytes: Vec<u8>,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
struct StrictBodyBytes {
    body_bytes: Vec<u8>,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
struct StrictContentLengthDelta {
    delta: i64,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
struct StrictResponseBytes {
    bytes: u64,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
struct StrictAfterBytes {
    after_bytes: u64,
}

impl From<HttpMatchConditionTemplate> for MatchCondition {
    fn from(value: HttpMatchConditionTemplate) -> Self {
        match value {
            HttpMatchConditionTemplate::Field(condition) => Self::Field {
                field: condition.field.into(),
                operator: condition.operator.into(),
            },
            HttpMatchConditionTemplate::NthHit(nth) => Self::NthHit(nth),
        }
    }
}

impl From<HttpMatchFieldTemplate> for MatchField {
    fn from(value: HttpMatchFieldTemplate) -> Self {
        match value {
            HttpMatchFieldTemplate::TerminalIp => Self::TerminalIp,
            HttpMatchFieldTemplate::CertificateFingerprint => Self::CertificateFingerprint,
            HttpMatchFieldTemplate::PathOrRequestType => Self::PathOrRequestType,
            HttpMatchFieldTemplate::JsonPath(path) => Self::JsonPath(path),
        }
    }
}

impl From<HttpMatchOperatorTemplate> for MatchOperator {
    fn from(value: HttpMatchOperatorTemplate) -> Self {
        match value {
            HttpMatchOperatorTemplate::Equals(expected) => Self::Equals(expected),
            HttpMatchOperatorTemplate::Contains(expected) => Self::Contains(expected),
            HttpMatchOperatorTemplate::Regex(pattern) => Self::Regex(pattern),
        }
    }
}

impl From<HttpRuleActionTemplate> for RuleAction {
    fn from(value: HttpRuleActionTemplate) -> Self {
        match value {
            HttpRuleActionTemplate::SetJsonField(value) => Self::SetJsonField {
                path: value.path,
                value: value.value,
            },
            HttpRuleActionTemplate::ReplaceBodyText(value) => Self::ReplaceBodyText(value),
            HttpRuleActionTemplate::SetHeader(value) => Self::SetHeader {
                name: value.name,
                value: value.value,
            },
            HttpRuleActionTemplate::Delay(value) => Self::Delay {
                milliseconds: value.milliseconds,
            },
            HttpRuleActionTemplate::Jitter(value) => Self::Jitter {
                minimum_milliseconds: value.minimum_milliseconds,
                maximum_milliseconds: value.maximum_milliseconds,
                scope: value.scope.into(),
            },
            HttpRuleActionTemplate::Throttle(value) => Self::Throttle {
                bytes_per_second: value.bytes_per_second,
                chunk_bytes: value.chunk_bytes,
                direction: value.direction.into(),
            },
            HttpRuleActionTemplate::Intermittent(value) => Self::Intermittent {
                available_milliseconds: value.available_milliseconds,
                blocked_milliseconds: value.blocked_milliseconds,
                direction: value.direction.into(),
            },
            HttpRuleActionTemplate::Pause => Self::Pause,
            HttpRuleActionTemplate::CustomHttpStatus(value) => Self::CustomHttpStatus {
                status: value.status,
            },
            HttpRuleActionTemplate::Terminal(value) => Self::Terminal(value.into()),
        }
    }
}

impl From<HttpJitterScopeTemplate> for JitterScope {
    fn from(value: HttpJitterScopeTemplate) -> Self {
        match value {
            HttpJitterScopeTemplate::BeforeMessage => Self::BeforeMessage,
            HttpJitterScopeTemplate::PerChunk => Self::PerChunk,
        }
    }
}

impl From<HttpTrafficDirectionTemplate> for TrafficDirection {
    fn from(value: HttpTrafficDirectionTemplate) -> Self {
        match value {
            HttpTrafficDirectionTemplate::Upstream => Self::Upstream,
            HttpTrafficDirectionTemplate::Downstream => Self::Downstream,
        }
    }
}

impl From<HttpTerminalActionTemplate> for TerminalAction {
    fn from(value: HttpTerminalActionTemplate) -> Self {
        match value {
            HttpTerminalActionTemplate::RejectTlsHandshake => Self::RejectTlsHandshake,
            HttpTerminalActionTemplate::DisconnectBeforeUpstream => Self::DisconnectBeforeUpstream,
            HttpTerminalActionTemplate::UpstreamConnectTimeout(value) => {
                Self::UpstreamConnectTimeout {
                    milliseconds: value.milliseconds,
                }
            }
            HttpTerminalActionTemplate::UpstreamWriteTimeout(value) => Self::UpstreamWriteTimeout {
                milliseconds: value.milliseconds,
            },
            HttpTerminalActionTemplate::UpstreamReadTimeout(value) => Self::UpstreamReadTimeout {
                milliseconds: value.milliseconds,
            },
            HttpTerminalActionTemplate::DropUpstreamResponse(value) => Self::DropUpstreamResponse {
                mode: value.mode.into(),
            },
            HttpTerminalActionTemplate::MockResponse(value) => Self::MockResponse {
                status: value.status,
                headers: value.headers,
                body_bytes: value.body_bytes,
            },
            HttpTerminalActionTemplate::InvalidJson(value) => Self::InvalidJson {
                body_bytes: value.body_bytes,
            },
            HttpTerminalActionTemplate::IncorrectContentLength(value) => {
                Self::IncorrectContentLength { delta: value.delta }
            }
            HttpTerminalActionTemplate::TruncateResponse(value) => {
                Self::TruncateResponse { bytes: value.bytes }
            }
            HttpTerminalActionTemplate::DisconnectDuringUpstreamWrite(value) => {
                Self::DisconnectDuringUpstreamWrite {
                    after_bytes: value.after_bytes,
                }
            }
            HttpTerminalActionTemplate::DisconnectDuringDownstreamWrite(value) => {
                Self::DisconnectDuringDownstreamWrite {
                    after_bytes: value.after_bytes,
                }
            }
        }
    }
}

impl From<HttpDropResponseModeTemplate> for DropResponseMode {
    fn from(value: HttpDropResponseModeTemplate) -> Self {
        match value {
            HttpDropResponseModeTemplate::ReadCompleteResponse => Self::ReadCompleteResponse,
            HttpDropResponseModeTemplate::CloseAfterRequestWrite => Self::CloseAfterRequestWrite,
        }
    }
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub(super) struct ProtocolDocumentRuleTemplate {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(super) existing_rule_id: Option<Uuid>,
    name: String,
    enabled: bool,
    priority: i32,
    listener_alias: String,
    package: ProtocolPackageExactRef,
    schema_version: u32,
    stage: ProtocolRuleStage,
    conditions: Vec<DocumentCondition>,
    actions: Vec<DocumentAction>,
}

impl ProtocolDocumentRuleTemplate {
    pub(super) const fn package_ref(&self) -> &ProtocolPackageExactRef {
        &self.package
    }

    pub(super) fn listener_alias(&self) -> &str {
        &self.listener_alias
    }
}

#[derive(Clone, Copy, Debug, Deserialize, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
enum ProtocolRuleStage {
    AppToProxy,
    ProxyToUpstream,
    UpstreamToProxy,
    ProxyToApp,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub(super) enum RuleTemplate {
    Http(HttpRuleTemplate),
    Socket(ProtocolDocumentRuleTemplate),
}

impl RuleTemplate {
    pub(super) fn existing_rule_id(&self) -> Option<Uuid> {
        match self {
            Self::Http(rule) => rule.existing_rule_id,
            Self::Socket(rule) => rule.existing_rule_id,
        }
    }

    pub(super) fn listener_alias(&self) -> &str {
        match self {
            Self::Http(rule) => rule.listener_alias(),
            Self::Socket(rule) => rule.listener_alias(),
        }
    }

    pub(super) fn package_ref(&self) -> Option<&ProtocolPackageExactRef> {
        match self {
            Self::Http(rule) => rule.document.as_ref().map(|document| &document.package),
            Self::Socket(rule) => Some(rule.package_ref()),
        }
    }

    pub(super) const fn as_http(&self) -> Option<&HttpRuleTemplate> {
        match self {
            Self::Http(rule) => Some(rule),
            Self::Socket(_) => None,
        }
    }

    pub(super) const fn as_socket(&self) -> Option<&ProtocolDocumentRuleTemplate> {
        match self {
            Self::Http(_) => None,
            Self::Socket(rule) => Some(rule),
        }
    }
}
