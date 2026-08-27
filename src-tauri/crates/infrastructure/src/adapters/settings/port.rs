use async_trait::async_trait;

use super::{
    AppError, AppResult, SettingsDraft, SettingsRepositoryAdapter, SettingsRepositoryPort,
    SettingsValidationViewModel, SettingsViewModel, app_error, json_error, serialize_settings,
};

#[async_trait]
impl SettingsRepositoryPort for SettingsRepositoryAdapter {
    async fn defaults(&self) -> AppResult<SettingsDraft> {
        Ok(self.defaults.clone())
    }

    async fn get(&self) -> AppResult<SettingsViewModel> {
        self.view().await
    }

    async fn validate(&self, draft: &SettingsDraft) -> AppResult<SettingsValidationViewModel> {
        let mut validation = Self::validate_domain(draft);
        self.validate_catalog(draft, &mut validation);
        if validation.valid {
            Self::validate_ports(draft, &mut validation);
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
        self.executor
            .execute(move |store| store.save_settings(expected, &value))
            .await
            .map_err(app_error)?;
        self.view().await
    }
}
