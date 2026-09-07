//! Runtime access to the authoritative Workspace rule collection.

use chrono::Utc;
use intercept_proxy_application::{AppError, AppResult, ProxyWorkspace};
use intercept_proxy_domain::{Revision, RuleLifecycleDelta, RuleRuntimeSnapshot, RuleSetSignature};

use crate::{
    InfrastructureError, IntoSqlitePersistence, SqliteExecutor, SqliteStore, WorkspaceRecord,
};

use super::common::{app_error, decode_workspace_record, encode_workspace_record, infra};

pub(crate) mod conversion;
use conversion::apply_runtime_deltas;

fn persisted_rule_error(message: String) -> AppError {
    app_error(InfrastructureError::PersistenceCorrupt {
        entity: "rule",
        message,
    })
}

#[derive(Debug)]
pub struct RuleRepositoryAdapter {
    executor: SqliteExecutor,
}

impl RuleRepositoryAdapter {
    #[must_use]
    pub fn new(persistence: impl IntoSqlitePersistence) -> Self {
        let (executor, store) = persistence.into_sqlite_persistence();
        drop(store);
        Self { executor }
    }

    fn load_workspace_for_channel_from(
        store: &SqliteStore,
        channel: &str,
    ) -> AppResult<ProxyWorkspace> {
        let snapshot = infra(store.load_workspaces())?;
        let mut found = None;
        for record in snapshot.records {
            let workspace = decode_workspace_record(record).map_err(persisted_rule_error)?;
            if workspace
                .listeners
                .iter()
                .any(|listener| listener.id.to_string() == channel)
            {
                if found.is_some() {
                    return Err(persisted_rule_error(format!(
                        "代理入口 {channel} 同时属于多个 Workspace。"
                    )));
                }
                found = Some(workspace);
            }
        }
        let workspace = found.ok_or_else(|| {
            AppError::new(
                "WORKSPACE_NOT_FOUND",
                "找不到运行中代理入口所属的 Workspace。",
            )
            .entity(channel.to_owned())
        })?;
        workspace.validate().map_err(AppError::from)?;
        Ok(workspace)
    }

    fn load_workspace_by_id_from(store: &SqliteStore, id: uuid::Uuid) -> AppResult<ProxyWorkspace> {
        let record = infra(store.load_workspaces())?
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

    pub async fn runtime_snapshot(&self, channel: &str) -> AppResult<RuleRuntimeSnapshot> {
        let channel = channel.to_owned();
        self.executor
            .execute(move |store| {
                let workspace = Self::load_workspace_for_channel_from(store, &channel)?;
                Ok(RuleRuntimeSnapshot::with_collection_identity_and_order(
                    Some(workspace.id.as_uuid()),
                    workspace.revision.get(),
                    workspace.rule_definitions.clone(),
                    workspace.runtime_rule_execution_order(),
                ))
            })
            .await
    }

    pub async fn commit_runtime_deltas(
        &self,
        snapshot: &RuleRuntimeSnapshot,
        deltas: &[RuleLifecycleDelta],
    ) -> AppResult<u64> {
        let snapshot = snapshot.clone();
        let deltas = deltas.to_vec();
        self.executor
            .execute(move |store| {
                if RuleSetSignature::from_definitions(&snapshot.rules) != snapshot.signature {
                    return Err(AppError::new(
                        "REVISION_CONFLICT",
                        "规则运行快照签名与内容不一致。",
                    ));
                }
                let collection_id = snapshot.collection_id.ok_or_else(|| {
                    AppError::new("REVISION_CONFLICT", "规则运行快照缺少 Workspace 标识。")
                })?;
                let mut workspace = Self::load_workspace_by_id_from(store, collection_id)?;
                if workspace.revision.get() != snapshot.collection_revision
                    || RuleSetSignature::from_definitions(&workspace.rule_definitions)
                        != snapshot.signature
                {
                    return Err(AppError::new(
                        "REVISION_CONFLICT",
                        "Workspace 或规则集合已在运行快照之后发生变化。",
                    ));
                }
                workspace.rule_definitions = apply_runtime_deltas(&snapshot, &deltas)?;
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
                if workspace.reset_runtime_rule_hit_metadata()? {
                    Self::save_workspace_to(store, workspace, expected_revision).map(|_| ())
                } else {
                    Ok(())
                }
            })
            .await
    }
}
