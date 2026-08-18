//! 不依赖网络或脚本引擎的 Socket Document 规则执行核心。

use std::{collections::BTreeSet, fmt};

use super::SocketRuleStage;
use super::{
    DocumentAction, DocumentCondition, MAX_SOCKET_DOCUMENT_RULES, SocketDirection,
    SocketDocumentRuleDefinition, sort_socket_document_rules,
};
use crate::{
    Document, DocumentSchema, DomainError, ErrorCode, ListenerId, ProtocolPackageRef,
    SocketDocumentRuleId,
};

/// 已冻结、可安全跨连接共享的 Socket Document 规则程序。
///
/// Program 在构造时保存精确 Listener、协议包版本、完整 Schema 和方向绑定，并把规则按
/// `(priority, created_order, rule_id)` 排序。字段均不可从外部修改，因此同一 Program 可由
/// 多个连接并发调用；每次执行的唯一可变状态都属于传入的 owned [`Document`]。
#[derive(Clone)]
pub struct SocketDocumentRuleProgram {
    listener_id: ListenerId,
    package: ProtocolPackageRef,
    schema: DocumentSchema,
    stage: SocketRuleStage,
    rules: Vec<SocketDocumentRuleDefinition>,
}

impl fmt::Debug for SocketDocumentRuleProgram {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("SocketDocumentRuleProgram")
            .field("listener_id", &self.listener_id)
            .field("package", &self.package)
            .field("schema_id", &self.schema.id())
            .field("schema_version", &self.schema.version())
            .field("stage", &self.stage)
            .field("rule_count", &self.rules.len())
            .finish()
    }
}

impl SocketDocumentRuleProgram {
    /// 校验精确绑定、Schema 和规则身份后创建不可变程序。
    ///
    /// 构造采用 fail-closed 语义：即使规则当前处于 disabled 状态，也必须属于同一绑定并通过
    /// Schema 校验；重复规则 ID 或超过 Workspace 上限的快照同样会被拒绝。成功后内部规则
    /// 已完成确定性排序，执行阶段不会再依赖仓库、Workspace 或调用方输入顺序。
    pub fn new(
        listener_id: ListenerId,
        package: ProtocolPackageRef,
        schema: DocumentSchema,
        direction: SocketDirection,
        rules: Vec<SocketDocumentRuleDefinition>,
    ) -> Result<Self, DomainError> {
        let stage = match direction {
            SocketDirection::Upstream => SocketRuleStage::ProxyToUpstream,
            SocketDirection::Downstream => SocketRuleStage::ProxyToApp,
        };
        Self::new_for_stage(listener_id, package, schema, stage, rules)
    }

    /// 创建绑定到明确处理阶段的不可变程序。
    pub fn new_for_stage(
        listener_id: ListenerId,
        package: ProtocolPackageRef,
        schema: DocumentSchema,
        stage: SocketRuleStage,
        mut rules: Vec<SocketDocumentRuleDefinition>,
    ) -> Result<Self, DomainError> {
        validate_rule_snapshot(listener_id, &package, &schema, stage, &rules)?;
        sort_socket_document_rules(&mut rules);
        Ok(Self {
            listener_id,
            package,
            schema,
            stage,
            rules,
        })
    }

    /// 在私有工作 Document 上按稳定顺序执行全部已启用且命中的规则。
    ///
    /// 条件读取当前工作副本，采用 AND 语义；空条件恒匹配，已声明但未赋值的字段只会让该
    /// 条件不匹配。命中规则的动作按声明顺序执行，后续规则因此可以观察并覆盖前序规则的值。
    /// 传入值由本方法独占，任一步失败时整个工作副本被丢弃，不会返回半修改结果。
    pub fn execute(&self, document: Document) -> Result<SocketDocumentRuleExecution, DomainError> {
        self.execute_with_cancellation(document, || false)
    }

    /// 在可取消边界上执行全部规则。
    ///
    /// `is_cancelled` 在 Schema 校验前、每条规则、每个条件、每个动作以及成功提交前调用。
    /// 调用方可以把连接级取消令牌适配为该闭包，Domain 本身仍不依赖异步运行时或网络身份。
    /// 一旦闭包返回 `true`，当前 owned 工作副本立即丢弃，并返回稳定的取消错误。
    pub fn execute_with_cancellation(
        &self,
        document: Document,
        mut is_cancelled: impl FnMut() -> bool,
    ) -> Result<SocketDocumentRuleExecution, DomainError> {
        ensure_not_cancelled(&mut is_cancelled)?;
        if document.schema() != &self.schema {
            return Err(
                rule_program_error("Document 与规则程序绑定的 Schema 不一致")
                    .with_field_error("document.schema", "必须使用程序构造时绑定的完整 Schema"),
            );
        }

        // owned Document 本身就是本次调用独享的工作副本。这里不把它放入 Program，也不使用
        // interior mutability，因此连续 Frame 和并发连接之间不存在可串用的执行状态。
        let mut working = document;
        let mut matched_rule_ids = Vec::new();
        for rule in &self.rules {
            ensure_not_cancelled(&mut is_cancelled)?;
            if !rule.enabled() || !matches_rule(rule, &working, &mut is_cancelled)? {
                continue;
            }
            apply_actions(rule.actions(), &mut working, &mut is_cancelled)?;
            matched_rule_ids.push(rule.rule_id());
        }
        // 最后一次检查是提交边界：即使取消恰好发生在最后一个动作完成后，也不会把该工作副本
        // 作为成功结果泄漏出去。
        ensure_not_cancelled(&mut is_cancelled)?;

        Ok(SocketDocumentRuleExecution {
            document: working,
            matched_rule_ids,
        })
    }

    /// 返回程序绑定的 Listener。
    #[must_use]
    pub const fn listener_id(&self) -> ListenerId {
        self.listener_id
    }

    /// 返回程序绑定的精确协议包 ID 与版本。
    #[must_use]
    pub const fn package(&self) -> &ProtocolPackageRef {
        &self.package
    }

    /// 返回程序绑定的完整 Schema。
    #[must_use]
    pub const fn schema(&self) -> &DocumentSchema {
        &self.schema
    }

    /// 返回程序绑定的数据方向。
    #[must_use]
    pub const fn direction(&self) -> SocketDirection {
        self.stage.direction()
    }

    #[must_use]
    pub const fn stage(&self) -> SocketRuleStage {
        self.stage
    }

    /// 返回已经按稳定执行顺序冻结的规则。
    #[must_use]
    pub fn rules(&self) -> &[SocketDocumentRuleDefinition] {
        &self.rules
    }
}

/// 一次完整规则执行的唯一成功结果。
///
/// 结果只包含最终 Document 和按实际执行顺序记录的命中规则 ID；它不包含一次规则一次回复的
/// 含义，外层始终应在整组规则完成后统一 Encode 或 Echo 一次。
#[derive(Clone, Eq, PartialEq)]
pub struct SocketDocumentRuleExecution {
    document: Document,
    matched_rule_ids: Vec<SocketDocumentRuleId>,
}

impl fmt::Debug for SocketDocumentRuleExecution {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("SocketDocumentRuleExecution")
            .field("schema_id", &self.document.schema().id())
            .field("schema_version", &self.document.schema().version())
            .field("matched_rule_ids", &self.matched_rule_ids)
            .finish()
    }
}

impl SocketDocumentRuleExecution {
    /// 借用规则执行后的最终 Document。
    #[must_use]
    pub const fn document(&self) -> &Document {
        &self.document
    }

    /// 消费结果并取得最终 Document。
    #[must_use]
    pub fn into_document(self) -> Document {
        self.document
    }

    /// 按实际执行顺序返回所有命中规则；disabled 或未匹配规则不会出现。
    #[must_use]
    pub fn matched_rule_ids(&self) -> &[SocketDocumentRuleId] {
        &self.matched_rule_ids
    }

    /// 消费结果并同时取得最终 Document 与命中规则列表。
    #[must_use]
    pub fn into_parts(self) -> (Document, Vec<SocketDocumentRuleId>) {
        (self.document, self.matched_rule_ids)
    }
}

fn validate_rule_snapshot(
    listener_id: ListenerId,
    package: &ProtocolPackageRef,
    schema: &DocumentSchema,
    stage: SocketRuleStage,
    rules: &[SocketDocumentRuleDefinition],
) -> Result<(), DomainError> {
    if rules.len() > MAX_SOCKET_DOCUMENT_RULES {
        return Err(rule_program_error("规则快照超过运行时上限")
            .with_field_error("rules", "规则数量不能超过 1024 条"));
    }

    let mut rule_ids = BTreeSet::new();
    for (index, rule) in rules.iter().enumerate() {
        let prefix = format!("rules.{index}");
        if !rule_ids.insert(rule.rule_id()) {
            return Err(rule_program_error("规则快照包含重复身份")
                .with_field_error(format!("{prefix}.rule_id"), "规则 ID 不能重复"));
        }
        if rule.listener_id() != listener_id
            || rule.package() != package
            || rule.schema_version() != schema.version()
            || rule.stage() != stage
        {
            return Err(
                rule_program_error("规则与程序的精确绑定不一致").with_field_error(
                    format!("{prefix}.binding"),
                    "Listener、包版本、Schema 版本和方向必须全部一致",
                ),
            );
        }
        rule.validate_against_schema(schema).map_err(|error| {
            rule_program_error("规则快照与程序 Schema 不兼容").with_field_error(
                format!("{prefix}.schema"),
                format!("{}: {}", error.code, error.message),
            )
        })?;
    }
    Ok(())
}

fn matches_rule(
    rule: &SocketDocumentRuleDefinition,
    document: &Document,
    is_cancelled: &mut impl FnMut() -> bool,
) -> Result<bool, DomainError> {
    for condition in rule.conditions() {
        ensure_not_cancelled(is_cancelled)?;
        match condition {
            DocumentCondition::Equals { field, value } => match document.get(field.as_str()) {
                Ok(actual) if actual == value => {}
                Ok(_)
                | Err(DomainError {
                    code: ErrorCode::DocumentFieldUnassigned,
                    ..
                }) => {
                    return Ok(false);
                }
                Err(error) => return Err(error),
            },
        }
    }
    Ok(true)
}

fn apply_actions(
    actions: &[DocumentAction],
    document: &mut Document,
    is_cancelled: &mut impl FnMut() -> bool,
) -> Result<(), DomainError> {
    for action in actions {
        ensure_not_cancelled(is_cancelled)?;
        match action {
            // 所有命中规则都会在规则动作完整成功后记录一次 ID；RecordMatch 的领域意义是
            // 允许一条规则显式声明“只观察、不修改”，因此这里没有额外 Document 副作用。
            DocumentAction::RecordMatch => {}
            DocumentAction::SetField { field, value } => {
                document.set(field.as_str(), value.clone())?;
            }
            DocumentAction::ClearField { field } => {
                document.clear_field(field.as_str())?;
            }
            DocumentAction::ClearDocument => document.clear(),
        }
    }
    Ok(())
}

fn ensure_not_cancelled(is_cancelled: &mut impl FnMut() -> bool) -> Result<(), DomainError> {
    if is_cancelled() {
        Err(DomainError::new(
            ErrorCode::RuleExecutionCancelled,
            "Socket Document 规则执行已取消",
        ))
    } else {
        Ok(())
    }
}

fn rule_program_error(message: &str) -> DomainError {
    DomainError::new(ErrorCode::RuleInvalid, message)
}

#[cfg(test)]
mod tests;
