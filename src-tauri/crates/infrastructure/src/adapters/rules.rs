//! 规则仓库适配器：校验、版本控制、导入导出与运行时命中数据持久化。
//!
//! 规则集合通过 revision/CAS 更新，避免并发编辑丢失；运行时使用不可变快照执行，持久化
//! 命中计数失败不会反向篡改已经匹配过的网络消息。

use std::{collections::BTreeMap, sync::Arc};

use async_trait::async_trait;
use chrono::Utc;
use intercept_proxy_application::{
    AppError, AppResult, FieldValidationViewModel, MessageStage as AppMessageStage,
    OperationResultViewModel, ProxyWorkspace, RuleAction as AppRuleAction,
    RuleCondition as AppRuleCondition, RuleDraft as AppRuleDraft,
    RuleDropResponseMode as AppRuleDropResponseMode, RuleId as AppRuleId,
    RuleJitterScope as AppRuleJitterScope, RuleMatchField as AppRuleMatchField,
    RuleMatchOperator as AppRuleMatchOperator, RuleRepositoryPort, RuleSummaryViewModel,
    RuleTerminalAction as AppRuleTerminalAction, RuleTrafficDirection as AppRuleTrafficDirection,
    RuleValidationViewModel, RuleViewModel, SessionId, SessionQueryPort, UiTone,
};
use intercept_proxy_domain::{
    ChannelId, DropResponseMode, JitterScope, MatchCondition, MatchField, MatchOperator,
    MessageStage, Revision, Rule, RuleAction, RuleDraft, RuleEngine, RuleId, RuleRuntimeSnapshot,
    RuleSetSignature, RuntimeEpoch, TerminalAction, TrafficDirection, validate_rule_draft,
};
use intercept_proxy_product_api::ProductChannel;
use parking_lot::Mutex;
use serde_json::{Map, Value};

use crate::files::RULE_IMPORT_MAX_BYTES;
use crate::{AtomicFileExporter, InfrastructureError, SqliteStore, WorkspaceRecord};

use super::{
    common::{app_error, decode_workspace_record, infra, json_error},
    files::{NativeFileDialog, cancelled},
};

const PERSISTENCE_VERSION_FIELD: &str = "_persistence_version";
const RULE_PERSISTENCE_VERSION: u64 = 1;

#[derive(Debug)]
pub struct RuleRepositoryAdapter {
    store: Arc<SqliteStore>,
    dialog: Arc<dyn NativeFileDialog>,
    sessions: Arc<dyn SessionQueryPort>,
    exporter: AtomicFileExporter,
    operations: Mutex<()>,
    channel_names: BTreeMap<ChannelId, String>,
    legacy_terminal_body_fields: &'static [&'static str],
}

impl RuleRepositoryAdapter {
    #[must_use]
    pub fn new(
        store: Arc<SqliteStore>,
        dialog: Arc<dyn NativeFileDialog>,
        sessions: Arc<dyn SessionQueryPort>,
        channels: &[ProductChannel],
        legacy_terminal_body_fields: &'static [&'static str],
    ) -> Self {
        Self {
            store,
            dialog,
            sessions,
            exporter: AtomicFileExporter,
            operations: Mutex::new(()),
            channel_names: channels
                .iter()
                .map(|channel| {
                    (
                        ChannelId::new(channel.id)
                            .expect("product channel IDs are compile-time validated"),
                        channel.display_name.to_owned(),
                    )
                })
                .collect(),
            legacy_terminal_body_fields,
        }
    }

    /// 规则属于当前选中的 Workspace 聚合；独立 `rules` 表只保留旧 schema，不再读取。
    fn load_selected_workspace(&self) -> AppResult<ProxyWorkspace> {
        let snapshot = infra(self.store.load_workspaces())?;
        let selected_id = snapshot
            .selected_id
            .ok_or_else(|| AppError::new("WORKSPACE_NOT_FOUND", "当前没有选中的 Workspace。"))?;
        let record = snapshot
            .records
            .into_iter()
            .find(|record| record.id == selected_id)
            .ok_or_else(|| persisted_rule_error("选中的 Workspace 记录不存在。".into()))?;
        decode_workspace_record(record).map_err(persisted_rule_error)
    }

    fn load(&self) -> AppResult<Vec<Rule>> {
        Ok(self.load_selected_workspace()?.rules)
    }

    fn load_workspace_for_channel(&self, channel: &str) -> AppResult<ProxyWorkspace> {
        let snapshot = infra(self.store.load_workspaces())?;
        let mut matches = Vec::new();
        for record in snapshot.records {
            let workspace = decode_workspace_record(record).map_err(persisted_rule_error)?;
            if workspace
                .listeners
                .iter()
                .any(|listener| listener.id.to_string() == channel)
            {
                matches.push(workspace);
            }
        }
        let mut matches = matches.into_iter();
        let workspace = matches.next().ok_or_else(|| {
            AppError::new(
                "WORKSPACE_NOT_FOUND",
                "找不到运行中代理入口所属的 Workspace。",
            )
            .entity(channel.to_owned())
        })?;
        if matches.next().is_some() {
            return Err(persisted_rule_error(format!(
                "代理入口 {channel} 同时属于多个 Workspace。"
            )));
        }
        workspace.validate().map_err(AppError::from)?;
        Ok(workspace)
    }

    fn load_workspace_by_id(&self, id: uuid::Uuid) -> AppResult<ProxyWorkspace> {
        let snapshot = infra(self.store.load_workspaces())?;
        let record = snapshot
            .records
            .into_iter()
            .find(|record| record.id == id)
            .ok_or_else(|| {
                AppError::new("WORKSPACE_NOT_FOUND", "运行规则所属 Workspace 不存在。")
            })?;
        decode_workspace_record(record).map_err(persisted_rule_error)
    }

    fn workspace_record(workspace: &ProxyWorkspace) -> AppResult<WorkspaceRecord> {
        Ok(WorkspaceRecord {
            id: workspace.id.as_uuid(),
            revision: workspace.revision.get(),
            value: serde_json::to_value(workspace)
                .map_err(|error| json_error("Workspace 序列化失败", error))?,
            updated_at: Utc::now(),
        })
    }

    fn save_selected_workspace(
        &self,
        mut workspace: ProxyWorkspace,
        expected_revision: u64,
    ) -> AppResult<ProxyWorkspace> {
        workspace.revision = Revision::new(expected_revision).next();
        workspace.validate().map_err(AppError::from)?;
        infra(
            self.store.compare_and_swap_workspace(
                expected_revision,
                &Self::workspace_record(&workspace)?,
            ),
        )?;
        Ok(workspace)
    }

    fn save_locked(&self, draft: &AppRuleDraft) -> AppResult<Rule> {
        let mut workspace = self.load_selected_workspace()?;
        let mut rules = workspace.rules.clone();
        let creation_order = draft
            .rule_id
            .and_then(|id| {
                rules
                    .iter()
                    .find(|rule| rule.id.as_uuid() == id)
                    .map(|rule| rule.created_order)
            })
            .unwrap_or_else(|| {
                rules
                    .iter()
                    .map(|rule| rule.created_order)
                    .max()
                    .unwrap_or(0)
                    .saturating_add(1)
            });
        let domain_draft = to_domain_draft(draft, creation_order).map_err(AppError::from)?;
        let changed = if let Some(id) = draft.rule_id {
            let domain_id = RuleId::from_uuid(id);
            let mut engine = RuleEngine::new(RuntimeEpoch::new(), rules);
            engine
                .save(domain_id, domain_draft)
                .map_err(AppError::from)?;
            rules = engine.rules().to_vec();
            rules
                .iter()
                .find(|rule| rule.id == domain_id)
                .cloned()
                .expect("domain engine retained saved rule")
        } else {
            Rule::create(domain_draft).map_err(AppError::from)?
        };
        if draft.rule_id.is_none() {
            rules.push(changed.clone());
            rules = RuleEngine::new(RuntimeEpoch::new(), rules).rules().to_vec();
        }
        let expected_workspace_revision = workspace.revision.get();
        workspace.rules = rules;
        self.save_selected_workspace(workspace, expected_workspace_revision)?;
        Ok(changed)
    }

    pub(crate) fn get_domain(&self, id: AppRuleId) -> AppResult<Rule> {
        self.load()?
            .into_iter()
            .find(|rule| rule.id.as_uuid() == id)
            .ok_or_else(|| AppError::new("RULE_INVALID", "规则不存在。").entity(id.to_string()))
    }

    pub(crate) fn toggle_domain(
        &self,
        id: AppRuleId,
        expected_revision: u64,
        enabled: bool,
    ) -> AppResult<Rule> {
        let _operation = self.operations.lock();
        let mut workspace = self.load_selected_workspace()?;
        let mut rules = workspace.rules.clone();
        let domain_id = RuleId::from_uuid(id);
        let mut engine = RuleEngine::new(RuntimeEpoch::new(), rules);
        engine
            .toggle(domain_id, Revision::new(expected_revision), enabled)
            .map_err(AppError::from)?;
        rules = engine.rules().to_vec();
        let changed = rules
            .iter()
            .find(|rule| rule.id == domain_id)
            .cloned()
            .expect("domain engine retained toggled rule");
        let expected_workspace_revision = workspace.revision.get();
        workspace.rules = rules;
        self.save_selected_workspace(workspace, expected_workspace_revision)?;
        Ok(changed)
    }

    pub fn runtime_snapshot(&self, channel: &str) -> AppResult<RuleRuntimeSnapshot> {
        let _operation = self.operations.lock();
        let workspace = self.load_workspace_for_channel(channel)?;
        Ok(RuleRuntimeSnapshot::with_collection_identity(
            Some(workspace.id.as_uuid()),
            workspace.revision.get(),
            workspace.rules,
        ))
    }

    pub fn commit_runtime_snapshot(
        &self,
        snapshot: &RuleRuntimeSnapshot,
        evaluated_rules: &[Rule],
    ) -> AppResult<u64> {
        let _operation = self.operations.lock();
        if RuleSetSignature::from_rules(&snapshot.rules) != snapshot.signature {
            return Err(AppError::new(
                "REVISION_CONFLICT",
                "规则运行快照签名与内容不一致。",
            ));
        }
        let collection_id = snapshot.collection_id.ok_or_else(|| {
            AppError::new("REVISION_CONFLICT", "规则运行快照缺少 Workspace 标识。")
        })?;
        let mut workspace = self.load_workspace_by_id(collection_id)?;
        if snapshot.collection_id != Some(workspace.id.as_uuid())
            || workspace.revision.get() != snapshot.collection_revision
            || RuleSetSignature::from_rules(&workspace.rules) != snapshot.signature
        {
            return Err(AppError::new(
                "REVISION_CONFLICT",
                "Workspace 或规则集合已在运行快照之后发生变化。",
            ));
        }
        workspace.rules = runtime_rules(snapshot, evaluated_rules)?;
        let expected_revision = workspace.revision.get();
        Ok(self
            .save_selected_workspace(workspace, expected_revision)?
            .revision
            .get())
    }

    pub fn reset_runtime_hit_metadata(&self, collection_id: uuid::Uuid) -> AppResult<()> {
        let _operation = self.operations.lock();
        let mut workspace = self.load_workspace_by_id(collection_id)?;
        let expected_revision = workspace.revision.get();
        for rule in &mut workspace.rules {
            rule.hit_count = 0;
            rule.last_hit_at = None;
        }
        self.save_selected_workspace(workspace, expected_revision)
            .map(|_| ())
    }
}

fn persisted_rule_error(message: String) -> AppError {
    app_error(InfrastructureError::PersistenceCorrupt {
        entity: "rule",
        message,
    })
}

fn deserialize_persisted_rule(
    mut value: Value,
    legacy_terminal_body_fields: &[&str],
) -> Result<Rule, String> {
    let object = value
        .as_object_mut()
        .ok_or_else(|| "rule root must be an object".to_owned())?;
    let version = take_rule_persistence_version(object)?;
    let has_legacy_body = has_legacy_terminal_body(object, legacy_terminal_body_fields);

    match (version, has_legacy_body) {
        (Some(RULE_PERSISTENCE_VERSION) | None, false) => {}
        (None, true) => {
            migrate_legacy_terminal_bodies(object, legacy_terminal_body_fields)?;
        }
        (Some(version), _) => {
            return Err(format!("unsupported rule persistence version {version}"));
        }
    }
    serde_json::from_value(value).map_err(|error| error.to_string())
}

fn take_rule_persistence_version(object: &mut Map<String, Value>) -> Result<Option<u64>, String> {
    object
        .remove(PERSISTENCE_VERSION_FIELD)
        .map(|value| {
            value
                .as_u64()
                .ok_or_else(|| "rule persistence version must be an unsigned integer".to_owned())
        })
        .transpose()
}

fn has_legacy_terminal_body(rule: &Map<String, Value>, legacy_fields: &[&str]) -> bool {
    terminal_action_objects(rule).any(|action| {
        legacy_fields
            .iter()
            .any(|field| action.contains_key(*field))
    })
}

fn migrate_legacy_terminal_bodies(
    rule: &mut Map<String, Value>,
    legacy_fields: &[&str],
) -> Result<(), String> {
    for action in terminal_action_objects_mut(rule) {
        let mut legacy_body = None;
        for field in legacy_fields {
            let Some(value) = action.remove(*field) else {
                continue;
            };
            if legacy_body.replace(value).is_some() {
                return Err("terminal action contains multiple legacy body fields".into());
            }
        }
        let Some(legacy_body) = legacy_body else {
            continue;
        };
        if action.insert("body_bytes".into(), legacy_body).is_some() {
            return Err("terminal action contains both legacy and current body fields".into());
        }
    }
    Ok(())
}

fn terminal_action_objects(rule: &Map<String, Value>) -> impl Iterator<Item = &Map<String, Value>> {
    rule.get("actions")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(|action| action.get("Terminal"))
        .filter_map(Value::as_object)
        .filter_map(|terminal| {
            terminal
                .get("MockResponse")
                .or_else(|| terminal.get("InvalidJson"))
        })
        .filter_map(Value::as_object)
}

fn terminal_action_objects_mut(
    rule: &mut Map<String, Value>,
) -> impl Iterator<Item = &mut Map<String, Value>> {
    rule.get_mut("actions")
        .and_then(Value::as_array_mut)
        .into_iter()
        .flatten()
        .filter_map(|action| action.get_mut("Terminal"))
        .filter_map(Value::as_object_mut)
        .filter_map(|terminal| {
            if terminal.contains_key("MockResponse") {
                terminal.get_mut("MockResponse")
            } else {
                terminal.get_mut("InvalidJson")
            }
        })
        .filter_map(Value::as_object_mut)
}

fn validate_persisted_rule(rule: &Rule) -> Result<(), intercept_proxy_domain::DomainError> {
    validate_rule_draft(&RuleDraft {
        expected_revision: Some(rule.revision),
        name: rule.name.clone(),
        description: rule.description.clone(),
        enabled: rule.enabled,
        priority: rule.priority,
        created_order: rule.created_order,
        channel: rule.channel.clone(),
        stage: rule.stage,
        conditions: rule.conditions.clone(),
        actions: rule.actions.clone(),
        one_shot: rule.one_shot,
    })
}

#[async_trait]
impl RuleRepositoryPort for RuleRepositoryAdapter {
    async fn list(&self) -> AppResult<Vec<RuleSummaryViewModel>> {
        self.load().and_then(|rules| {
            rules
                .iter()
                .map(|rule| summary(rule, &self.channel_names))
                .collect()
        })
    }

    async fn get(&self, rule_id: AppRuleId) -> AppResult<RuleViewModel> {
        view(&self.get_domain(rule_id)?, &self.channel_names)
    }

    async fn new_draft(&self) -> AppResult<AppRuleDraft> {
        Ok(AppRuleDraft {
            rule_id: None,
            expected_revision: None,
            name: "新建规则".into(),
            description: String::new(),
            enabled: true,
            priority: 100,
            channel: None,
            stage: Some(AppMessageStage::Request),
            conditions: Vec::new(),
            actions: Vec::new(),
            one_shot: false,
        })
    }

    async fn create_from_session(&self, session_id: SessionId) -> AppResult<AppRuleDraft> {
        let session = self.sessions.get(session_id).await?;
        let condition = MatchCondition::Field {
            field: intercept_proxy_domain::MatchField::PathOrRequestType,
            operator: intercept_proxy_domain::MatchOperator::Equals(session.summary.target.clone()),
        };
        Ok(AppRuleDraft {
            rule_id: None,
            expected_revision: None,
            name: format!("匹配 {}", session.summary.target),
            description: format!(
                "基于请求 {} 创建，请确认动作后保存。",
                session.summary.request_id
            ),
            enabled: true,
            priority: 100,
            channel: Some(session.summary.channel),
            stage: Some(AppMessageStage::Request),
            conditions: vec![condition_to_app(&condition)],
            actions: Vec::new(),
            one_shot: false,
        })
    }

    async fn validate(&self, draft: &AppRuleDraft) -> AppResult<RuleValidationViewModel> {
        let creation_order = self
            .load()?
            .iter()
            .map(|rule| rule.created_order)
            .max()
            .unwrap_or(0)
            .saturating_add(1);
        match to_domain_draft(draft, creation_order).and_then(|draft| {
            validate_rule_draft(&draft)?;
            Ok(draft)
        }) {
            Ok(candidate) => {
                let mut warnings = Vec::new();
                if let Ok(rule) = Rule::create(candidate) {
                    let mut all = self.load()?;
                    all.push(rule);
                    warnings.extend(
                        RuleEngine::new(RuntimeEpoch::new(), all)
                            .conflict_warnings()
                            .into_iter()
                            .map(|warning| warning.message),
                    );
                }
                Ok(FieldValidationViewModel {
                    valid: true,
                    field_errors: BTreeMap::default(),
                    warnings,
                })
            }
            Err(error) => Ok(validation_from_domain(&error)),
        }
    }

    async fn save(&self, draft: AppRuleDraft) -> AppResult<RuleViewModel> {
        let _operation = self.operations.lock();
        view(&self.save_locked(&draft)?, &self.channel_names)
    }

    async fn copy(&self, rule_id: AppRuleId) -> AppResult<RuleViewModel> {
        let _operation = self.operations.lock();
        let source = self
            .load()?
            .into_iter()
            .find(|rule| rule.id.as_uuid() == rule_id)
            .ok_or_else(|| {
                AppError::new("RULE_INVALID", "规则不存在。").entity(rule_id.to_string())
            })?;
        let mut draft = app_draft(&source)?;
        draft.rule_id = None;
        draft.expected_revision = None;
        draft.name = format!("{}（副本）", draft.name);
        view(&self.save_locked(&draft)?, &self.channel_names)
    }

    async fn delete(
        &self,
        rule_id: AppRuleId,
        expected_revision: u64,
    ) -> AppResult<OperationResultViewModel> {
        let _operation = self.operations.lock();
        let mut workspace = self.load_selected_workspace()?;
        let rule = workspace
            .rules
            .iter()
            .find(|rule| rule.id.as_uuid() == rule_id)
            .ok_or_else(|| AppError::new("RULE_INVALID", "规则不存在。"))?;
        if rule.revision.get() != expected_revision {
            return Err(AppError::new("REVISION_CONFLICT", "规则已被其他操作更新。"));
        }
        let expected_workspace_revision = workspace.revision.get();
        workspace.rules.retain(|rule| rule.id.as_uuid() != rule_id);
        self.save_selected_workspace(workspace, expected_workspace_revision)?;
        Ok(OperationResultViewModel::success("规则已删除。"))
    }

    async fn toggle(
        &self,
        rule_id: AppRuleId,
        expected_revision: u64,
        enabled: bool,
    ) -> AppResult<RuleViewModel> {
        view(
            &self.toggle_domain(rule_id, expected_revision, enabled)?,
            &self.channel_names,
        )
    }

    async fn import(&self) -> AppResult<OperationResultViewModel> {
        let selected = self.load_selected_workspace()?;
        let expected_workspace_id = selected.id;
        let expected_workspace_revision = selected.revision.get();
        let Some(path) = self.dialog.choose_open_file("rules_json")? else {
            return Ok(cancelled("已取消规则导入。"));
        };
        let bytes = infra(self.exporter.read_bounded(&path, RULE_IMPORT_MAX_BYTES))?;
        let values: Vec<Value> = serde_json::from_slice(&bytes)
            .map_err(|error| json_error("规则导入文件无效", error))?;
        let rules = values
            .into_iter()
            .map(|value| deserialize_persisted_rule(value, self.legacy_terminal_body_fields))
            .collect::<Result<Vec<_>, _>>()
            .map_err(|error| json_error("规则导入文件无效", error))?;
        for rule in &rules {
            validate_persisted_rule(rule).map_err(AppError::from)?;
        }
        let imported_count = rules.len();
        let _operation = self.operations.lock();
        let mut current = self.load_selected_workspace()?;
        if current.id != expected_workspace_id
            || current.revision.get() != expected_workspace_revision
        {
            return Err(AppError::new(
                "REVISION_CONFLICT",
                "导入期间当前 Workspace 已切换或被更新。",
            ));
        }
        current.rules = RuleEngine::new(RuntimeEpoch::new(), rules).rules().to_vec();
        self.save_selected_workspace(current, expected_workspace_revision)?;
        Ok(OperationResultViewModel::success(format!(
            "已导入 {imported_count} 条规则。"
        )))
    }

    async fn export(&self) -> AppResult<OperationResultViewModel> {
        let Some(selection) = self.dialog.choose_save_file("rules_json")? else {
            return Ok(cancelled("已取消规则导出。"));
        };
        let bytes = serde_json::to_vec_pretty(&self.load()?)
            .map_err(|error| json_error("规则导出序列化失败", error))?;
        infra(
            self.exporter
                .write(&selection.path, &bytes, selection.overwrite_confirmed),
        )?;
        Ok(OperationResultViewModel::success("规则已导出。"))
    }
}

fn runtime_rules(snapshot: &RuleRuntimeSnapshot, evaluated_rules: &[Rule]) -> AppResult<Vec<Rule>> {
    let mut expected_ids = snapshot
        .rules
        .iter()
        .map(|rule| rule.id)
        .collect::<Vec<_>>();
    let mut evaluated_ids = evaluated_rules
        .iter()
        .map(|rule| rule.id)
        .collect::<Vec<_>>();
    expected_ids.sort_unstable();
    evaluated_ids.sort_unstable();
    if evaluated_ids != expected_ids {
        return Err(AppError::new(
            "RULE_INVALID",
            "运行态提交不得增加、删除或重复规则。",
        ));
    }
    snapshot
        .rules
        .iter()
        .map(|original| {
            let evaluated = evaluated_rules
                .iter()
                .find(|rule| rule.id == original.id)
                .ok_or_else(|| AppError::new("RULE_INVALID", "运行态提交缺少规则。"))?;
            let one_shot_fired = original.one_shot
                && original.enabled
                && !evaluated.enabled
                && evaluated.revision == original.revision.next();
            let mut allowed = original.clone();
            allowed.hit_count = evaluated.hit_count;
            allowed.last_hit_at = evaluated.last_hit_at;
            if one_shot_fired {
                allowed.enabled = evaluated.enabled;
                allowed.revision = evaluated.revision;
            }
            if allowed != *evaluated {
                return Err(AppError::new(
                    "RULE_INVALID",
                    "运行态提交包含非命中元数据的配置变更。",
                )
                .entity(original.id.to_string()));
            }
            Ok(evaluated.clone())
        })
        .collect()
}

fn to_domain_draft(
    draft: &AppRuleDraft,
    creation_order: u64,
) -> Result<RuleDraft, intercept_proxy_domain::DomainError> {
    let stage = match draft.stage {
        Some(AppMessageStage::Request) => MessageStage::Request,
        Some(AppMessageStage::Response) => MessageStage::Response,
        Some(AppMessageStage::TlsHandshake) => MessageStage::TlsHandshake,
        _ => {
            return Err(intercept_proxy_domain::DomainError::new(
                intercept_proxy_domain::ErrorCode::RuleInvalid,
                "规则必须指定 TLS 握手、请求或响应阶段",
            )
            .with_field_error("stage", "阶段无效"));
        }
    };
    let priority = u32::try_from(draft.priority).map_err(|_| {
        intercept_proxy_domain::DomainError::new(
            intercept_proxy_domain::ErrorCode::RuleInvalid,
            "规则优先级不能为负数",
        )
        .with_field_error("priority", "必须大于等于 0")
    })?;
    let conditions = draft.conditions.iter().map(condition_to_domain).collect();
    let actions = draft
        .actions
        .iter()
        .enumerate()
        .map(|(index, action)| {
            action_to_domain(action).map_err(|error| {
                let (field, message) = match action {
                    AppRuleAction::SetJsonField { value_json, .. }
                        if value_json.trim().is_empty() =>
                    {
                        (
                            format!("actions.{index}.value_json"),
                            format!(
                                "动作 {} 的 JSON 值不能为空；请输入 null、字符串、数字、对象或数组",
                                index + 1
                            ),
                        )
                    }
                    AppRuleAction::SetJsonField { .. } => (
                        format!("actions.{index}.value_json"),
                        format!(
                            "动作 {} 的 JSON 值格式无效；第 {} 行第 {} 列附近存在错误",
                            index + 1,
                            error.line(),
                            error.column()
                        ),
                    ),
                    _ => (
                        format!("actions.{index}"),
                        format!("动作 {} 的参数格式无效", index + 1),
                    ),
                };
                intercept_proxy_domain::DomainError::new(
                    intercept_proxy_domain::ErrorCode::RuleInvalid,
                    "规则动作无效",
                )
                .with_field_error(field, message)
            })
        })
        .collect::<Result<Vec<_>, _>>()?;
    Ok(RuleDraft {
        expected_revision: draft.expected_revision.map(Revision::new),
        name: draft.name.clone(),
        description: draft.description.clone(),
        enabled: draft.enabled,
        priority,
        created_order: creation_order,
        channel: draft.channel.clone(),
        stage,
        conditions,
        actions,
        one_shot: draft.one_shot,
    })
}

fn app_draft(rule: &Rule) -> AppResult<AppRuleDraft> {
    Ok(AppRuleDraft {
        rule_id: Some(rule.id.as_uuid()),
        expected_revision: Some(rule.revision.get()),
        name: rule.name.clone(),
        description: rule.description.clone(),
        enabled: rule.enabled,
        priority: i32::try_from(rule.priority)
            .map_err(|error| json_error("规则优先级超出 UI 范围", error))?,
        channel: rule.channel.clone(),
        stage: Some(match rule.stage {
            MessageStage::Request => AppMessageStage::Request,
            MessageStage::Response => AppMessageStage::Response,
            MessageStage::TlsHandshake => AppMessageStage::TlsHandshake,
        }),
        conditions: rule.conditions.iter().map(condition_to_app).collect(),
        actions: rule
            .actions
            .iter()
            .map(action_to_app)
            .collect::<Result<_, _>>()
            .map_err(|error| json_error("规则动作转换失败", error))?,
        one_shot: rule.one_shot,
    })
}

pub(crate) fn condition_to_domain(condition: &AppRuleCondition) -> MatchCondition {
    match condition {
        AppRuleCondition::Field { field, operator } => MatchCondition::Field {
            field: match field {
                AppRuleMatchField::TerminalIp => MatchField::TerminalIp,
                AppRuleMatchField::CertificateFingerprint => MatchField::CertificateFingerprint,
                AppRuleMatchField::PathOrRequestType => MatchField::PathOrRequestType,
                AppRuleMatchField::JsonPath { path } => MatchField::JsonPath(path.clone()),
            },
            operator: match operator {
                AppRuleMatchOperator::Equals { value } => MatchOperator::Equals(value.clone()),
                AppRuleMatchOperator::Contains { value } => MatchOperator::Contains(value.clone()),
                AppRuleMatchOperator::Regex { pattern } => MatchOperator::Regex(pattern.clone()),
            },
        },
        AppRuleCondition::NthHit { count } => MatchCondition::NthHit(*count),
    }
}

pub(crate) fn condition_to_app(condition: &MatchCondition) -> AppRuleCondition {
    match condition {
        MatchCondition::Field { field, operator } => AppRuleCondition::Field {
            field: match field {
                MatchField::TerminalIp => AppRuleMatchField::TerminalIp,
                MatchField::CertificateFingerprint => AppRuleMatchField::CertificateFingerprint,
                MatchField::PathOrRequestType => AppRuleMatchField::PathOrRequestType,
                MatchField::JsonPath(path) => AppRuleMatchField::JsonPath { path: path.clone() },
            },
            operator: match operator {
                MatchOperator::Equals(value) => AppRuleMatchOperator::Equals {
                    value: value.clone(),
                },
                MatchOperator::Contains(value) => AppRuleMatchOperator::Contains {
                    value: value.clone(),
                },
                MatchOperator::Regex(pattern) => AppRuleMatchOperator::Regex {
                    pattern: pattern.clone(),
                },
            },
        },
        MatchCondition::NthHit(count) => AppRuleCondition::NthHit { count: *count },
    }
}

pub(crate) fn action_to_domain(action: &AppRuleAction) -> Result<RuleAction, serde_json::Error> {
    Ok(match action {
        AppRuleAction::SetJsonField { path, value_json } => RuleAction::SetJsonField {
            path: path.clone(),
            value: serde_json::from_str(value_json)?,
        },
        AppRuleAction::ReplaceBodyText { text } => RuleAction::ReplaceBodyText(text.clone()),
        AppRuleAction::SetHeader { name, value } => RuleAction::SetHeader {
            name: name.clone(),
            value: value.clone(),
        },
        AppRuleAction::Delay { milliseconds } => RuleAction::Delay {
            milliseconds: *milliseconds,
        },
        AppRuleAction::Jitter {
            minimum_milliseconds,
            maximum_milliseconds,
            scope,
        } => RuleAction::Jitter {
            minimum_milliseconds: *minimum_milliseconds,
            maximum_milliseconds: *maximum_milliseconds,
            scope: match scope {
                AppRuleJitterScope::BeforeMessage => JitterScope::BeforeMessage,
                AppRuleJitterScope::PerChunk => JitterScope::PerChunk,
            },
        },
        AppRuleAction::Throttle {
            bytes_per_second,
            chunk_bytes,
            direction,
        } => RuleAction::Throttle {
            bytes_per_second: *bytes_per_second,
            chunk_bytes: *chunk_bytes,
            direction: traffic_direction_to_domain(*direction),
        },
        AppRuleAction::Intermittent {
            available_milliseconds,
            blocked_milliseconds,
            direction,
        } => RuleAction::Intermittent {
            available_milliseconds: *available_milliseconds,
            blocked_milliseconds: *blocked_milliseconds,
            direction: traffic_direction_to_domain(*direction),
        },
        AppRuleAction::Pause => RuleAction::Pause,
        AppRuleAction::CustomHttpStatus { status } => {
            RuleAction::CustomHttpStatus { status: *status }
        }
        AppRuleAction::Terminal { action } => {
            RuleAction::Terminal(terminal_action_to_domain(action))
        }
    })
}

fn terminal_action_to_domain(action: &AppRuleTerminalAction) -> TerminalAction {
    match action {
        AppRuleTerminalAction::RejectTlsHandshake => TerminalAction::RejectTlsHandshake,
        AppRuleTerminalAction::DisconnectBeforeUpstream => TerminalAction::DisconnectBeforeUpstream,
        AppRuleTerminalAction::UpstreamConnectTimeout { milliseconds } => {
            TerminalAction::UpstreamConnectTimeout {
                milliseconds: *milliseconds,
            }
        }
        AppRuleTerminalAction::UpstreamWriteTimeout { milliseconds } => {
            TerminalAction::UpstreamWriteTimeout {
                milliseconds: *milliseconds,
            }
        }
        AppRuleTerminalAction::UpstreamReadTimeout { milliseconds } => {
            TerminalAction::UpstreamReadTimeout {
                milliseconds: *milliseconds,
            }
        }
        AppRuleTerminalAction::DropUpstreamResponse { mode } => {
            TerminalAction::DropUpstreamResponse {
                mode: match mode {
                    AppRuleDropResponseMode::ReadCompleteResponse => {
                        DropResponseMode::ReadCompleteResponse
                    }
                    AppRuleDropResponseMode::CloseAfterRequestWrite => {
                        DropResponseMode::CloseAfterRequestWrite
                    }
                },
            }
        }
        AppRuleTerminalAction::MockResponse {
            status,
            headers,
            body_bytes,
        } => TerminalAction::MockResponse {
            status: *status,
            headers: headers.clone(),
            body_bytes: body_bytes.clone(),
        },
        AppRuleTerminalAction::InvalidJson { body_bytes } => TerminalAction::InvalidJson {
            body_bytes: body_bytes.clone(),
        },
        AppRuleTerminalAction::IncorrectContentLength { delta } => {
            TerminalAction::IncorrectContentLength { delta: *delta }
        }
        AppRuleTerminalAction::TruncateResponse { bytes } => {
            TerminalAction::TruncateResponse { bytes: *bytes }
        }
        AppRuleTerminalAction::DisconnectDuringUpstreamWrite { after_bytes } => {
            TerminalAction::DisconnectDuringUpstreamWrite {
                after_bytes: *after_bytes,
            }
        }
        AppRuleTerminalAction::DisconnectDuringDownstreamWrite { after_bytes } => {
            TerminalAction::DisconnectDuringDownstreamWrite {
                after_bytes: *after_bytes,
            }
        }
    }
}

pub(crate) fn action_to_app(action: &RuleAction) -> Result<AppRuleAction, serde_json::Error> {
    Ok(match action {
        RuleAction::SetJsonField { path, value } => AppRuleAction::SetJsonField {
            path: path.clone(),
            value_json: serde_json::to_string(value)?,
        },
        RuleAction::ReplaceBodyText(text) => AppRuleAction::ReplaceBodyText { text: text.clone() },
        RuleAction::SetHeader { name, value } => AppRuleAction::SetHeader {
            name: name.clone(),
            value: value.clone(),
        },
        RuleAction::Delay { milliseconds } => AppRuleAction::Delay {
            milliseconds: *milliseconds,
        },
        RuleAction::Jitter {
            minimum_milliseconds,
            maximum_milliseconds,
            scope,
        } => AppRuleAction::Jitter {
            minimum_milliseconds: *minimum_milliseconds,
            maximum_milliseconds: *maximum_milliseconds,
            scope: match scope {
                JitterScope::BeforeMessage => AppRuleJitterScope::BeforeMessage,
                JitterScope::PerChunk => AppRuleJitterScope::PerChunk,
            },
        },
        RuleAction::Throttle {
            bytes_per_second,
            chunk_bytes,
            direction,
        } => AppRuleAction::Throttle {
            bytes_per_second: *bytes_per_second,
            chunk_bytes: *chunk_bytes,
            direction: traffic_direction_to_app(*direction),
        },
        RuleAction::Intermittent {
            available_milliseconds,
            blocked_milliseconds,
            direction,
        } => AppRuleAction::Intermittent {
            available_milliseconds: *available_milliseconds,
            blocked_milliseconds: *blocked_milliseconds,
            direction: traffic_direction_to_app(*direction),
        },
        RuleAction::Pause => AppRuleAction::Pause,
        RuleAction::CustomHttpStatus { status } => {
            AppRuleAction::CustomHttpStatus { status: *status }
        }
        RuleAction::Terminal(action) => AppRuleAction::Terminal {
            action: terminal_action_to_app(action),
        },
    })
}

fn terminal_action_to_app(action: &TerminalAction) -> AppRuleTerminalAction {
    match action {
        TerminalAction::RejectTlsHandshake => AppRuleTerminalAction::RejectTlsHandshake,
        TerminalAction::DisconnectBeforeUpstream => AppRuleTerminalAction::DisconnectBeforeUpstream,
        TerminalAction::UpstreamConnectTimeout { milliseconds } => {
            AppRuleTerminalAction::UpstreamConnectTimeout {
                milliseconds: *milliseconds,
            }
        }
        TerminalAction::UpstreamWriteTimeout { milliseconds } => {
            AppRuleTerminalAction::UpstreamWriteTimeout {
                milliseconds: *milliseconds,
            }
        }
        TerminalAction::UpstreamReadTimeout { milliseconds } => {
            AppRuleTerminalAction::UpstreamReadTimeout {
                milliseconds: *milliseconds,
            }
        }
        TerminalAction::DropUpstreamResponse { mode } => {
            AppRuleTerminalAction::DropUpstreamResponse {
                mode: match mode {
                    DropResponseMode::ReadCompleteResponse => {
                        AppRuleDropResponseMode::ReadCompleteResponse
                    }
                    DropResponseMode::CloseAfterRequestWrite => {
                        AppRuleDropResponseMode::CloseAfterRequestWrite
                    }
                },
            }
        }
        TerminalAction::MockResponse {
            status,
            headers,
            body_bytes,
        } => AppRuleTerminalAction::MockResponse {
            status: *status,
            headers: headers.clone(),
            body_bytes: body_bytes.clone(),
        },
        TerminalAction::InvalidJson { body_bytes } => AppRuleTerminalAction::InvalidJson {
            body_bytes: body_bytes.clone(),
        },
        TerminalAction::IncorrectContentLength { delta } => {
            AppRuleTerminalAction::IncorrectContentLength { delta: *delta }
        }
        TerminalAction::TruncateResponse { bytes } => {
            AppRuleTerminalAction::TruncateResponse { bytes: *bytes }
        }
        TerminalAction::DisconnectDuringUpstreamWrite { after_bytes } => {
            AppRuleTerminalAction::DisconnectDuringUpstreamWrite {
                after_bytes: *after_bytes,
            }
        }
        TerminalAction::DisconnectDuringDownstreamWrite { after_bytes } => {
            AppRuleTerminalAction::DisconnectDuringDownstreamWrite {
                after_bytes: *after_bytes,
            }
        }
    }
}

const fn traffic_direction_to_domain(direction: AppRuleTrafficDirection) -> TrafficDirection {
    match direction {
        AppRuleTrafficDirection::Upstream => TrafficDirection::Upstream,
        AppRuleTrafficDirection::Downstream => TrafficDirection::Downstream,
    }
}

const fn traffic_direction_to_app(direction: TrafficDirection) -> AppRuleTrafficDirection {
    match direction {
        TrafficDirection::Upstream => AppRuleTrafficDirection::Upstream,
        TrafficDirection::Downstream => AppRuleTrafficDirection::Downstream,
    }
}

fn summary(
    rule: &Rule,
    channel_names: &BTreeMap<ChannelId, String>,
) -> AppResult<RuleSummaryViewModel> {
    Ok(RuleSummaryViewModel {
        rule_id: rule.id.as_uuid(),
        revision: rule.revision.get(),
        name: rule.name.clone(),
        enabled: rule.enabled,
        priority: i32::try_from(rule.priority)
            .map_err(|error| json_error("规则优先级超出 UI 范围", error))?,
        creation_order: rule.created_order,
        channel_text: rule.channel.as_ref().map_or_else(
            || "全部".into(),
            |channel| {
                channel_names
                    .get(channel)
                    .cloned()
                    .unwrap_or_else(|| channel.to_string())
            },
        ),
        stage_text: match rule.stage {
            MessageStage::Request => "请求".into(),
            MessageStage::Response => "响应".into(),
            MessageStage::TlsHandshake => "TLS 握手".into(),
        },
        match_summary: format!("{} 个条件", rule.conditions.len()),
        action_summary: format!("{} 个动作", rule.actions.len()),
        hit_count: rule.hit_count,
        last_hit_at: rule.last_hit_at,
        ui_tone: if rule.enabled {
            UiTone::Positive
        } else {
            UiTone::Neutral
        },
    })
}

fn view(rule: &Rule, channel_names: &BTreeMap<ChannelId, String>) -> AppResult<RuleViewModel> {
    Ok(RuleViewModel {
        summary: summary(rule, channel_names)?,
        draft: app_draft(rule)?,
    })
}

fn validation_from_domain(error: &intercept_proxy_domain::DomainError) -> RuleValidationViewModel {
    FieldValidationViewModel {
        valid: false,
        field_errors: error
            .field_errors
            .iter()
            .map(|(field, messages)| (field.clone(), messages.clone()))
            .collect(),
        warnings: Vec::new(),
    }
}

#[cfg(test)]
mod tests {
    use std::{path::PathBuf, sync::Arc};

    use super::*;
    use crate::adapters::{FileSelection, NativeFileDialog};
    use chrono::TimeZone;
    use intercept_proxy_domain::{MatchContext, TerminalIdentity};

    #[derive(Debug)]
    struct NoDialog;

    impl NativeFileDialog for NoDialog {
        fn choose_open_file(&self, _: &str) -> AppResult<Option<PathBuf>> {
            Ok(None)
        }

        fn choose_save_file(&self, _: &str) -> AppResult<Option<FileSelection>> {
            Ok(None)
        }
    }

    #[derive(Debug)]
    struct StaticOpenDialog {
        path: PathBuf,
    }

    impl NativeFileDialog for StaticOpenDialog {
        fn choose_open_file(&self, _: &str) -> AppResult<Option<PathBuf>> {
            Ok(Some(self.path.clone()))
        }

        fn choose_save_file(&self, _: &str) -> AppResult<Option<FileSelection>> {
            Ok(None)
        }
    }

    #[derive(Debug)]
    struct MutatingOpenDialog {
        path: PathBuf,
        store: Arc<SqliteStore>,
        concurrent_rule: Rule,
    }

    impl NativeFileDialog for MutatingOpenDialog {
        fn choose_open_file(&self, _: &str) -> AppResult<Option<PathBuf>> {
            let snapshot = infra(self.store.load_workspaces())?;
            let selected_id = snapshot.selected_id.expect("selected workspace");
            let record = snapshot
                .records
                .into_iter()
                .find(|record| record.id == selected_id)
                .expect("selected record");
            let mut workspace: ProxyWorkspace =
                serde_json::from_value(record.value).expect("workspace");
            workspace.rules.push(self.concurrent_rule.clone());
            workspace.revision = workspace.revision.next();
            infra(self.store.compare_and_swap_selected_workspace(
                selected_id,
                record.revision,
                &RuleRepositoryAdapter::workspace_record(&workspace)?,
            ))?;
            Ok(Some(self.path.clone()))
        }

        fn choose_save_file(&self, _: &str) -> AppResult<Option<FileSelection>> {
            Ok(None)
        }
    }

    fn request_delay_draft(name: &str, one_shot: bool) -> AppRuleDraft {
        AppRuleDraft {
            rule_id: None,
            expected_revision: None,
            name: name.into(),
            description: String::new(),
            enabled: true,
            priority: 10,
            channel: None,
            stage: Some(AppMessageStage::Request),
            conditions: Vec::new(),
            actions: vec![AppRuleAction::Delay { milliseconds: 10 }],
            one_shot,
        }
    }

    fn seed_workspace(store: &Arc<SqliteStore>, rules: Vec<Rule>) -> ProxyWorkspace {
        let workspace = ProxyWorkspace {
            rules,
            ..ProxyWorkspace::default()
        };
        store
            .insert_workspace(&RuleRepositoryAdapter::workspace_record(&workspace).expect("record"))
            .expect("seed workspace");
        workspace
    }

    fn adapter_with(
        store: Arc<SqliteStore>,
        dialog: Arc<dyn NativeFileDialog>,
    ) -> Arc<RuleRepositoryAdapter> {
        if store
            .load_workspaces()
            .expect("workspaces")
            .records
            .is_empty()
        {
            seed_workspace(&store, Vec::new());
        }
        Arc::new(RuleRepositoryAdapter::new(
            store,
            dialog,
            Arc::new(intercept_proxy_application::InMemorySessionStore::default()),
            &[],
            &[],
        ))
    }

    fn adapter() -> Arc<RuleRepositoryAdapter> {
        adapter_with(
            Arc::new(SqliteStore::in_memory().expect("store")),
            Arc::new(NoDialog),
        )
    }

    fn runtime_snapshot(adapter: &RuleRepositoryAdapter) -> RuleRuntimeSnapshot {
        let workspace = adapter
            .load_selected_workspace()
            .expect("selected workspace");
        let channel = workspace.listeners[0].id().to_string();
        adapter
            .runtime_snapshot(&channel)
            .expect("runtime snapshot")
    }

    fn legacy_rule_json(rule_id: uuid::Uuid, terminal_action: &Value) -> Value {
        serde_json::json!({
            "id": rule_id,
            "revision": 3,
            "name": "legacy Shift-JIS rule",
            "description": "persisted by the pre-generic Payment proxy",
            "enabled": true,
            "priority": 10,
            "created_order": 1,
            "channel": null,
            "stage": "Request",
            "conditions": [],
            "actions": [{"Terminal": terminal_action}],
            "one_shot": false,
            "hit_count": 0,
            "last_hit_at": null
        })
    }

    #[test]
    fn typed_ipc_conditions_and_actions_round_trip_without_changing_domain_values() {
        let conditions = vec![
            MatchCondition::Field {
                field: MatchField::JsonPath("$.amount".into()),
                operator: MatchOperator::Regex(r"^\d+$".into()),
            },
            MatchCondition::NthHit(3),
        ];
        let actions = vec![
            RuleAction::SetJsonField {
                path: "$.approved".into(),
                value: serde_json::json!({"ok": true, "code": 0}),
            },
            RuleAction::ReplaceBodyText("本文".into()),
            RuleAction::SetHeader {
                name: "x-test".into(),
                value: "yes".into(),
            },
            RuleAction::Delay { milliseconds: 25 },
            RuleAction::Pause,
            RuleAction::CustomHttpStatus { status: 503 },
            RuleAction::Terminal(TerminalAction::MockResponse {
                status: 200,
                headers: vec![("content-type".into(), "application/json".into())],
                body_bytes: vec![0x82, 0xa0],
            }),
        ];

        assert_eq!(
            conditions,
            conditions
                .iter()
                .map(condition_to_app)
                .map(|condition| condition_to_domain(&condition))
                .collect::<Vec<_>>()
        );
        assert_eq!(
            actions,
            actions
                .iter()
                .map(action_to_app)
                .collect::<Result<Vec<_>, _>>()
                .expect("app actions")
                .iter()
                .map(action_to_domain)
                .collect::<Result<Vec<_>, _>>()
                .expect("domain actions")
        );
    }

    #[tokio::test]
    async fn new_rule_defaults_are_owned_by_the_rust_repository() {
        let draft = adapter().new_draft().await.expect("new draft");
        assert_eq!(draft.name, "新建规则");
        assert_eq!(draft.priority, 100);
        assert_eq!(draft.stage, Some(AppMessageStage::Request));
        assert!(draft.rule_id.is_none());
        assert!(draft.expected_revision.is_none());
        assert!(draft.conditions.is_empty());
        assert!(draft.actions.is_empty());
    }

    #[tokio::test]
    async fn json_action_validation_returns_a_chinese_field_error() {
        let adapter = adapter();
        let mut empty = request_delay_draft("设置 JSON 字段", false);
        empty.actions = vec![AppRuleAction::SetJsonField {
            path: "$.result".into(),
            value_json: String::new(),
        }];

        let empty_validation = adapter.validate(&empty).await.expect("validate empty JSON");
        assert!(!empty_validation.valid);
        assert_eq!(
            empty_validation.field_errors["actions.0.value_json"],
            vec!["动作 1 的 JSON 值不能为空；请输入 null、字符串、数字、对象或数组"]
        );

        empty.actions = vec![AppRuleAction::SetJsonField {
            path: "$.result".into(),
            value_json: "{".into(),
        }];
        let malformed_validation = adapter
            .validate(&empty)
            .await
            .expect("validate malformed JSON");
        let message = &malformed_validation.field_errors["actions.0.value_json"][0];
        assert!(message.starts_with("动作 1 的 JSON 值格式无效；"));
        assert!(!message.contains("expected value"));
    }

    // RULE-003, RULE-011, ENGINE-008, TEST-RULE
    #[tokio::test]
    async fn domain_validation_and_sqlite_revision_are_enforced() {
        let adapter = adapter();
        let created = adapter
            .save(request_delay_draft("延迟", false))
            .await
            .expect("create");
        assert_eq!(created.summary.revision, 1);
        assert_eq!(adapter.list().await.expect("list").len(), 1);

        let mut stale = created.draft;
        stale.expected_revision = Some(0);
        assert_eq!(
            adapter
                .save(stale)
                .await
                .expect_err("stale")
                .view_model
                .code,
            "REVISION_CONFLICT"
        );
    }

    #[tokio::test]
    async fn multiple_new_rules_and_toggle_are_persisted_independently() {
        let adapter = adapter();
        let first = adapter
            .save(request_delay_draft("规则一", false))
            .await
            .expect("create first rule");
        let second = adapter
            .save(request_delay_draft("规则二", true))
            .await
            .expect("create second rule");

        let listed = adapter.list().await.expect("list both rules");
        assert_eq!(listed.len(), 2);
        assert!(listed.iter().any(|rule| rule.name == "规则一"));
        assert!(listed.iter().any(|rule| rule.name == "规则二"));

        adapter
            .toggle(first.summary.rule_id, first.summary.revision, false)
            .await
            .expect("disable first rule");

        let listed = adapter.list().await.expect("list after toggle");
        assert_eq!(listed.len(), 2);
        assert!(
            !listed
                .iter()
                .find(|rule| rule.rule_id == first.summary.rule_id)
                .expect("first rule remains")
                .enabled
        );
        assert!(
            listed
                .iter()
                .find(|rule| rule.rule_id == second.summary.rule_id)
                .expect("second rule remains")
                .enabled
        );
    }

    #[tokio::test]
    async fn selected_workspace_owns_rule_list_runtime_snapshot_and_revision() {
        let store = Arc::new(SqliteStore::in_memory().expect("store"));
        let first_workspace = seed_workspace(&store, Vec::new());
        let adapter = adapter_with(Arc::clone(&store), Arc::new(NoDialog));
        adapter
            .save(request_delay_draft("first workspace rule", false))
            .await
            .expect("save first rule");
        let first_after_save = store
            .load_workspaces()
            .expect("workspaces")
            .records
            .into_iter()
            .find(|record| record.id == first_workspace.id.as_uuid())
            .expect("first workspace");
        assert_eq!(
            first_after_save.revision,
            first_workspace.revision.get() + 1
        );

        let second_rule = Rule::create(
            to_domain_draft(&request_delay_draft("second workspace rule", false), 1)
                .expect("second draft"),
        )
        .expect("second rule");
        let second_workspace = seed_workspace(&store, vec![second_rule.clone()]);
        store
            .select_workspace(second_workspace.id.as_uuid())
            .expect("select second workspace");

        let listed = adapter.list().await.expect("selected rules");
        assert_eq!(listed.len(), 1);
        assert_eq!(listed[0].name, "second workspace rule");
        let runtime = runtime_snapshot(&adapter);
        assert_eq!(runtime.collection_revision, second_workspace.revision.get());
        assert_eq!(runtime.rules, vec![second_rule]);

        store
            .select_workspace(first_workspace.id.as_uuid())
            .expect("reselect first workspace");
        assert_eq!(
            adapter.list().await.expect("first rules")[0].name,
            "first workspace rule"
        );
    }

    #[tokio::test]
    async fn runtime_snapshot_commit_stays_bound_to_owning_workspace_after_ui_switch() {
        let store = Arc::new(SqliteStore::in_memory().expect("store"));
        let first = seed_workspace(&store, Vec::new());
        let adapter = adapter_with(Arc::clone(&store), Arc::new(NoDialog));
        adapter
            .save(request_delay_draft("shared rule", false))
            .await
            .expect("save rule");
        let stale = runtime_snapshot(&adapter);

        let second = ProxyWorkspace {
            revision: Revision::new(stale.collection_revision),
            rules: stale.rules.clone(),
            ..ProxyWorkspace::default()
        };
        store
            .insert_workspace(&RuleRepositoryAdapter::workspace_record(&second).expect("record"))
            .expect("insert identical workspace");
        store
            .select_workspace(second.id.as_uuid())
            .expect("switch workspace");

        let revision = adapter
            .commit_runtime_snapshot(&stale, &stale.rules)
            .expect("runtime commit remains on first workspace");
        assert_eq!(revision, stale.collection_revision + 1);
        let snapshot = store.load_workspaces().expect("workspaces");
        assert_eq!(snapshot.selected_id, Some(second.id.as_uuid()));
        assert_eq!(
            snapshot
                .records
                .iter()
                .find(|record| record.id == first.id.as_uuid())
                .expect("first workspace")
                .revision,
            revision
        );
    }

    #[tokio::test]
    async fn concurrent_same_revision_save_has_exactly_one_winner() {
        let adapter = adapter();
        let created = adapter
            .save(request_delay_draft("原始", false))
            .await
            .expect("create");
        let mut first = created.draft.clone();
        first.name = "first".into();
        let mut second = created.draft;
        second.name = "second".into();

        let (first, second) = tokio::join!(adapter.save(first), adapter.save(second));
        assert_ne!(first.is_ok(), second.is_ok());
        let conflict = first.err().or_else(|| second.err()).expect("one conflict");
        assert_eq!(conflict.view_model.code, "REVISION_CONFLICT");
        let stored = adapter
            .get(created.summary.rule_id)
            .await
            .expect("stored winner");
        assert_eq!(stored.summary.revision, 2);
    }

    #[tokio::test]
    async fn import_rejects_cross_process_changes_instead_of_replacing_them() {
        let directory = tempfile::tempdir().expect("temp directory");
        let database = directory.path().join("rules.sqlite3");
        let import = directory.path().join("rules.json");
        std::fs::write(&import, b"[]").expect("import file");
        let primary_store = Arc::new(SqliteStore::open(&database).expect("primary store"));
        let secondary_store = Arc::new(SqliteStore::open(&database).expect("secondary store"));

        let existing = Rule::create(
            to_domain_draft(&request_delay_draft("existing", false), 1).expect("existing draft"),
        )
        .expect("existing rule");
        seed_workspace(&primary_store, vec![existing.clone()]);
        let concurrent = Rule::create(
            to_domain_draft(&request_delay_draft("concurrent", false), 2)
                .expect("concurrent draft"),
        )
        .expect("concurrent rule");
        let adapter = RuleRepositoryAdapter::new(
            Arc::clone(&primary_store),
            Arc::new(MutatingOpenDialog {
                path: import,
                store: secondary_store,
                concurrent_rule: concurrent.clone(),
            }),
            Arc::new(intercept_proxy_application::InMemorySessionStore::default()),
            &[],
            &[],
        );

        let error = adapter.import().await.expect_err("stale import");
        assert_eq!(error.view_model.code, "REVISION_CONFLICT");
        let stored = RuleRepositoryAdapter::new(
            primary_store,
            Arc::new(NoDialog),
            Arc::new(intercept_proxy_application::InMemorySessionStore::default()),
            &[],
            &[],
        )
        .load()
        .expect("stored rules");
        assert_eq!(stored.len(), 2);
        assert!(stored.iter().any(|rule| rule.id == existing.id));
        assert!(stored.iter().any(|rule| rule.id == concurrent.id));
    }

    #[tokio::test]
    async fn rule_import_rejects_files_over_the_rule_specific_limit() {
        let directory = tempfile::tempdir().expect("temp directory");
        let import = directory.path().join("oversized-rules.json");
        std::fs::File::create(&import)
            .expect("create import")
            .set_len(RULE_IMPORT_MAX_BYTES + 1)
            .expect("size import");
        let adapter = adapter_with(
            Arc::new(SqliteStore::in_memory().expect("store")),
            Arc::new(StaticOpenDialog { path: import }),
        );

        let error = adapter.import().await.expect_err("oversized import");
        assert_eq!(error.view_model.code, "IMPORT_TOO_LARGE");
    }

    #[tokio::test]
    async fn malformed_persisted_rule_maps_to_persistence_corrupt() {
        let store = Arc::new(SqliteStore::in_memory().expect("store"));
        let workspace = ProxyWorkspace::default();
        let mut value = serde_json::to_value(&workspace).expect("workspace value");
        value["rules"] = serde_json::json!([{"not": "a rule"}]);
        store
            .insert_workspace(&WorkspaceRecord {
                id: workspace.id.as_uuid(),
                revision: workspace.revision.get(),
                value,
                updated_at: Utc::now(),
            })
            .expect("seed malformed workspace");
        let adapter = RuleRepositoryAdapter::new(
            store,
            Arc::new(NoDialog),
            Arc::new(intercept_proxy_application::InMemorySessionStore::default()),
            &[],
            &[],
        );

        let error = adapter.list().await.expect_err("corrupt rule");
        assert_eq!(error.view_model.code, "PERSISTENCE_CORRUPT");
        assert_ne!(error.view_model.code, "CERTIFICATE_INVALID");
    }

    #[tokio::test]
    async fn legacy_global_rule_table_is_not_a_runtime_or_crud_source() {
        let store = Arc::new(SqliteStore::in_memory().expect("store"));
        seed_workspace(&store, Vec::new());
        let legacy = Rule::create(
            to_domain_draft(&request_delay_draft("legacy global row", false), 1)
                .expect("legacy draft"),
        )
        .expect("legacy rule");
        let mut value = serde_json::to_value(&legacy).expect("legacy value");
        value.as_object_mut().expect("rule object").insert(
            PERSISTENCE_VERSION_FIELD.into(),
            Value::from(RULE_PERSISTENCE_VERSION),
        );
        store
            .insert_rule(&crate::RuleRecord {
                id: legacy.id.as_uuid(),
                revision: legacy.revision.get(),
                enabled: legacy.enabled,
                value,
                updated_at: Utc::now(),
            })
            .expect("seed legacy global row");

        let adapter = adapter_with(store, Arc::new(NoDialog));
        assert!(adapter.list().await.expect("workspace rules").is_empty());
        assert!(runtime_snapshot(&adapter).rules.is_empty());
    }

    #[tokio::test]
    async fn legacy_rule_json_import_migrates_shift_jis_body() {
        let directory = tempfile::tempdir().expect("temp directory");
        let import = directory.path().join("legacy-rules.json");
        let id = uuid::Uuid::new_v4();
        let legacy = legacy_rule_json(
            id,
            &serde_json::json!({
                "MockResponse": {
                    "status": 200,
                    "headers": [],
                    "shift_jis_body": [123, 125]
                }
            }),
        );
        std::fs::write(
            &import,
            serde_json::to_vec_pretty(&vec![legacy]).expect("legacy JSON"),
        )
        .expect("write legacy JSON");
        let store = Arc::new(SqliteStore::in_memory().expect("store"));
        seed_workspace(&store, Vec::new());
        let adapter = RuleRepositoryAdapter::new(
            store,
            Arc::new(StaticOpenDialog { path: import }),
            Arc::new(intercept_proxy_application::InMemorySessionStore::default()),
            &[],
            &["shift_jis_body"],
        );

        let result = adapter.import().await.expect("import legacy JSON");
        assert!(result.success);
        let loaded = adapter.get(id).await.expect("imported legacy rule");
        assert!(matches!(
            loaded.draft.actions.as_slice(),
            [AppRuleAction::Terminal {
                action: AppRuleTerminalAction::MockResponse { body_bytes, .. }
            }] if body_bytes == b"{}"
        ));
    }

    #[test]
    fn legacy_invalid_json_terminal_body_has_an_explicit_v0_compatibility_path() {
        let id = uuid::Uuid::new_v4();
        let rule = deserialize_persisted_rule(
            legacy_rule_json(
                id,
                &serde_json::json!({"InvalidJson": {"shift_jis_body": [123]}}),
            ),
            &["shift_jis_body"],
        )
        .expect("migrate legacy InvalidJson body");
        assert!(matches!(
            rule.actions.as_slice(),
            [RuleAction::Terminal(TerminalAction::InvalidJson { body_bytes })]
                if body_bytes == b"{"
        ));
    }

    #[tokio::test]
    async fn runtime_commit_is_full_signature_cas_and_reset_preserves_enabled() {
        let adapter = adapter();
        let created = adapter
            .save(request_delay_draft("one-shot", true))
            .await
            .expect("create");
        let snapshot = runtime_snapshot(&adapter);
        let epoch = RuntimeEpoch::new();
        let terminal = TerminalIdentity {
            source_ip: "127.0.0.1".into(),
            certificate_sha256: String::new(),
        };
        let mut engine = RuleEngine::new(epoch, snapshot.rules.clone());
        engine.evaluate(
            &MatchContext {
                runtime_epoch: epoch,
                channel: ChannelId::new("alpha").unwrap(),
                stage: MessageStage::Request,
                terminal: &terminal,
                path_or_request_type: None,
                json_body: None,
            },
            Utc.timestamp_opt(1_700_000_000, 0).unwrap(),
        );
        adapter
            .commit_runtime_snapshot(&snapshot, engine.rules())
            .expect("runtime commit");

        let fired = adapter
            .get_domain(created.summary.rule_id)
            .expect("fired rule");
        assert!(!fired.enabled);
        assert_eq!(fired.revision, Revision::new(2));
        assert_eq!(fired.hit_count, 1);
        assert!(fired.last_hit_at.is_some());

        adapter
            .reset_runtime_hit_metadata(snapshot.collection_id.expect("workspace id"))
            .expect("explicit reset");
        let reset = adapter
            .get_domain(created.summary.rule_id)
            .expect("reset rule");
        assert!(!reset.enabled);
        assert_eq!(reset.revision, Revision::new(2));
        assert_eq!(reset.hit_count, 0);
        assert_eq!(reset.last_hit_at, None);

        let stale = runtime_snapshot(&adapter);
        adapter
            .toggle_domain(created.summary.rule_id, 2, true)
            .expect("concurrent config update");
        let error = adapter
            .commit_runtime_snapshot(&stale, &stale.rules)
            .expect_err("stale runtime commit");
        assert_eq!(error.view_model.code, "REVISION_CONFLICT");
        let configured = adapter
            .get_domain(created.summary.rule_id)
            .expect("configured rule");
        assert!(configured.enabled);
        assert_eq!(configured.revision, Revision::new(3));
    }
}
