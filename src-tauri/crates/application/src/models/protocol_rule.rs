//! 协议 Document 规则的独立 IPC 输入与能力目录。
//!
//! 这些类型刻意不复用 HTTP Rule DTO，因此从类型层面不可能携带 Method、Header、
//! Status、JSONPath 或 HTTP Body 等字段。

use serde::{Deserialize, Serialize};
use specta::Type;

use super::{
    DocumentAction, DocumentCondition, ListenerId, ProtocolDocumentRuleId, ProtocolPackageRef,
    ProtocolPackageSchemaFieldTypeViewModel, ProtocolRuleStage,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "snake_case")]
pub enum ProtocolRuleFieldOperatorCapability {
    Equals,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "snake_case")]
pub enum ProtocolRuleFieldActionCapability {
    SetField,
    ClearField,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "snake_case")]
pub enum ProtocolRuleCommonActionCapability {
    RecordMatch,
    ClearDocument,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(deny_unknown_fields)]
pub struct ProtocolRuleFieldCapability {
    pub name: String,
    pub label: String,
    #[serde(rename = "type")]
    pub field_type: ProtocolPackageSchemaFieldTypeViewModel,
    pub operators: Vec<ProtocolRuleFieldOperatorCapability>,
    pub actions: Vec<ProtocolRuleFieldActionCapability>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(deny_unknown_fields)]
pub struct ProtocolRuleCapabilityCatalog {
    pub package: ProtocolPackageRef,
    pub schema_version: u32,
    pub stage: ProtocolRuleStage,
    pub fields: Vec<ProtocolRuleFieldCapability>,
    pub common_actions: Vec<ProtocolRuleCommonActionCapability>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(deny_unknown_fields)]
pub struct ProtocolRuleSaveInput {
    /// `None` 表示创建；更新时必须同时提供规则 ID 与期望 revision。
    pub rule_id: Option<ProtocolDocumentRuleId>,
    pub expected_revision: Option<u64>,
    pub name: String,
    pub enabled: bool,
    pub priority: i32,
    pub listener_id: ListenerId,
    pub package: ProtocolPackageRef,
    pub schema_version: u32,
    pub stage: ProtocolRuleStage,
    pub conditions: Vec<DocumentCondition>,
    pub actions: Vec<DocumentAction>,
}
