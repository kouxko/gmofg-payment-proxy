//! Socket Document 规则的独立 IPC 输入与能力目录。
//!
//! 这些类型刻意不复用 HTTP Rule DTO，因此从类型层面不可能携带 Method、Header、
//! Status、JSONPath 或 HTTP Body 等字段。

use serde::{Deserialize, Serialize};
use specta::Type;

use super::{
    DocumentAction, DocumentCondition, ListenerId, ProtocolPackageRef,
    ProtocolPackageSchemaFieldTypeViewModel, SocketDirection, SocketDocumentRuleId,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "snake_case")]
pub enum SocketRuleFieldOperatorCapability {
    Equals,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "snake_case")]
pub enum SocketRuleFieldActionCapability {
    SetField,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "snake_case")]
pub enum SocketRuleCommonActionCapability {
    RecordMatch,
    ClearDocument,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(deny_unknown_fields)]
pub struct SocketRuleFieldCapability {
    pub name: String,
    pub label: String,
    #[serde(rename = "type")]
    pub field_type: ProtocolPackageSchemaFieldTypeViewModel,
    pub operators: Vec<SocketRuleFieldOperatorCapability>,
    pub actions: Vec<SocketRuleFieldActionCapability>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(deny_unknown_fields)]
pub struct SocketRuleCapabilityCatalog {
    pub package: ProtocolPackageRef,
    pub schema_version: u32,
    pub direction: SocketDirection,
    pub fields: Vec<SocketRuleFieldCapability>,
    pub common_actions: Vec<SocketRuleCommonActionCapability>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(deny_unknown_fields)]
pub struct SocketRuleSaveInput {
    /// `None` 表示创建；更新时必须同时提供规则 ID 与期望 revision。
    pub rule_id: Option<SocketDocumentRuleId>,
    pub expected_revision: Option<u64>,
    pub enabled: bool,
    pub priority: i32,
    pub listener_id: ListenerId,
    pub package: ProtocolPackageRef,
    pub schema_version: u32,
    pub direction: SocketDirection,
    pub conditions: Vec<DocumentCondition>,
    pub actions: Vec<DocumentAction>,
}
