//! 完整应用配置导入导出用例。

use super::{Application, validation::require_confirmation};
use crate::{
    APPLICATION_CONFIGURATION_FORMAT_VERSION, AndroidNetworkState, AppError, AppResult,
    ApplicationConfigurationDocument, OperationResultViewModel, PortableSettings, ProxyWorkspace,
    SettingsDraft, UiTone, parse_application_configuration,
    retain_reachable_certificate_references, serialize_application_configuration,
};
use chrono::Utc;

impl Application {
    pub async fn application_configuration_export(&self) -> AppResult<OperationResultViewModel> {
        let summaries = self.workspaces.list().await?;
        let selected_workspace_id = summaries
            .iter()
            .find(|summary| summary.selected)
            .map(|summary| summary.id)
            .ok_or_else(|| AppError::new("WORKSPACE_NOT_SELECTED", "请先选择一个 Workspace。"))?;
        let mut workspaces = Vec::with_capacity(summaries.len());
        for summary in summaries {
            let mut workspace = self.workspaces.get(summary.id).await?;
            retain_reachable_certificate_references(&mut workspace);
            workspaces.push(workspace);
        }
        let settings = self.settings.get().await?;
        let certificate_materials = self.export_certificate_materials(&workspaces).await?;
        let document = ApplicationConfigurationDocument {
            format_version: APPLICATION_CONFIGURATION_FORMAT_VERSION,
            selected_workspace_id,
            workspaces,
            settings: PortableSettings::from(&settings.stored),
            certificate_materials,
        };
        let bytes = serialize_application_configuration(&document)?;
        let saved = self
            .workspace_documents
            .save_export_application_configuration("intercept-proxy.intercept-config".into(), bytes)
            .await?;
        Ok(OperationResultViewModel {
            success: saved,
            cancelled: !saved,
            message: if saved {
                "完整应用配置与证书材料已导出到单个文件。".into()
            } else {
                "已取消导出完整应用配置。".into()
            },
            ui_tone: if saved {
                UiTone::Positive
            } else {
                UiTone::Neutral
            },
            entity_id: None,
            revision: None,
            requires_restart: false,
        })
    }

    pub async fn application_data_reset(
        &self,
        confirmed: bool,
    ) -> AppResult<OperationResultViewModel> {
        require_confirmation(confirmed, "清除全部配置和测试数据需要显式确认。")?;
        let _gate = self.mutation_gate.lock().await;

        if let Ok(status) = self.android.network_status().await
            && matches!(
                status.state,
                AndroidNetworkState::StartRequested
                    | AndroidNetworkState::Running
                    | AndroidNetworkState::StopRequested
            )
        {
            self.android.network_stop().await.map_err(|error| {
                AppError::new(
                    "APPLICATION_DATA_RESET_BLOCKED",
                    format!(
                        "设备网络接管停止失败，未清除数据：{}",
                        error.view_model.message
                    ),
                )
                .retryable("请先在设备网络页执行紧急恢复，再重试清除。")
            })?;
        }
        self.app_shutdown_inner().await?;

        let workspace = ProxyWorkspace {
            name: "Default Workspace".into(),
            ..ProxyWorkspace::default()
        };
        let document = ApplicationConfigurationDocument {
            format_version: APPLICATION_CONFIGURATION_FORMAT_VERSION,
            selected_workspace_id: workspace.id,
            workspaces: vec![workspace],
            settings: PortableSettings::from(&SettingsDraft::default()),
            certificate_materials: Vec::new(),
        };
        self.configuration_store.reset_all(document).await?;
        *self.android_package_cache.lock().await = None;

        Ok(OperationResultViewModel {
            success: true,
            cancelled: false,
            message: "全部配置与测试数据已清除，应用将重启并重建干净初始状态。".into(),
            ui_tone: UiTone::Positive,
            entity_id: None,
            revision: None,
            requires_restart: true,
        })
    }

    pub async fn application_configuration_import(&self) -> AppResult<OperationResultViewModel> {
        let _gate = self.mutation_gate.lock().await;
        let Some(bytes) = self
            .workspace_documents
            .pick_import_application_configuration()
            .await?
        else {
            return Ok(OperationResultViewModel {
                success: false,
                cancelled: true,
                message: "已取消导入完整应用配置。".into(),
                ui_tone: UiTone::Neutral,
                entity_id: None,
                revision: None,
                requires_restart: false,
            });
        };
        let mut document = parse_application_configuration(&bytes)?;
        let imported_settings = document.settings.to_draft(None);
        let settings_validation = self.settings.validate(&imported_settings).await?;
        if !settings_validation.valid {
            return Err(AppError::field(
                "APPLICATION_CONFIGURATION_INVALID",
                "完整配置中的全局 Settings 未通过校验。",
                settings_validation.field_errors,
            ));
        }
        let mut has_android_network_profiles = false;
        let mut old_workspaces = Vec::new();
        for summary in self.workspaces.list().await? {
            let current = self.workspaces.get(summary.id).await?;
            self.ensure_workspace_not_running(&current).await?;
            has_android_network_profiles |= !current.android_network_profiles.is_empty();
            old_workspaces.push(current);
        }
        // 完整替换会移除所有本地 Profile 元数据。即使本地存储已经为空，设备端仍可能
        // 残留一个无法映射回 Workspace 的运行态，因此在控制适配器可用时仍主动确认；
        // 只有旧配置确实包含 Profile 时，离线或未选设备才必须阻止替换。
        self.ensure_android_network_replacement_safe(has_android_network_profiles)
            .await?;
        let materials = document.certificate_materials.clone();
        let restored = self
            .restore_certificate_materials(&mut document.workspaces, materials)
            .await?;
        let imported_workspaces = document.workspaces.clone();
        if let Err(error) = self.configuration_store.replace_all(document).await {
            return Err(match self.rollback_restored_certificates(&restored).await {
                Ok(()) => error,
                Err(cleanup) => {
                    super::certificate_portability::certificate_operation_cleanup_error(
                        error, cleanup,
                    )
                }
            });
        }
        let cleanup_warning = self
            .discard_replaced_certificate_materials(&old_workspaces, &imported_workspaces)
            .await
            .err();
        if let Some(error) = &cleanup_warning {
            self.events.publish(
                None,
                Utc::now(),
                None,
                None,
                crate::UiEventPayload::ResourceWarning {
                    message: error.view_model.message.clone(),
                },
            );
        }
        let (message, ui_tone) = cleanup_warning.as_ref().map_or_else(
            || {
                (
                    "完整应用配置已原子替换；全局设置将在下次启动代理时生效。".into(),
                    UiTone::Positive,
                )
            },
            |error| {
                (
                    format!(
                        "完整应用配置已导入；旧证书材料清理未全部完成：{}",
                        error.view_model.message
                    ),
                    UiTone::Warning,
                )
            },
        );
        Ok(OperationResultViewModel {
            success: true,
            cancelled: false,
            message,
            ui_tone,
            entity_id: None,
            revision: None,
            requires_restart: true,
        })
    }
}
