//! 规则嵌套值的严格反序列化 Wire 类型。

use serde::{Deserialize, Serialize};

use super::{
    ProtocolDocumentOperation, ProtocolDocumentPredicate, ProtocolDocumentRuleDefinition,
    ProtocolRuleStage,
};
use crate::{DocumentValue, ListenerId, ProtocolDocumentRuleId, ProtocolPackageRef, Revision};

#[derive(Deserialize)]
#[serde(transparent)]
pub(super) struct StrictDocumentValue(DocumentValue);

impl From<StrictDocumentValue> for DocumentValue {
    fn from(value: StrictDocumentValue) -> Self {
        value.0
    }
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub(super) struct ProtocolDocumentRuleWire {
    pub(super) rule_id: ProtocolDocumentRuleId,
    pub(super) revision: Revision,
    pub(super) name: String,
    pub(super) enabled: bool,
    pub(super) priority: i32,
    pub(super) created_order: u64,
    pub(super) listener_id: ListenerId,
    pub(super) package: ProtocolPackageRef,
    pub(super) stage: ProtocolRuleStage,
    pub(super) conditions: Vec<ProtocolDocumentPredicate>,
    pub(super) actions: Vec<ProtocolDocumentOperation>,
}

impl TryFrom<ProtocolDocumentRuleWire> for ProtocolDocumentRuleDefinition {
    type Error = crate::DomainError;

    fn try_from(value: ProtocolDocumentRuleWire) -> Result<Self, Self::Error> {
        Self::from_wire(value)
    }
}

impl<'de> Deserialize<'de> for ProtocolDocumentRuleDefinition {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        use serde::de::Error as _;
        ProtocolDocumentRuleWire::deserialize(deserializer)?
            .try_into()
            .map_err(D::Error::custom)
    }
}

impl Serialize for ProtocolDocumentRuleDefinition {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        ProtocolDocumentRuleWire::from(self.clone()).serialize(serializer)
    }
}

impl From<ProtocolDocumentRuleDefinition> for ProtocolDocumentRuleWire {
    fn from(value: ProtocolDocumentRuleDefinition) -> Self {
        Self {
            rule_id: value.rule_id,
            revision: value.revision,
            name: value.name,
            enabled: value.enabled,
            priority: value.priority,
            created_order: value.created_order,
            listener_id: value.listener_id,
            package: value.package,
            stage: value.stage,
            conditions: value.conditions,
            actions: value.actions,
        }
    }
}
