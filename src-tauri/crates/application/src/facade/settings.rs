//! 设置校验与保存用例。

use super::{
    Application,
    validation::{
        ensure_valid, normalize_settings, parse_sans_raw, require_confirmation,
        validate_settings_locally,
    },
};
use crate::{AppResult, SettingsDraft, SettingsValidationViewModel, SettingsViewModel};

impl Application {
    pub async fn settings_get(&self) -> AppResult<SettingsViewModel> {
        self.settings.get().await
    }

    pub async fn settings_validate(
        &self,
        draft: SettingsDraft,
    ) -> AppResult<SettingsValidationViewModel> {
        let draft = normalize_settings(draft);
        let mut validation = validate_settings_locally(&draft);
        if !validation.valid {
            return Ok(validation);
        }
        validation = self.settings.validate(&draft).await?;
        if !validation.valid {
            return Ok(validation);
        }

        // 证书和监听地址已经属于 Workspace Listener。系统设置校验只处理全局容量、
        // 超时与应用行为，避免用户修改内存上限时被无关的入口证书状态阻断。
        Ok(validation)
    }

    pub async fn settings_validate_input(
        &self,
        mut draft: SettingsDraft,
        leaf_sans_raw: String,
    ) -> AppResult<SettingsValidationViewModel> {
        draft.leaf_sans = parse_sans_raw(&leaf_sans_raw);
        self.settings_validate(draft).await
    }

    pub async fn settings_save(&self, draft: SettingsDraft) -> AppResult<SettingsViewModel> {
        let _gate = self.mutation_gate.lock().await;
        let saved = self.settings_save_inner(draft).await?;
        self.publish_settings(&saved);
        Ok(saved)
    }

    async fn settings_save_inner(&self, draft: SettingsDraft) -> AppResult<SettingsViewModel> {
        let draft = normalize_settings(draft);
        let validation = self.settings_validate(draft.clone()).await?;
        ensure_valid("CONFIG_INVALID", "设置校验失败。", &validation)?;
        self.settings.save(draft).await
    }

    pub async fn settings_save_input(
        &self,
        mut draft: SettingsDraft,
        leaf_sans_raw: String,
    ) -> AppResult<SettingsViewModel> {
        draft.leaf_sans = parse_sans_raw(&leaf_sans_raw);
        self.settings_save(draft).await
    }

    pub async fn settings_reset_defaults(&self, confirmed: bool) -> AppResult<SettingsDraft> {
        require_confirmation(confirmed, "恢复默认设置需要确认。")?;
        self.settings.defaults().await
    }
}
