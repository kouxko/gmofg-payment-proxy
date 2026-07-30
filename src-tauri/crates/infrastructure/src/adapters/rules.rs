use std::{collections::BTreeMap, sync::Arc};

use async_trait::async_trait;
use chrono::Utc;
use gmofg_proxy_application::{
    AppError, AppResult, ChannelKind as AppChannelKind, FieldValidationViewModel,
    MessageStage as AppMessageStage, OperationResultViewModel, RuleAction as AppRuleAction,
    RuleCondition as AppRuleCondition, RuleDraft as AppRuleDraft,
    RuleDropResponseMode as AppRuleDropResponseMode, RuleId as AppRuleId,
    RuleJitterScope as AppRuleJitterScope, RuleMatchField as AppRuleMatchField,
    RuleMatchOperator as AppRuleMatchOperator, RuleRepositoryPort, RuleSummaryViewModel,
    RuleTerminalAction as AppRuleTerminalAction, RuleTrafficDirection as AppRuleTrafficDirection,
    RuleValidationViewModel, RuleViewModel, SessionId, SessionQueryPort, UiTone,
};
use gmofg_proxy_domain::{
    ChannelKind, DropResponseMode, JitterScope, MatchCondition, MatchField, MatchOperator,
    MessageStage, Revision, Rule, RuleAction, RuleDraft, RuleEngine, RuleId, RuleRuntimeSnapshot,
    RuleSetSignature, RuntimeEpoch, TerminalAction, TrafficDirection, validate_rule_draft,
};
use parking_lot::Mutex;

use crate::sqlite::RuleRuntimeUpdate;
use crate::{AtomicFileExporter, RuleRecord, SqliteStore};

use super::{
    common::{infra, json_error},
    files::{NativeFileDialog, cancelled},
};

#[derive(Debug)]
pub struct RuleRepositoryAdapter {
    store: Arc<SqliteStore>,
    dialog: Arc<dyn NativeFileDialog>,
    sessions: Arc<dyn SessionQueryPort>,
    exporter: AtomicFileExporter,
    operations: Mutex<()>,
}

impl RuleRepositoryAdapter {
    #[must_use]
    pub fn new(
        store: Arc<SqliteStore>,
        dialog: Arc<dyn NativeFileDialog>,
        sessions: Arc<dyn SessionQueryPort>,
    ) -> Self {
        Self {
            store,
            dialog,
            sessions,
            exporter: AtomicFileExporter,
            operations: Mutex::new(()),
        }
    }

    fn load(&self) -> AppResult<Vec<Rule>> {
        let snapshot = infra(self.store.load_rules_snapshot())?;
        Self::parse_records(snapshot.records)
    }

    fn parse_records(records: Vec<RuleRecord>) -> AppResult<Vec<Rule>> {
        records
            .into_iter()
            .map(|record| {
                let rule: Rule = serde_json::from_value(record.value)
                    .map_err(|error| json_error("持久化规则无效", error))?;
                validate_persisted_rule(&rule).map_err(AppError::from)?;
                Ok(rule)
            })
            .collect()
    }

    fn record(rule: &Rule) -> AppResult<RuleRecord> {
        Ok(RuleRecord {
            id: rule.id.as_uuid(),
            revision: rule.revision.get(),
            enabled: rule.enabled,
            value: serde_json::to_value(rule)
                .map_err(|error| json_error("规则序列化失败", error))?,
            updated_at: Utc::now(),
        })
    }

    fn replace_all(&self, expected_collection_revision: u64, rules: &[Rule]) -> AppResult<()> {
        let records = rules
            .iter()
            .map(Self::record)
            .collect::<AppResult<Vec<_>>>()?;
        infra(
            self.store
                .replace_rules_atomically(expected_collection_revision, &records),
        )
        .map(|_| ())
    }

    fn save_locked(&self, draft: &AppRuleDraft) -> AppResult<Rule> {
        let mut rules = self.load()?;
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
        let record = Self::record(&changed)?;
        if draft.rule_id.is_some() {
            let expected_revision = draft.expected_revision.ok_or_else(|| {
                AppError::new("REVISION_CONFLICT", "修改规则必须提供当前 revision。")
            })?;
            infra(self.store.compare_and_swap_rule(expected_revision, &record))?;
        } else {
            infra(self.store.insert_rule(&record))?;
        }
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
        let mut rules = self.load()?;
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
        infra(
            self.store
                .compare_and_swap_rule(expected_revision, &Self::record(&changed)?),
        )?;
        Ok(changed)
    }

    pub fn runtime_snapshot(&self) -> AppResult<RuleRuntimeSnapshot> {
        let _operation = self.operations.lock();
        let snapshot = infra(self.store.load_rules_snapshot())?;
        let rules = Self::parse_records(snapshot.records)?;
        Ok(RuleRuntimeSnapshot::with_collection_revision(
            snapshot.revision,
            rules,
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
        let updates = runtime_updates(snapshot, evaluated_rules)?;
        let signature = snapshot
            .signature
            .entries
            .iter()
            .map(|entry| (entry.rule_id.as_uuid(), entry.revision.get()))
            .collect::<Vec<_>>();
        infra(self.store.compare_and_swap_rule_runtime(
            snapshot.collection_revision,
            &signature,
            &updates,
        ))
    }

    pub fn reset_runtime_hit_metadata(&self) -> AppResult<()> {
        let _operation = self.operations.lock();
        let stored = infra(self.store.load_rules_snapshot())?;
        let collection_revision = stored.revision;
        let rules = Self::parse_records(stored.records)?;
        let signature = RuleSetSignature::from_rules(&rules);
        let updates = rules
            .iter()
            .map(|rule| RuleRuntimeUpdate {
                id: rule.id.as_uuid(),
                expected_revision: rule.revision.get(),
                revision: rule.revision.get(),
                enabled: rule.enabled,
                hit_count: 0,
                last_hit_at: None,
            })
            .collect::<Vec<_>>();
        let signature = signature
            .entries
            .iter()
            .map(|entry| (entry.rule_id.as_uuid(), entry.revision.get()))
            .collect::<Vec<_>>();
        infra(
            self.store
                .compare_and_swap_rule_runtime(collection_revision, &signature, &updates),
        )
        .map(|_| ())
    }
}

fn validate_persisted_rule(rule: &Rule) -> Result<(), gmofg_proxy_domain::DomainError> {
    validate_rule_draft(&RuleDraft {
        expected_revision: Some(rule.revision),
        name: rule.name.clone(),
        description: rule.description.clone(),
        enabled: rule.enabled,
        priority: rule.priority,
        created_order: rule.created_order,
        channel: rule.channel,
        stage: rule.stage,
        conditions: rule.conditions.clone(),
        actions: rule.actions.clone(),
        one_shot: rule.one_shot,
    })
}

#[async_trait]
impl RuleRepositoryPort for RuleRepositoryAdapter {
    async fn list(&self) -> AppResult<Vec<RuleSummaryViewModel>> {
        self.load()?.iter().map(summary).collect()
    }

    async fn get(&self, rule_id: AppRuleId) -> AppResult<RuleViewModel> {
        view(&self.get_domain(rule_id)?)
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
            field: gmofg_proxy_domain::MatchField::PathOrRequestType,
            operator: gmofg_proxy_domain::MatchOperator::Equals(session.summary.target.clone()),
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
        view(&self.save_locked(&draft)?)
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
        view(&self.save_locked(&draft)?)
    }

    async fn delete(
        &self,
        rule_id: AppRuleId,
        expected_revision: u64,
    ) -> AppResult<OperationResultViewModel> {
        let _operation = self.operations.lock();
        let rule = self
            .load()?
            .into_iter()
            .find(|rule| rule.id.as_uuid() == rule_id)
            .ok_or_else(|| AppError::new("RULE_INVALID", "规则不存在。"))?;
        if rule.revision.get() != expected_revision {
            return Err(AppError::new("REVISION_CONFLICT", "规则已被其他操作更新。"));
        }
        infra(self.store.delete_rule(rule_id, expected_revision))?;
        Ok(OperationResultViewModel::success("规则已删除。"))
    }

    async fn toggle(
        &self,
        rule_id: AppRuleId,
        expected_revision: u64,
        enabled: bool,
    ) -> AppResult<RuleViewModel> {
        view(&self.toggle_domain(rule_id, expected_revision, enabled)?)
    }

    async fn import(&self) -> AppResult<OperationResultViewModel> {
        let expected_collection_revision = infra(self.store.load_rules_snapshot())?.revision;
        let Some(path) = self.dialog.choose_open_file("rules_json")? else {
            return Ok(cancelled("已取消规则导入。"));
        };
        let bytes = infra(self.exporter.read(&path))?;
        let rules: Vec<Rule> = serde_json::from_slice(&bytes)
            .map_err(|error| json_error("规则导入文件无效", error))?;
        for rule in &rules {
            validate_persisted_rule(rule).map_err(AppError::from)?;
        }
        let _operation = self.operations.lock();
        self.replace_all(expected_collection_revision, &rules)?;
        Ok(OperationResultViewModel::success(format!(
            "已导入 {} 条规则。",
            rules.len()
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

fn runtime_updates(
    snapshot: &RuleRuntimeSnapshot,
    evaluated_rules: &[Rule],
) -> AppResult<Vec<RuleRuntimeUpdate>> {
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
            Ok(RuleRuntimeUpdate {
                id: original.id.as_uuid(),
                expected_revision: original.revision.get(),
                revision: evaluated.revision.get(),
                enabled: evaluated.enabled,
                hit_count: evaluated.hit_count,
                last_hit_at: evaluated.last_hit_at,
            })
        })
        .collect()
}

fn to_domain_draft(
    draft: &AppRuleDraft,
    creation_order: u64,
) -> Result<RuleDraft, gmofg_proxy_domain::DomainError> {
    let stage = match draft.stage {
        Some(AppMessageStage::Request) => MessageStage::Request,
        Some(AppMessageStage::Response) => MessageStage::Response,
        Some(AppMessageStage::TlsHandshake) => MessageStage::TlsHandshake,
        _ => {
            return Err(gmofg_proxy_domain::DomainError::new(
                gmofg_proxy_domain::ErrorCode::RuleInvalid,
                "规则必须指定 TLS 握手、请求或响应阶段",
            )
            .with_field_error("stage", "阶段无效"));
        }
    };
    let priority = u32::try_from(draft.priority).map_err(|_| {
        gmofg_proxy_domain::DomainError::new(
            gmofg_proxy_domain::ErrorCode::RuleInvalid,
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
                let field = if matches!(action, AppRuleAction::SetJsonField { .. }) {
                    format!("actions.{index}.value_json")
                } else {
                    format!("actions.{index}")
                };
                gmofg_proxy_domain::DomainError::new(
                    gmofg_proxy_domain::ErrorCode::RuleInvalid,
                    "规则动作无效",
                )
                .with_field_error(field, error.to_string())
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
        channel: draft.channel.map(|channel| match channel {
            AppChannelKind::Transaction => ChannelKind::Transaction,
            AppChannelKind::Dll => ChannelKind::Dll,
        }),
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
        channel: rule.channel.map(|channel| match channel {
            ChannelKind::Transaction => AppChannelKind::Transaction,
            ChannelKind::Dll => AppChannelKind::Dll,
        }),
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
            shift_jis_body,
        } => TerminalAction::MockResponse {
            status: *status,
            headers: headers.clone(),
            shift_jis_body: shift_jis_body.clone(),
        },
        AppRuleTerminalAction::InvalidJson { shift_jis_body } => TerminalAction::InvalidJson {
            shift_jis_body: shift_jis_body.clone(),
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
            shift_jis_body,
        } => AppRuleTerminalAction::MockResponse {
            status: *status,
            headers: headers.clone(),
            shift_jis_body: shift_jis_body.clone(),
        },
        TerminalAction::InvalidJson { shift_jis_body } => AppRuleTerminalAction::InvalidJson {
            shift_jis_body: shift_jis_body.clone(),
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

fn summary(rule: &Rule) -> AppResult<RuleSummaryViewModel> {
    Ok(RuleSummaryViewModel {
        rule_id: rule.id.as_uuid(),
        revision: rule.revision.get(),
        name: rule.name.clone(),
        enabled: rule.enabled,
        priority: i32::try_from(rule.priority)
            .map_err(|error| json_error("规则优先级超出 UI 范围", error))?,
        creation_order: rule.created_order,
        channel_text: rule.channel.map_or("全部".into(), |channel| match channel {
            ChannelKind::Transaction => "交易".into(),
            ChannelKind::Dll => "DLL".into(),
        }),
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

fn view(rule: &Rule) -> AppResult<RuleViewModel> {
    Ok(RuleViewModel {
        summary: summary(rule)?,
        draft: app_draft(rule)?,
    })
}

fn validation_from_domain(error: &gmofg_proxy_domain::DomainError) -> RuleValidationViewModel {
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

    use chrono::TimeZone;
    use gmofg_proxy_domain::{MatchContext, TerminalIdentity};

    use super::*;
    use crate::adapters::{FileSelection, NativeFileDialog};

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
    struct MutatingOpenDialog {
        path: PathBuf,
        store: Arc<SqliteStore>,
        concurrent_rule: RuleRecord,
    }

    impl NativeFileDialog for MutatingOpenDialog {
        fn choose_open_file(&self, _: &str) -> AppResult<Option<PathBuf>> {
            infra(self.store.insert_rule(&self.concurrent_rule))?;
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
            channel: Some(AppChannelKind::Transaction),
            stage: Some(AppMessageStage::Request),
            conditions: Vec::new(),
            actions: vec![AppRuleAction::Delay { milliseconds: 10 }],
            one_shot,
        }
    }

    fn adapter() -> Arc<RuleRepositoryAdapter> {
        Arc::new(RuleRepositoryAdapter::new(
            Arc::new(SqliteStore::in_memory().expect("store")),
            Arc::new(NoDialog),
            Arc::new(gmofg_proxy_application::InMemorySessionStore::default()),
        ))
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
                shift_jis_body: vec![0x82, 0xa0],
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
        primary_store
            .insert_rule(&RuleRepositoryAdapter::record(&existing).expect("existing record"))
            .expect("seed existing");
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
                concurrent_rule: RuleRepositoryAdapter::record(&concurrent)
                    .expect("concurrent record"),
            }),
            Arc::new(gmofg_proxy_application::InMemorySessionStore::default()),
        );

        let error = adapter.import().await.expect_err("stale import");
        assert_eq!(error.view_model.code, "REVISION_CONFLICT");
        let stored = primary_store.list_rules().expect("stored rules");
        assert_eq!(stored.len(), 2);
        assert!(
            stored
                .iter()
                .any(|record| record.id == existing.id.as_uuid())
        );
        assert!(
            stored
                .iter()
                .any(|record| record.id == concurrent.id.as_uuid())
        );
    }

    #[tokio::test]
    async fn runtime_commit_is_full_signature_cas_and_reset_preserves_enabled() {
        let adapter = adapter();
        let created = adapter
            .save(request_delay_draft("one-shot", true))
            .await
            .expect("create");
        let snapshot = adapter.runtime_snapshot().expect("snapshot");
        let epoch = RuntimeEpoch::new();
        let terminal = TerminalIdentity {
            source_ip: "127.0.0.1".into(),
            certificate_sha256: String::new(),
        };
        let mut engine = RuleEngine::new(epoch, snapshot.rules.clone());
        engine.evaluate(
            &MatchContext {
                runtime_epoch: epoch,
                channel: ChannelKind::Transaction,
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
            .reset_runtime_hit_metadata()
            .expect("explicit reset");
        let reset = adapter
            .get_domain(created.summary.rule_id)
            .expect("reset rule");
        assert!(!reset.enabled);
        assert_eq!(reset.revision, Revision::new(2));
        assert_eq!(reset.hit_count, 0);
        assert_eq!(reset.last_hit_at, None);

        let stale = adapter.runtime_snapshot().expect("stale snapshot");
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
