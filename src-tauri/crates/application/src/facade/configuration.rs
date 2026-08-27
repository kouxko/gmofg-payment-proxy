//! 完整应用配置导入导出用例。

use super::{Application, validation::require_confirmation};
use crate::{
    APPLICATION_CONFIGURATION_FORMAT_VERSION, AndroidRuntimeTarget, AppError, AppResult,
    ApplicationConfigurationDocument, OperationResultViewModel, PortableSettings, ProxyWorkspace,
    SettingsDraft, UiTone,
};

impl Application {
    pub async fn application_data_reset(
        &self,
        confirmed: bool,
    ) -> AppResult<OperationResultViewModel> {
        require_confirmation(confirmed, "清除全部配置和测试数据需要显式确认。")?;
        let _gate = self.mutation_gate.lock().await;

        let owners = self.android.runtime_owners().await.map_err(|error| {
            AppError::new(
                "APPLICATION_DATA_RESET_BLOCKED",
                format!(
                    "无法读取设备网络运行实例，未清除数据：{}",
                    error.view_model.message
                ),
            )
            .retryable("请恢复所有设备连接并重试清除。")
        })?;
        let mut stop_errors = Vec::new();
        for owner in owners {
            if let Err(error) = self
                .android
                .network_stop(AndroidRuntimeTarget {
                    serial: owner.serial.clone(),
                    expected_epoch: owner.epoch,
                })
                .await
            {
                stop_errors.push(format!(
                    "{} [{}] {}",
                    owner.serial, error.view_model.code, error.view_model.message
                ));
            }
        }
        if !stop_errors.is_empty() {
            return Err(AppError::new(
                "APPLICATION_DATA_RESET_BLOCKED",
                format!(
                    "设备网络接管停止失败，未清除数据：{}",
                    stop_errors.join("；")
                ),
            )
            .retryable("请先对失败设备执行紧急恢复，再重试清除。"));
        }
        if !self.android.runtime_owners().await?.is_empty() {
            return Err(AppError::new(
                "APPLICATION_DATA_RESET_BLOCKED",
                "设备网络运行实例仍未清空，未清除数据。",
            )
            .retryable("请刷新设备状态并执行紧急恢复后重试。"));
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
            protocol_packages: Vec::new(),
        };
        self.protocol_package_portability
            .reset_application_bundle(document)
            .await?;
        self.android_package_cache.lock().await.clear();

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

    pub(super) async fn workspaces_before_replacement(&self) -> AppResult<Vec<ProxyWorkspace>> {
        let mut has_android_network_profiles = false;
        let mut old_workspaces = Vec::new();
        for current in self.workspaces.snapshot().await?.details {
            self.ensure_workspace_not_running(&current).await?;
            has_android_network_profiles |= !current.android_network_profiles.is_empty();
            old_workspaces.push(current);
        }
        // 完整替换会移除所有本地 Profile 元数据。即使本地存储已经为空，设备端仍可能
        // 残留一个无法映射回 Workspace 的运行态，因此在控制适配器可用时仍主动确认；
        // 只有旧配置确实包含 Profile 时，离线或未选设备才必须阻止替换。
        self.ensure_android_network_replacement_safe(has_android_network_profiles)
            .await?;
        Ok(old_workspaces)
    }

    pub(super) async fn restore_and_replace_configuration(
        &self,
        mut document: ApplicationConfigurationDocument,
    ) -> AppResult<Vec<ProxyWorkspace>> {
        let materials = document.certificate_materials.clone();
        let restored = self
            .restore_certificate_materials(&mut document.workspaces, materials)
            .await?;
        let imported_workspaces = document.workspaces.clone();
        let protocol_packages = document.protocol_packages.clone();
        let replacement = self
            .protocol_package_portability
            .replace_application_bundle(protocol_packages, document)
            .await;
        if let Err(error) = replacement {
            return Err(match self.rollback_restored_certificates(&restored).await {
                Ok(()) => error,
                Err(cleanup) => {
                    super::certificate_portability::certificate_operation_cleanup_error(
                        error, cleanup,
                    )
                }
            });
        }
        Ok(imported_workspaces)
    }
}
