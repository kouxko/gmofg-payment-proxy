//! 通用 Workspace 的 `SQLite` 仓储适配器。
//!
//! 这里负责持久化、乐观锁和安全导入导出；它只保存领域模型及系统秘密引用，绝不把
//! PKCS#12 密码、私钥或代理认证明文写入 Workspace JSON。文件选择由独立平台端口
//! 完成，因此同一仓储可被 Tauri、未来 CLI/TUI 和无界面测试复用。

use async_trait::async_trait;
use chrono::Utc;
use intercept_proxy_application::{
    AppError, AppResult, ApplicationConfigurationDocument, ApplicationConfigurationStorePort,
    OperationResultViewModel, ProxyWorkspace, UiTone, WorkspaceCollectionViewModel, WorkspaceId,
    WorkspaceRepositoryPort, WorkspaceSummaryViewModel, WorkspaceValidationViewModel,
    remap_workspace_identity,
};

use crate::{SqliteExecutor, SqliteStore, WorkspaceRecord};

use super::{
    common::{app_error, decode_workspace_record, encode_workspace_record},
    settings::serialize_settings,
};

#[derive(Debug)]
pub struct WorkspaceRepositoryAdapter {
    executor: SqliteExecutor,
}

impl WorkspaceRepositoryAdapter {
    #[must_use]
    pub fn new(persistence: impl Into<SqliteExecutor>) -> Self {
        Self {
            executor: persistence.into(),
        }
    }

    async fn load_snapshot(&self) -> AppResult<(Option<WorkspaceId>, Vec<ProxyWorkspace>)> {
        let snapshot = self
            .executor
            .execute(SqliteStore::load_workspaces)
            .await
            .map_err(app_error)?;
        let selected = snapshot.selected_id.map(WorkspaceId::from_uuid);
        let workspaces = snapshot
            .records
            .into_iter()
            .map(|record| {
                decode_workspace_record(record)
                    .map_err(|message| AppError::new("PERSISTENCE_CORRUPT", message))
            })
            .collect::<AppResult<Vec<_>>>()?;
        Ok((selected, workspaces))
    }

    async fn get_stored(&self, workspace_id: WorkspaceId) -> AppResult<ProxyWorkspace> {
        let record = self
            .executor
            .execute(move |store| store.load_workspace(workspace_id.as_uuid()))
            .await
            .map_err(app_error)?
            .ok_or_else(|| {
                AppError::new("WORKSPACE_NOT_FOUND", "Workspace 不存在或已被删除。")
                    .entity(workspace_id.to_string())
            })?;
        decode_workspace_record(record)
            .map_err(|message| AppError::new("PERSISTENCE_CORRUPT", message))
    }

    pub(crate) fn record(workspace: &ProxyWorkspace) -> AppResult<WorkspaceRecord> {
        Ok(WorkspaceRecord {
            id: workspace.id.as_uuid(),
            revision: workspace.revision.get(),
            value: encode_workspace_record(workspace)
                .map_err(|message| AppError::new("PERSISTENCE_FAILED", message))?,
            updated_at: Utc::now(),
        })
    }
}

#[async_trait]
impl ApplicationConfigurationStorePort for WorkspaceRepositoryAdapter {
    async fn replace_all(&self, document: ApplicationConfigurationDocument) -> AppResult<()> {
        document.validate()?;
        let records = document
            .workspaces
            .iter()
            .map(Self::record)
            .collect::<AppResult<Vec<_>>>()?;
        let settings = serialize_settings(&document.settings.to_draft(None)).map_err(|error| {
            AppError::new(
                "APPLICATION_CONFIGURATION_INVALID",
                format!("完整配置中的 Settings 无法持久化：{error}"),
            )
        })?;
        let selected_id = document.selected_workspace_id.as_uuid();
        self.executor
            .execute(move |store| {
                store.replace_application_configuration(selected_id, &records, &settings)
            })
            .await
            .map_err(app_error)
    }

    async fn reset_all(&self, document: ApplicationConfigurationDocument) -> AppResult<()> {
        document.validate()?;
        let records = document
            .workspaces
            .iter()
            .map(Self::record)
            .collect::<AppResult<Vec<_>>>()?;
        let settings = serialize_settings(&document.settings.to_draft(None)).map_err(|error| {
            AppError::new(
                "APPLICATION_CONFIGURATION_INVALID",
                format!("默认 Settings 无法持久化：{error}"),
            )
        })?;
        let selected_id = document.selected_workspace_id.as_uuid();
        self.executor
            .execute(move |store| store.reset_application_data(selected_id, &records, &settings))
            .await
            .map_err(app_error)
    }
}

#[async_trait]
impl WorkspaceRepositoryPort for WorkspaceRepositoryAdapter {
    async fn snapshot(&self) -> AppResult<WorkspaceCollectionViewModel> {
        let (selected, details) = self.load_snapshot().await?;
        let summaries = details
            .iter()
            .map(|workspace| {
                WorkspaceSummaryViewModel::from_workspace(workspace, selected == Some(workspace.id))
            })
            .collect();
        Ok(WorkspaceCollectionViewModel { summaries, details })
    }

    async fn list(&self) -> AppResult<Vec<WorkspaceSummaryViewModel>> {
        let (selected, workspaces) = self.load_snapshot().await?;
        Ok(workspaces
            .iter()
            .map(|workspace| {
                WorkspaceSummaryViewModel::from_workspace(workspace, selected == Some(workspace.id))
            })
            .collect())
    }

    async fn get(&self, workspace_id: WorkspaceId) -> AppResult<ProxyWorkspace> {
        self.get_stored(workspace_id).await
    }

    async fn create(&self, name: String) -> AppResult<ProxyWorkspace> {
        let workspace = ProxyWorkspace {
            name: name.trim().to_owned(),
            ..ProxyWorkspace::default()
        };
        workspace.validate().map_err(AppError::from)?;
        let record = Self::record(&workspace)?;
        self.executor
            .execute(move |store| store.insert_workspace(&record))
            .await
            .map_err(app_error)?;
        Ok(workspace)
    }

    async fn copy(&self, workspace_id: WorkspaceId) -> AppResult<ProxyWorkspace> {
        let mut workspace = self.get_stored(workspace_id).await?;
        remap_workspace_identity(&mut workspace)?;
        workspace.name = format!("{} Copy", workspace.name);
        workspace.validate().map_err(AppError::from)?;
        let record = Self::record(&workspace)?;
        self.executor
            .execute(move |store| store.insert_workspace(&record))
            .await
            .map_err(app_error)?;
        Ok(workspace)
    }

    async fn select(&self, workspace_id: WorkspaceId) -> AppResult<WorkspaceSummaryViewModel> {
        let workspace = self.get_stored(workspace_id).await?;
        self.executor
            .execute(move |store| store.select_workspace(workspace_id.as_uuid()))
            .await
            .map_err(app_error)?;
        Ok(WorkspaceSummaryViewModel::from_workspace(&workspace, true))
    }

    async fn validate(&self, workspace: ProxyWorkspace) -> AppResult<WorkspaceValidationViewModel> {
        Ok(WorkspaceValidationViewModel::validate(workspace))
    }

    async fn save(&self, mut workspace: ProxyWorkspace) -> AppResult<ProxyWorkspace> {
        workspace.validate().map_err(AppError::from)?;
        let current = self.get_stored(workspace.id).await?;
        current
            .revision
            .verify(workspace.revision)
            .map_err(AppError::from)?;
        let expected_revision = current.revision.get();
        workspace.revision = current.revision.next();
        let record = Self::record(&workspace)?;
        self.executor
            .execute(move |store| store.compare_and_swap_workspace(expected_revision, &record))
            .await
            .map_err(app_error)?;
        Ok(workspace)
    }

    async fn import_workspace(&self, mut workspace: ProxyWorkspace) -> AppResult<ProxyWorkspace> {
        workspace.validate().map_err(AppError::from)?;
        remap_workspace_identity(&mut workspace)?;
        let record = Self::record(&workspace)?;
        self.executor
            .execute(move |store| store.insert_workspace(&record))
            .await
            .map_err(app_error)?;
        Ok(workspace)
    }

    async fn delete(
        &self,
        workspace_id: WorkspaceId,
        expected_revision: u64,
    ) -> AppResult<OperationResultViewModel> {
        self.get_stored(workspace_id).await?;
        self.executor
            .execute(move |store| store.delete_workspace(workspace_id.as_uuid(), expected_revision))
            .await
            .map_err(app_error)?;
        Ok(OperationResultViewModel {
            success: true,
            cancelled: false,
            message: "Workspace 已删除。".into(),
            ui_tone: UiTone::Positive,
            entity_id: Some(workspace_id.to_string()),
            revision: Some(expected_revision),
            requires_restart: false,
        })
    }
}

#[cfg(test)]
#[path = "workspaces/tests.rs"]
mod tests;
