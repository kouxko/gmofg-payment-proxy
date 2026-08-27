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
#[cfg(test)]
use parking_lot::Mutex;
use serde_json::{Map, Value};

use crate::files::RULE_IMPORT_MAX_BYTES;
use crate::{
    AtomicFileExporter, InfrastructureError, IntoSqlitePersistence, SqliteExecutor, SqliteStore,
    WorkspaceRecord,
};

use super::{
    common::{app_error, decode_workspace_record, encode_workspace_record, infra, json_error},
    files::{NativeFileDialog, cancelled},
};

const PERSISTENCE_VERSION_FIELD: &str = "_persistence_version";
const RULE_PERSISTENCE_VERSION: u64 = 1;

#[derive(Debug)]
pub struct RuleRepositoryAdapter {
    #[cfg(test)]
    store: Arc<SqliteStore>,
    executor: SqliteExecutor,
    dialog: Arc<dyn NativeFileDialog>,
    sessions: Arc<dyn SessionQueryPort>,
    exporter: AtomicFileExporter,
    #[cfg(test)]
    operations: Mutex<()>,
    channel_names: BTreeMap<ChannelId, String>,
}

impl RuleRepositoryAdapter {
    #[must_use]
    pub fn new(
        persistence: impl IntoSqlitePersistence,
        dialog: Arc<dyn NativeFileDialog>,
        sessions: Arc<dyn SessionQueryPort>,
        channels: &[ProductChannel],
    ) -> Self {
        let (executor, store) = persistence.into_sqlite_persistence();
        #[cfg(not(test))]
        drop(store);
        Self {
            #[cfg(test)]
            store,
            executor,
            dialog,
            sessions,
            exporter: AtomicFileExporter,
            #[cfg(test)]
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
        }
    }

    /// 规则属于当前选中的 Workspace 聚合；独立 `rules` 表只保留旧 schema，不再读取。
    #[cfg(test)]
    fn load_selected_workspace(&self) -> AppResult<ProxyWorkspace> {
        Self::load_selected_workspace_from(&self.store)
    }

    fn load_selected_workspace_from(store: &SqliteStore) -> AppResult<ProxyWorkspace> {
        let snapshot = infra(store.load_workspaces())?;
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

    #[cfg(test)]
    fn load(&self) -> AppResult<Vec<Rule>> {
        Ok(self.load_selected_workspace()?.rules)
    }

    fn load_from(store: &SqliteStore) -> AppResult<Vec<Rule>> {
        Ok(Self::load_selected_workspace_from(store)?.rules)
    }

    fn load_workspace_for_channel_from(
        store: &SqliteStore,
        channel: &str,
    ) -> AppResult<ProxyWorkspace> {
        let snapshot = infra(store.load_workspaces())?;
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

    fn load_workspace_by_id_from(store: &SqliteStore, id: uuid::Uuid) -> AppResult<ProxyWorkspace> {
        let snapshot = infra(store.load_workspaces())?;
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
            value: encode_workspace_record(workspace)
                .map_err(|message| AppError::new("PERSISTENCE_FAILED", message))?,
            updated_at: Utc::now(),
        })
    }

    fn save_selected_workspace_to(
        store: &SqliteStore,
        mut workspace: ProxyWorkspace,
        expected_revision: u64,
    ) -> AppResult<ProxyWorkspace> {
        workspace.revision = Revision::new(expected_revision).next();
        workspace.validate().map_err(AppError::from)?;
        infra(store.compare_and_swap_selected_workspace(
            workspace.id.as_uuid(),
            expected_revision,
            &Self::workspace_record(&workspace)?,
        ))?;
        Ok(workspace)
    }

    fn save_workspace_to(
        store: &SqliteStore,
        mut workspace: ProxyWorkspace,
        expected_revision: u64,
    ) -> AppResult<ProxyWorkspace> {
        workspace.revision = Revision::new(expected_revision).next();
        workspace.validate().map_err(AppError::from)?;
        infra(
            store.compare_and_swap_workspace(
                expected_revision,
                &Self::workspace_record(&workspace)?,
            ),
        )?;
        Ok(workspace)
    }

    fn save_locked_to(store: &SqliteStore, draft: &AppRuleDraft) -> AppResult<Rule> {
        let mut workspace = Self::load_selected_workspace_from(store)?;
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
        Self::save_selected_workspace_to(store, workspace, expected_workspace_revision)?;
        Ok(changed)
    }

    #[cfg(test)]
    pub(crate) fn get_domain(&self, id: AppRuleId) -> AppResult<Rule> {
        self.load()?
            .into_iter()
            .find(|rule| rule.id.as_uuid() == id)
            .ok_or_else(|| AppError::new("RULE_INVALID", "规则不存在。").entity(id.to_string()))
    }

    #[cfg(test)]
    pub(crate) fn toggle_domain(
        &self,
        id: AppRuleId,
        expected_revision: u64,
        enabled: bool,
    ) -> AppResult<Rule> {
        let _operation = self.operations.lock();
        Self::toggle_domain_to(&self.store, id, expected_revision, enabled)
    }

    fn toggle_domain_to(
        store: &SqliteStore,
        id: AppRuleId,
        expected_revision: u64,
        enabled: bool,
    ) -> AppResult<Rule> {
        let mut workspace = Self::load_selected_workspace_from(store)?;
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
        Self::save_selected_workspace_to(store, workspace, expected_workspace_revision)?;
        Ok(changed)
    }

    pub async fn runtime_snapshot(&self, channel: &str) -> AppResult<RuleRuntimeSnapshot> {
        let channel = channel.to_owned();
        self.executor
            .execute(move |store| {
                let workspace = Self::load_workspace_for_channel_from(store, &channel)?;
                Ok(RuleRuntimeSnapshot::with_collection_identity(
                    Some(workspace.id.as_uuid()),
                    workspace.revision.get(),
                    workspace.rules,
                ))
            })
            .await
    }

    pub async fn commit_runtime_snapshot(
        &self,
        snapshot: &RuleRuntimeSnapshot,
        evaluated_rules: &[Rule],
    ) -> AppResult<u64> {
        let snapshot = snapshot.clone();
        let evaluated_rules = evaluated_rules.to_vec();
        self.executor
            .execute(move |store| {
                if RuleSetSignature::from_rules(&snapshot.rules) != snapshot.signature {
                    return Err(AppError::new(
                        "REVISION_CONFLICT",
                        "规则运行快照签名与内容不一致。",
                    ));
                }
                let collection_id = snapshot.collection_id.ok_or_else(|| {
                    AppError::new("REVISION_CONFLICT", "规则运行快照缺少 Workspace 标识。")
                })?;
                let mut workspace = Self::load_workspace_by_id_from(store, collection_id)?;
                if snapshot.collection_id != Some(workspace.id.as_uuid())
                    || workspace.revision.get() != snapshot.collection_revision
                    || RuleSetSignature::from_rules(&workspace.rules) != snapshot.signature
                {
                    return Err(AppError::new(
                        "REVISION_CONFLICT",
                        "Workspace 或规则集合已在运行快照之后发生变化。",
                    ));
                }
                workspace.rules = runtime_rules(&snapshot, &evaluated_rules)?;
                let expected_revision = workspace.revision.get();
                Ok(
                    Self::save_workspace_to(store, workspace, expected_revision)?
                        .revision
                        .get(),
                )
            })
            .await
    }

    pub async fn reset_runtime_hit_metadata(&self, collection_id: uuid::Uuid) -> AppResult<()> {
        self.executor
            .execute(move |store| {
                let mut workspace = Self::load_workspace_by_id_from(store, collection_id)?;
                let expected_revision = workspace.revision.get();
                for rule in &mut workspace.rules {
                    rule.hit_count = 0;
                    rule.last_hit_at = None;
                }
                Self::save_workspace_to(store, workspace, expected_revision).map(|_| ())
            })
            .await
    }
}

mod action_conversion;
mod conversion;
mod persistence;
mod port;

pub(crate) use action_conversion::{action_to_app, action_to_domain};
pub(crate) use conversion::condition_to_app;
#[cfg(test)]
pub(crate) use conversion::condition_to_domain;
use conversion::{
    app_draft, runtime_rules, summary, to_domain_draft, validation_from_domain, view,
};
use persistence::{
    deserialize_persisted_rule, persisted_rule_error, serialize_persisted_rule,
    validate_persisted_rule,
};

#[cfg(test)]
#[path = "rules/tests/mod.rs"]
mod rules_tests;
