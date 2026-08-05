//! 完整应用配置导入导出用例。

use super::Application;
use crate::{
    APPLICATION_CONFIGURATION_FORMAT_VERSION, AppError, AppResult,
    ApplicationConfigurationDocument, OperationResultViewModel, PortableSettings, UiTone,
    parse_application_configuration, serialize_application_configuration,
};

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
            workspaces.push(self.workspaces.get(summary.id).await?);
        }
        let settings = self.settings.get().await?;
        let document = ApplicationConfigurationDocument {
            format_version: APPLICATION_CONFIGURATION_FORMAT_VERSION,
            selected_workspace_id,
            workspaces,
            settings: PortableSettings::from(&settings.stored),
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
                "完整应用配置已导出。".into()
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
        let document = parse_application_configuration(&bytes)?;
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
        for summary in self.workspaces.list().await? {
            let current = self.workspaces.get(summary.id).await?;
            self.ensure_workspace_not_running(&current).await?;
            has_android_network_profiles |= !current.android_network_profiles.is_empty();
        }
        // 完整替换会移除所有本地 Profile 元数据。即使本地存储已经为空，设备端仍可能
        // 残留一个无法映射回 Workspace 的运行态，因此在控制适配器可用时仍主动确认；
        // 只有旧配置确实包含 Profile 时，离线或未选设备才必须阻止替换。
        self.ensure_android_network_replacement_safe(has_android_network_profiles)
            .await?;
        self.configuration_store.replace_all(document).await?;
        Ok(OperationResultViewModel {
            success: true,
            cancelled: false,
            message: "完整应用配置已原子替换；全局设置将在下次启动代理时生效。".into(),
            ui_tone: UiTone::Positive,
            entity_id: None,
            revision: None,
            requires_restart: true,
        })
    }
}
