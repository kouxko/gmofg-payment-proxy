//! 通用 Workspace 的 `SQLite` 仓储适配器。
//!
//! 这里负责持久化、乐观锁和安全导入导出；它只保存领域模型及系统秘密引用，绝不把
//! PKCS#12 密码、私钥或代理认证明文写入 Workspace JSON。文件选择由独立平台端口
//! 完成，因此同一仓储可被 Tauri、未来 CLI/TUI 和无界面测试复用。

use std::sync::Arc;

use async_trait::async_trait;
use chrono::Utc;
use intercept_proxy_application::{
    AppError, AppResult, ApplicationConfigurationDocument, ApplicationConfigurationStorePort,
    MAX_APPLICATION_CONFIGURATION_BYTES, MAX_WORKSPACE_DOCUMENT_BYTES, OperationResultViewModel,
    ProxyWorkspace, UiTone, WorkspaceDocumentPort, WorkspaceId, WorkspaceRepositoryPort,
    WorkspaceSummaryViewModel, WorkspaceValidationViewModel, parse_workspace_document,
    remap_workspace_identity, serialize_workspace_document,
};

use crate::{AtomicFileExporter, SqliteStore, WorkspaceRecord};

use super::{
    NativeFileDialog,
    common::{app_error, decode_workspace_record, encode_workspace_record, infra},
    settings::serialize_settings,
};

#[derive(Debug)]
pub struct WorkspaceRepositoryAdapter {
    store: Arc<SqliteStore>,
}

/// 原生 Dialog/文件系统到应用文档端口的薄适配器。
#[derive(Debug)]
pub struct WorkspaceDocumentAdapter {
    dialog: Arc<dyn NativeFileDialog>,
    exporter: AtomicFileExporter,
}

impl WorkspaceDocumentAdapter {
    #[must_use]
    pub fn new(dialog: Arc<dyn NativeFileDialog>) -> Self {
        Self {
            dialog,
            exporter: AtomicFileExporter,
        }
    }
}

#[async_trait]
impl WorkspaceDocumentPort for WorkspaceDocumentAdapter {
    async fn pick_import_document(&self) -> AppResult<Option<Vec<u8>>> {
        let Some(path) = self.dialog.choose_open_file("intercept_workspace")? else {
            return Ok(None);
        };
        infra(
            self.exporter
                .read_bounded(&path, MAX_WORKSPACE_DOCUMENT_BYTES as u64),
        )
        .map(Some)
    }

    async fn save_export_document(
        &self,
        suggested_file_name: String,
        document: Vec<u8>,
    ) -> AppResult<bool> {
        let Some(selection) = self
            .dialog
            .choose_save_file("intercept_workspace", &suggested_file_name)?
        else {
            return Ok(false);
        };
        infra(
            self.exporter
                .write(&selection.path, &document, selection.overwrite_confirmed),
        )?;
        Ok(true)
    }

    async fn pick_import_application_configuration(&self) -> AppResult<Option<Vec<u8>>> {
        let Some(path) = self.dialog.choose_open_file("intercept_configuration")? else {
            return Ok(None);
        };
        infra(
            self.exporter
                .read_bounded(&path, MAX_APPLICATION_CONFIGURATION_BYTES as u64),
        )
        .map(Some)
    }

    async fn save_export_application_configuration(
        &self,
        suggested_file_name: String,
        document: Vec<u8>,
    ) -> AppResult<bool> {
        let Some(selection) = self
            .dialog
            .choose_save_file("intercept_configuration", &suggested_file_name)?
        else {
            return Ok(false);
        };
        infra(
            self.exporter
                .write(&selection.path, &document, selection.overwrite_confirmed),
        )?;
        Ok(true)
    }
}

impl WorkspaceRepositoryAdapter {
    #[must_use]
    pub fn new(store: Arc<SqliteStore>) -> Self {
        Self { store }
    }

    fn snapshot(&self) -> AppResult<(Option<WorkspaceId>, Vec<ProxyWorkspace>)> {
        let snapshot = infra(self.store.load_workspaces())?;
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

    fn get_stored(&self, workspace_id: WorkspaceId) -> AppResult<ProxyWorkspace> {
        self.snapshot()?
            .1
            .into_iter()
            .find(|workspace| workspace.id == workspace_id)
            .ok_or_else(|| {
                AppError::new("WORKSPACE_NOT_FOUND", "Workspace 不存在或已被删除。")
                    .entity(workspace_id.to_string())
            })
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
        infra(self.store.replace_application_configuration(
            document.selected_workspace_id.as_uuid(),
            &records,
            &settings,
        ))
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
        infra(self.store.reset_application_data(
            document.selected_workspace_id.as_uuid(),
            &records,
            &settings,
        ))
    }
}

#[async_trait]
impl WorkspaceRepositoryPort for WorkspaceRepositoryAdapter {
    async fn list(&self) -> AppResult<Vec<WorkspaceSummaryViewModel>> {
        let (selected, workspaces) = self.snapshot()?;
        Ok(workspaces
            .iter()
            .map(|workspace| {
                WorkspaceSummaryViewModel::from_workspace(workspace, selected == Some(workspace.id))
            })
            .collect())
    }

    async fn get(&self, workspace_id: WorkspaceId) -> AppResult<ProxyWorkspace> {
        self.get_stored(workspace_id)
    }

    async fn create(&self, name: String) -> AppResult<ProxyWorkspace> {
        let workspace = ProxyWorkspace {
            name: name.trim().to_owned(),
            ..ProxyWorkspace::default()
        };
        workspace.validate().map_err(AppError::from)?;
        infra(self.store.insert_workspace(&Self::record(&workspace)?))?;
        Ok(workspace)
    }

    async fn copy(&self, workspace_id: WorkspaceId) -> AppResult<ProxyWorkspace> {
        let mut workspace = self.get_stored(workspace_id)?;
        remap_workspace_identity(&mut workspace)?;
        workspace.name = format!("{} Copy", workspace.name);
        workspace.validate().map_err(AppError::from)?;
        infra(self.store.insert_workspace(&Self::record(&workspace)?))?;
        Ok(workspace)
    }

    async fn select(&self, workspace_id: WorkspaceId) -> AppResult<WorkspaceSummaryViewModel> {
        let workspace = self.get_stored(workspace_id)?;
        infra(self.store.select_workspace(workspace_id.as_uuid()))?;
        Ok(WorkspaceSummaryViewModel::from_workspace(&workspace, true))
    }

    async fn validate(&self, workspace: ProxyWorkspace) -> AppResult<WorkspaceValidationViewModel> {
        Ok(WorkspaceValidationViewModel::validate(workspace))
    }

    async fn save(&self, mut workspace: ProxyWorkspace) -> AppResult<ProxyWorkspace> {
        workspace.validate().map_err(AppError::from)?;
        let current = self.get_stored(workspace.id)?;
        current
            .revision
            .verify(workspace.revision)
            .map_err(AppError::from)?;
        let expected_revision = current.revision.get();
        workspace.revision = current.revision.next();
        infra(
            self.store
                .compare_and_swap_workspace(expected_revision, &Self::record(&workspace)?),
        )?;
        Ok(workspace)
    }

    async fn import_workspace(&self, mut workspace: ProxyWorkspace) -> AppResult<ProxyWorkspace> {
        workspace.validate().map_err(AppError::from)?;
        remap_workspace_identity(&mut workspace)?;
        infra(self.store.insert_workspace(&Self::record(&workspace)?))?;
        Ok(workspace)
    }

    async fn delete(
        &self,
        workspace_id: WorkspaceId,
        expected_revision: u64,
    ) -> AppResult<OperationResultViewModel> {
        self.get_stored(workspace_id)?;
        self.store
            .delete_workspace(workspace_id.as_uuid(), expected_revision)
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

    async fn import_document(&self, document: Vec<u8>) -> AppResult<ProxyWorkspace> {
        self.import_workspace(parse_workspace_document(&document)?.workspace)
            .await
    }

    async fn export_document(&self, workspace_id: WorkspaceId) -> AppResult<Vec<u8>> {
        serialize_workspace_document(&intercept_proxy_application::WorkspaceDocument {
            format_version: intercept_proxy_application::WORKSPACE_DOCUMENT_FORMAT_VERSION,
            workspace: self.get_stored(workspace_id)?,
            certificate_materials: Vec::new(),
            protocol_packages: Vec::new(),
        })
    }
}

#[cfg(test)]
#[path = "workspaces/tests.rs"]
mod tests;
