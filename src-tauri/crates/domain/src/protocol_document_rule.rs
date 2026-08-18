//! Schema 驱动的 协议 Document 规则领域模型。
//!
//! 本模块与 HTTP [`crate::Rule`] 完全独立：条件只认识 Schema 字段和值类型，动作只认识
//! Document，不暴露 Method、Header、Status、JSONPath 或 HTTP Body 能力。包安装状态及
//! Manifest 入口能力需要查询外部注册表，仍由 Application 层校验。

use serde::{Deserialize, Serialize};
use specta::Type;

use crate::{
    DocumentFieldName, DocumentSchema, DocumentValue, DomainError, ListenerId,
    ProtocolDocumentRuleId, ProtocolPackageRef, Revision,
};

mod definition;
mod execution;
mod validation;
mod wire;
pub use execution::*;
use validation::{
    add_error, next_rule_revision, rule_error, validate_field_value, validate_structure,
};
use wire::{ProtocolDocumentRuleWire, StrictDocumentValue};

/// 单个 Workspace 最多保存的 协议 Document 规则数。
pub const MAX_PROTOCOL_DOCUMENT_RULES: usize = 1_024;
/// 单条规则最多允许的 AND 条件数。
pub const MAX_PROTOCOL_DOCUMENT_RULE_CONDITIONS: usize = 64;
/// 单条规则最多允许的顺序动作数。
pub const MAX_PROTOCOL_DOCUMENT_RULE_ACTIONS: usize = 64;
/// 规则中单个 UTF-8 文本值的最大字节数（16 KiB）。
pub const MAX_PROTOCOL_DOCUMENT_RULE_STRING_BYTES: usize = 16 * 1_024;
/// 规则中单个 Blob 值的最大字节数（64 KiB）。
pub const MAX_PROTOCOL_DOCUMENT_RULE_BLOB_BYTES: usize = 64 * 1_024;
/// 规则名称的最大 UTF-8 字节数。
pub const MAX_PROTOCOL_DOCUMENT_RULE_NAME_BYTES: usize = 128;

/// Socket Frame 相对于代理的稳定数据方向。
#[derive(Clone, Copy, Debug, Deserialize, Eq, Hash, PartialEq, Serialize, Type)]
#[serde(rename_all = "snake_case")]
pub enum ProtocolDirection {
    /// App 到 Server，或 `LocalResponder` 的请求方向。
    Upstream,
    /// Server 到 App，或 `LocalResponder` 的响应方向。
    Downstream,
}

/// Socket 报文经过代理时可独立配置的处理阶段。
#[derive(Clone, Copy, Debug, Deserialize, Eq, Hash, PartialEq, Serialize, Type)]
#[serde(rename_all = "snake_case")]
pub enum ProtocolRuleStage {
    /// 应用发出的报文刚进入代理。
    AppToProxy,
    /// 代理即将把报文发送给上游服务。
    ProxyToUpstream,
    /// 上游服务返回的报文刚进入代理。
    UpstreamToProxy,
    /// 代理即将把报文返回给应用。
    ProxyToApp,
}

/// v1 Document 条件。多个条件按声明顺序读取并执行 AND；空列表恒匹配。
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Type)]
#[serde(tag = "operator", rename_all = "snake_case")]
pub enum DocumentCondition {
    /// 字段当前值必须与给定类型化值严格相等；不进行文本或数字转换。
    Equals {
        /// Schema 中声明的字段名。
        field: DocumentFieldName,
        /// 参与严格比较的值。
        value: DocumentValue,
    },
}

impl<'de> Deserialize<'de> for DocumentCondition {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        #[derive(Deserialize)]
        #[serde(tag = "operator", rename_all = "snake_case", deny_unknown_fields)]
        enum Wire {
            Equals {
                field: DocumentFieldName,
                value: StrictDocumentValue,
            },
        }
        Ok(match Wire::deserialize(deserializer)? {
            Wire::Equals { field, value } => Self::Equals {
                field,
                value: value.into(),
            },
        })
    }
}

impl DocumentCondition {
    #[must_use]
    /// 返回条件读取的字段。
    pub const fn field(&self) -> &DocumentFieldName {
        match self {
            Self::Equals { field, .. } => field,
        }
    }

    const fn value(&self) -> &DocumentValue {
        match self {
            Self::Equals { value, .. } => value,
        }
    }
}

/// v1 Document 动作。动作按声明顺序执行，不包含隐式终止或 first-match 语义。
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Type)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum DocumentAction {
    /// 只记录命中，不修改 Document。
    RecordMatch,
    /// 只替换一个 Schema 已声明字段的值。
    SetField {
        /// Schema 中声明的目标字段。
        field: DocumentFieldName,
        /// 与字段声明类型严格一致的新值。
        value: DocumentValue,
    },
    /// 清除一个 Schema 已声明字段的值。
    ClearField {
        /// Schema 中声明的目标字段。
        field: DocumentFieldName,
    },
    /// 清空所有字段值槽，但保留当前 Schema 身份和结构。
    ClearDocument,
}

impl<'de> Deserialize<'de> for DocumentAction {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        #[derive(Deserialize)]
        #[serde(tag = "type", rename_all = "snake_case", deny_unknown_fields)]
        enum Wire {
            RecordMatch {},
            SetField {
                field: DocumentFieldName,
                value: StrictDocumentValue,
            },
            ClearField {
                field: DocumentFieldName,
            },
            ClearDocument {},
        }
        Ok(match Wire::deserialize(deserializer)? {
            Wire::RecordMatch {} => Self::RecordMatch,
            Wire::SetField { field, value } => Self::SetField {
                field,
                value: value.into(),
            },
            Wire::ClearField { field } => Self::ClearField { field },
            Wire::ClearDocument {} => Self::ClearDocument,
        })
    }
}

/// 创建或编辑规则时由 Application 提交的字段。
///
/// Draft 不包含实体身份、revision 或 `created_order`。绑定字段在创建后被冻结；更新提交若
/// 试图切换 Listener、协议包、Schema 版本或方向，会由领域层 fail-closed 拒绝。
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize, Type)]
#[serde(deny_unknown_fields)]
pub struct ProtocolDocumentRuleDraft {
    /// 用户可编辑的规则名称。
    pub name: String,
    /// 新规则是否启用。
    pub enabled: bool,
    /// 越小越先执行的显式优先级。
    pub priority: i32,
    /// 冻结绑定的 Socket Listener。
    pub listener_id: ListenerId,
    /// 冻结绑定的精确协议包版本。
    pub package: ProtocolPackageRef,
    /// 冻结绑定的正整数 Schema 版本。
    pub schema_version: u32,
    /// 冻结绑定的处理阶段。
    pub stage: ProtocolRuleStage,
    /// 按声明顺序执行 AND 的条件；允许为空。
    pub conditions: Vec<DocumentCondition>,
    /// 按声明顺序执行的非空动作。
    pub actions: Vec<DocumentAction>,
}

impl DocumentAction {
    fn field_and_value(&self) -> Option<(&DocumentFieldName, &DocumentValue)> {
        match self {
            Self::SetField { field, value } => Some((field, value)),
            Self::RecordMatch | Self::ClearField { .. } | Self::ClearDocument => None,
        }
    }
}

/// 可持久化的 协议 Document 规则实体。
///
/// `rule_id` 与 `created_order` 在更新时保持稳定，`revision` 对每次成功更新或启停递增。
/// 反序列化会重新执行全部结构限制，因此导入不能绕过空动作、重复字段或资源上限。
#[derive(Clone, Debug, Eq, PartialEq, Type)]
pub struct ProtocolDocumentRuleDefinition {
    rule_id: ProtocolDocumentRuleId,
    revision: Revision,
    name: String,
    enabled: bool,
    priority: i32,
    created_order: u64,
    listener_id: ListenerId,
    package: ProtocolPackageRef,
    schema_version: u32,
    stage: ProtocolRuleStage,
    conditions: Vec<DocumentCondition>,
    actions: Vec<DocumentAction>,
}

impl ProtocolDocumentRuleDefinition {
    /// 从 Draft 创建具有新稳定身份和初始 revision 的规则。
    pub fn create(
        draft: ProtocolDocumentRuleDraft,
        created_order: u64,
    ) -> Result<Self, DomainError> {
        Self::new_named_for_stage(
            ProtocolDocumentRuleId::new(),
            draft.name,
            draft.enabled,
            draft.priority,
            created_order,
            draft.listener_id,
            draft.package,
            draft.schema_version,
            draft.stage,
            draft.conditions,
            draft.actions,
        )
    }

    /// 创建 revision 为 1 的规则，并校验所有不依赖 Schema 的持久化不变量。
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        rule_id: ProtocolDocumentRuleId,
        enabled: bool,
        priority: i32,
        created_order: u64,
        listener_id: ListenerId,
        package: ProtocolPackageRef,
        schema_version: u32,
        direction: ProtocolDirection,
        conditions: Vec<DocumentCondition>,
        actions: Vec<DocumentAction>,
    ) -> Result<Self, DomainError> {
        Self::new_named(
            rule_id,
            format!("规则 {created_order}"),
            enabled,
            priority,
            created_order,
            listener_id,
            package,
            schema_version,
            direction,
            conditions,
            actions,
        )
    }

    /// 创建带用户名称、revision 为 1 的规则。
    #[allow(clippy::too_many_arguments)]
    pub fn new_named(
        rule_id: ProtocolDocumentRuleId,
        name: String,
        enabled: bool,
        priority: i32,
        created_order: u64,
        listener_id: ListenerId,
        package: ProtocolPackageRef,
        schema_version: u32,
        direction: ProtocolDirection,
        conditions: Vec<DocumentCondition>,
        actions: Vec<DocumentAction>,
    ) -> Result<Self, DomainError> {
        let stage = match direction {
            ProtocolDirection::Upstream => ProtocolRuleStage::ProxyToUpstream,
            ProtocolDirection::Downstream => ProtocolRuleStage::ProxyToApp,
        };
        Self::new_named_for_stage(
            rule_id,
            name,
            enabled,
            priority,
            created_order,
            listener_id,
            package,
            schema_version,
            stage,
            conditions,
            actions,
        )
    }

    /// 创建绑定到明确处理阶段的规则。
    #[allow(clippy::too_many_arguments)]
    pub fn new_named_for_stage(
        rule_id: ProtocolDocumentRuleId,
        name: String,
        enabled: bool,
        priority: i32,
        created_order: u64,
        listener_id: ListenerId,
        package: ProtocolPackageRef,
        schema_version: u32,
        stage: ProtocolRuleStage,
        conditions: Vec<DocumentCondition>,
        actions: Vec<DocumentAction>,
    ) -> Result<Self, DomainError> {
        Self::from_wire(ProtocolDocumentRuleWire {
            rule_id,
            revision: Revision::INITIAL,
            name,
            enabled,
            priority,
            created_order,
            listener_id,
            package,
            schema_version,
            stage,
            conditions,
            actions,
        })
    }

    /// 用 Draft 更新可编辑内容；身份、创建顺序与 Listener/包/Schema/方向绑定保持不变。
    pub fn update(
        &mut self,
        expected_revision: Revision,
        draft: ProtocolDocumentRuleDraft,
    ) -> Result<Revision, DomainError> {
        self.revision.verify(expected_revision)?;
        if self.listener_id != draft.listener_id
            || self.package != draft.package
            || self.schema_version != draft.schema_version
            || self.stage != draft.stage
        {
            return Err(
                rule_error("规则更新不能切换 Listener、协议包、Schema 或方向")
                    .with_field_error("binding", "请新建规则以使用不同绑定"),
            );
        }
        let next = Self::from_wire(ProtocolDocumentRuleWire {
            rule_id: self.rule_id,
            revision: next_rule_revision(self.revision)?,
            name: draft.name,
            enabled: draft.enabled,
            priority: draft.priority,
            created_order: self.created_order,
            listener_id: self.listener_id,
            package: self.package.clone(),
            schema_version: self.schema_version,
            stage: self.stage,
            conditions: draft.conditions,
            actions: draft.actions,
        })?;
        let revision = next.revision;
        *self = next;
        Ok(revision)
    }

    /// 在乐观锁保护下启用或停用规则，身份及其他配置保持不变。
    pub fn set_enabled(
        &mut self,
        expected_revision: Revision,
        enabled: bool,
    ) -> Result<Revision, DomainError> {
        self.revision.verify(expected_revision)?;
        let revision = next_rule_revision(self.revision)?;
        self.enabled = enabled;
        self.revision = revision;
        Ok(revision)
    }

    /// [`Self::set_enabled`] 的用例语义别名。
    pub fn toggle(
        &mut self,
        expected_revision: Revision,
        enabled: bool,
    ) -> Result<Revision, DomainError> {
        self.set_enabled(expected_revision, enabled)
    }

    /// Workspace 复制/导入在 Listener ID 映射完成后受控替换绑定 Listener。
    ///
    /// 该操作保留 `rule_id`、revision、`created_order`、包、Schema、方向、条件和动作；
    /// 它不是普通规则编辑能力，调用方仍须在 Workspace 聚合校验中确认新 Listener 的拓扑与开关。
    pub fn rebind_listener_for_workspace_remap(
        &mut self,
        new_listener_id: ListenerId,
    ) -> Result<(), DomainError> {
        validate_structure(
            &self.name,
            self.revision,
            self.created_order,
            self.schema_version,
            &self.conditions,
            &self.actions,
        )?;
        self.listener_id = new_listener_id;
        Ok(())
    }

    /// 使用具体 Schema 校验版本、未知字段及严格值类型。
    pub fn validate_against_schema(&self, schema: &DocumentSchema) -> Result<(), DomainError> {
        let mut error = rule_error("协议 Document 规则与 Schema 不兼容");
        if self.schema_version != schema.version() {
            add_error(
                &mut error,
                "schema_version",
                "规则 Schema 版本与绑定 Schema 不一致",
            );
        }
        for (index, condition) in self.conditions.iter().enumerate() {
            validate_field_value(
                schema,
                condition.field(),
                condition.value(),
                &format!("conditions.{index}"),
                &mut error,
            );
        }
        for (index, action) in self.actions.iter().enumerate() {
            if let Some((field, value)) = action.field_and_value() {
                validate_field_value(
                    schema,
                    field,
                    value,
                    &format!("actions.{index}"),
                    &mut error,
                );
            } else if let DocumentAction::ClearField { field } = action
                && schema.field_index(field.as_str()).is_none()
            {
                add_error(
                    &mut error,
                    format!("actions.{index}.field"),
                    "字段未在绑定 Schema 中声明",
                );
            }
        }
        if error.field_errors.is_empty() {
            Ok(())
        } else {
            Err(error)
        }
    }

    fn from_wire(value: ProtocolDocumentRuleWire) -> Result<Self, DomainError> {
        validate_structure(
            &value.name,
            value.revision,
            value.created_order,
            value.schema_version,
            &value.conditions,
            &value.actions,
        )?;
        Ok(Self {
            rule_id: value.rule_id,
            revision: value.revision,
            name: value.name,
            enabled: value.enabled,
            priority: value.priority,
            created_order: value.created_order,
            listener_id: value.listener_id,
            package: value.package,
            schema_version: value.schema_version,
            stage: value.stage,
            conditions: value.conditions,
            actions: value.actions,
        })
    }
}

/// 按 `(priority, created_order, rule_id)` 原地排序，不重写规则身份或创建顺序。
pub fn sort_protocol_document_rules(rules: &mut [ProtocolDocumentRuleDefinition]) {
    rules.sort_by(|left, right| {
        left.priority
            .cmp(&right.priority)
            .then_with(|| left.created_order.cmp(&right.created_order))
            .then_with(|| left.rule_id.cmp(&right.rule_id))
    });
}

#[cfg(test)]
mod tests;
