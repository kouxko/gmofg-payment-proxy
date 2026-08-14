//! 规则嵌套值的严格反序列化 Wire 类型。

use serde::{Deserialize, Serialize};

use super::{DocumentAction, DocumentCondition, SocketDirection, SocketDocumentRuleDefinition};
use crate::{DocumentValue, ListenerId, ProtocolPackageRef, Revision, SocketDocumentRuleId};

#[derive(Deserialize)]
#[serde(
    tag = "type",
    content = "value",
    rename_all = "snake_case",
    deny_unknown_fields
)]
pub(super) enum StrictDocumentValue {
    String(String),
    Int(i64),
    Bool(bool),
    Blob(Vec<u8>),
}

impl From<StrictDocumentValue> for DocumentValue {
    fn from(value: StrictDocumentValue) -> Self {
        match value {
            StrictDocumentValue::String(value) => Self::String(value),
            StrictDocumentValue::Int(value) => Self::Int(value),
            StrictDocumentValue::Bool(value) => Self::Bool(value),
            StrictDocumentValue::Blob(value) => Self::Blob(value),
        }
    }
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub(super) struct SocketDocumentRuleWire {
    pub(super) rule_id: SocketDocumentRuleId,
    pub(super) revision: Revision,
    pub(super) enabled: bool,
    pub(super) priority: i32,
    pub(super) created_order: u64,
    pub(super) listener_id: ListenerId,
    pub(super) package: ProtocolPackageRef,
    pub(super) schema_version: u32,
    pub(super) direction: SocketDirection,
    pub(super) conditions: Vec<DocumentCondition>,
    pub(super) actions: Vec<DocumentAction>,
}

impl TryFrom<SocketDocumentRuleWire> for SocketDocumentRuleDefinition {
    type Error = crate::DomainError;

    fn try_from(value: SocketDocumentRuleWire) -> Result<Self, Self::Error> {
        Self::from_wire(value)
    }
}

impl<'de> Deserialize<'de> for SocketDocumentRuleDefinition {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        use serde::de::Error as _;
        SocketDocumentRuleWire::deserialize(deserializer)?
            .try_into()
            .map_err(D::Error::custom)
    }
}

impl Serialize for SocketDocumentRuleDefinition {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        SocketDocumentRuleWire::from(self.clone()).serialize(serializer)
    }
}

impl From<SocketDocumentRuleDefinition> for SocketDocumentRuleWire {
    fn from(value: SocketDocumentRuleDefinition) -> Self {
        Self {
            rule_id: value.rule_id,
            revision: value.revision,
            enabled: value.enabled,
            priority: value.priority,
            created_order: value.created_order,
            listener_id: value.listener_id,
            package: value.package,
            schema_version: value.schema_version,
            direction: value.direction,
            conditions: value.conditions,
            actions: value.actions,
        }
    }
}
