use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};
use specta::Type;

use super::{ChannelId, Revision, RuleId, UiTone};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "snake_case")]
pub enum FaultParameterKind {
    Boolean,
    Integer,
    Text,
    Json,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(tag = "kind", content = "value", rename_all = "snake_case")]
pub enum FaultParameterValue {
    Boolean(bool),
    Integer(i64),
    Text(String),
    Json(String),
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
pub struct FaultParameterFieldViewModel {
    pub key: String,
    pub label: String,
    pub description: String,
    pub kind: FaultParameterKind,
    pub required: bool,
    pub minimum: Option<i64>,
    pub maximum: Option<i64>,
    pub multiline: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
/// 故障模拟页展示的产品化模板及参数 schema。
pub struct FaultTemplateViewModel {
    pub template_id: String,
    pub name: String,
    pub stage_text: String,
    pub behavior_text: String,
    pub affected_party_text: String,
    pub default_channel: ChannelId,
    pub default_priority: i32,
    pub default_parameters: BTreeMap<String, FaultParameterValue>,
    pub parameter_schema: Vec<FaultParameterFieldViewModel>,
    pub risk_text: String,
    pub ui_tone: UiTone,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Type)]
pub struct FaultConfigurationDraft {
    pub template_id: String,
    pub existing_rule_id: Option<RuleId>,
    pub expected_revision: Option<Revision>,
    pub channel: Option<ChannelId>,
    pub terminal: Option<String>,
    pub target: Option<String>,
    pub priority: i32,
    pub parameters: BTreeMap<String, FaultParameterValue>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
pub struct ActiveFaultViewModel {
    pub rule_id: RuleId,
    pub template_name: String,
    pub target_summary: String,
    pub priority: i32,
    pub hit_count: u64,
    pub enabled: bool,
    pub status_text: String,
    pub ui_tone: UiTone,
    pub revision: Revision,
}
