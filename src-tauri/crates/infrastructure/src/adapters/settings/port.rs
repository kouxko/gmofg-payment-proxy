use async_trait::async_trait;

use super::{
    AppError, AppResult, SettingsDraft, SettingsRepositoryAdapter, SettingsRepositoryPort,
    SettingsValidationViewModel, SettingsViewModel, infra, json_error, serialize_settings,
};

#[async_trait]
impl SettingsRepositoryPort for SettingsRepositoryAdapter {
    async fn defaults(&self) -> AppResult<SettingsDraft> {
        Ok(self.defaults.clone())
    }

    async fn get(&self) -> AppResult<SettingsViewModel> {
        self.view()
    }

    async fn validate(&self, draft: &SettingsDraft) -> AppResult<SettingsValidationViewModel> {
        let mut validation = Self::validate_domain(draft);
        self.validate_catalog(draft, &mut validation);
        if validation.valid {
            self.validate_ports(draft, &mut validation);
        }
        Ok(validation)
    }

    async fn save(&self, mut draft: SettingsDraft) -> AppResult<SettingsViewModel> {
        let mut validation = Self::validate_domain(&draft);
        self.validate_catalog(&draft, &mut validation);
        if !validation.valid {
            return Err(AppError::field(
                "CONFIG_INVALID",
                "设置存在字段错误。",
                validation.field_errors,
            ));
        }
        let expected = draft.expected_revision.unwrap_or(0);
        draft.expected_revision = Some(expected.saturating_add(1));
        let value =
            serialize_settings(&draft).map_err(|error| json_error("设置序列化失败", error))?;
        infra(self.store.save_settings(expected, &value))?;
        self.view()
    }

    async fn restore(&self, settings: SettingsViewModel) -> AppResult<SettingsViewModel> {
        let (_, current_revision) = self.load_stored()?;
        let mut restored = settings.stored;
        restored.expected_revision = Some(current_revision.saturating_add(1));
        let value = serialize_settings(&restored)
            .map_err(|error| json_error("设置回滚序列化失败", error))?;
        infra(self.store.save_settings(current_revision, &value))?;
        *self.effective.write() = settings.effective;
        self.view()
    }

    async fn apply_effective(&self, settings: SettingsDraft) -> AppResult<SettingsViewModel> {
        *self.effective.write() = Some(settings);
        self.view()
    }

    async fn clear_effective(&self) -> AppResult<SettingsViewModel> {
        *self.effective.write() = None;
        self.view()
    }
}
