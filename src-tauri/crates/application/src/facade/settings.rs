//! Settings validation, persistence, restart, and rollback workflows.
//!
//! Restart is treated as a transaction: save the candidate, start it, record
//! the effective snapshot, and restore the prior settings/runtime on failure.

use super::{
    Application,
    validation::{
        ensure_valid, normalize_certificate_sans, normalize_settings, parse_sans_raw, push_error,
        require_confirmation, validate_settings_locally,
    },
};
use crate::{
    AppError, AppResult, ProxyState, SettingsDraft, SettingsValidationViewModel, SettingsViewModel,
};

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

        let certificate_overview = self.certificates.overview().await?;
        if certificate_overview.can_initialize {
            validation
                .warnings
                .push("证书材料尚未配置；保存设置后请先完成证书配置再启动 Proxy。".into());
            return Ok(validation);
        }

        let certificate_validation = self.certificates.validate().await?;
        for (field, messages) in certificate_validation.field_errors {
            validation
                .field_errors
                .insert(format!("certificates.{field}"), messages);
        }
        let leaf_sans = certificate_overview
            .items
            .iter()
            .find(|item| item.usage.contains("App → Proxy"))
            .map(|item| normalize_certificate_sans(&item.sans));
        if leaf_sans.is_none_or(|sans| {
            draft
                .leaf_sans
                .iter()
                .any(|required| !sans.contains(required))
        }) {
            push_error(
                &mut validation.field_errors,
                "leaf_sans",
                "当前 Proxy 叶子证书 SAN 未覆盖设置中要求的全部地址。",
            );
        }
        validation.valid = validation.field_errors.is_empty();
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
        self.ensure_settings_write_allowed().await?;
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

    pub async fn settings_save_and_restart(
        &self,
        draft: SettingsDraft,
    ) -> AppResult<SettingsViewModel> {
        let _gate = self.mutation_gate.lock().await;
        self.ensure_settings_write_allowed().await?;
        let old_settings = self.settings.get().await?;
        let old_status = self.proxy.status().await?;
        let was_running = old_status.state == ProxyState::Running;
        let saved = self.settings_save_inner(draft).await?;
        if !was_running {
            self.publish_settings(&saved);
            return Ok(saved);
        }

        self.proxy_stop_inner().await?;
        let candidate_status = match self.proxy.start(saved.stored.clone()).await {
            Ok(status) => status,
            Err(error) => {
                return self
                    .rollback_settings_and_recover(old_settings, error, None)
                    .await;
            }
        };
        match self.settings.apply_effective(saved.stored).await {
            Ok(_) => {
                self.publish_runtime(&candidate_status);
                let settings = self.settings.get().await?;
                self.publish_settings(&settings);
                Ok(settings)
            }
            Err(apply_error) => match self.proxy.stop().await {
                Ok(stopped) => {
                    self.publish_runtime(&stopped);
                    let clear_error = self.settings.clear_effective().await.err();
                    self.rollback_settings_and_recover(old_settings, apply_error, clear_error)
                        .await
                }
                Err(stop_error) => {
                    let restore = self.settings.restore(old_settings).await;
                    let restore_text = restore.map_or_else(
                        |error| format!("旧设置恢复失败：{}", error.view_model.message),
                        |_| "旧设置数据库已恢复，但运行状态未恢复".to_owned(),
                    );
                    Err(AppError::new(
                        "CONFIG_ROLLBACK_FAILED",
                        format!(
                            "候选 Proxy 已启动，但生效设置记录失败且无法停止；原始错误 [{}] {}；停止错误 [{}] {}；{restore_text}。",
                            apply_error.view_model.code,
                            apply_error.view_model.message,
                            stop_error.view_model.code,
                            stop_error.view_model.message
                        ),
                    )
                    .retryable("请立即检查 Proxy 实际监听状态，停止后再恢复配置。"))
                }
            },
        }
    }

    pub async fn settings_save_and_restart_input(
        &self,
        mut draft: SettingsDraft,
        leaf_sans_raw: String,
    ) -> AppResult<SettingsViewModel> {
        draft.leaf_sans = parse_sans_raw(&leaf_sans_raw);
        self.settings_save_and_restart(draft).await
    }

    pub async fn settings_reset_defaults(&self, confirmed: bool) -> AppResult<SettingsDraft> {
        require_confirmation(confirmed, "恢复默认设置需要确认。")?;
        self.ensure_settings_write_allowed().await?;
        self.settings.defaults().await
    }

    async fn rollback_settings_and_recover(
        &self,
        old_settings: SettingsViewModel,
        mut candidate_error: AppError,
        cleanup_error: Option<AppError>,
    ) -> AppResult<SettingsViewModel> {
        let restored = self
            .settings
            .restore(old_settings.clone())
            .await
            .map_err(|error| {
                AppError::new(
                    "CONFIG_ROLLBACK_FAILED",
                    format!(
                        "候选设置失败 [{}] {}；旧设置数据库恢复失败 [{}] {}。",
                        candidate_error.view_model.code,
                        candidate_error.view_model.message,
                        error.view_model.code,
                        error.view_model.message
                    ),
                )
                .retryable("Proxy 当前保持停止；请检查设置存储后手动恢复。")
            })?;
        let recovery_settings = old_settings
            .effective
            .unwrap_or_else(|| restored.stored.clone());
        let recovery_status =
            self.proxy
                .start(recovery_settings.clone())
                .await
                .map_err(|error| {
                    AppError::new(
                        "CONFIG_ROLLBACK_FAILED",
                        format!(
                            "候选设置失败 [{}] {}；旧设置已恢复，但旧 Proxy 启动失败 [{}] {}。",
                            candidate_error.view_model.code,
                            candidate_error.view_model.message,
                            error.view_model.code,
                            error.view_model.message
                        ),
                    )
                    .retryable("请检查旧配置的端口和证书；Proxy 当前未恢复运行。")
                })?;
        if let Err(error) = self.settings.apply_effective(recovery_settings).await {
            let cleanup = self
                .cleanup_failed_start(error.clone(), "旧 Proxy 恢复后无法记录生效设置")
                .await;
            return Err(AppError::new(
                "CONFIG_ROLLBACK_FAILED",
                format!(
                    "候选设置失败 [{}] {}；旧 Proxy 虽已启动，但恢复事务未完成：{}。",
                    candidate_error.view_model.code,
                    candidate_error.view_model.message,
                    cleanup.view_model.message
                ),
            )
            .retryable("请检查 Proxy 实际状态和设置存储。"));
        }
        self.publish_runtime(&recovery_status);
        let cleanup_note = cleanup_error.map_or_else(String::new, |error| {
            format!(
                "；候选清理附加错误 [{}] {}",
                error.view_model.code, error.view_model.message
            )
        });
        candidate_error.view_model.message = format!(
            "新设置未生效，旧设置和运行状态已恢复：{}{}。",
            candidate_error.view_model.message, cleanup_note
        );
        candidate_error.view_model.retryable = true;
        candidate_error.view_model.suggested_action = Some("请按原错误码检查新设置后重试。".into());
        Err(candidate_error)
    }
}
