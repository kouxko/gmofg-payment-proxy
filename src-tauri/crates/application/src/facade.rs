use std::sync::Arc;

use chrono::Utc;

use crate::{
    AppBootstrapViewModel, AppError, AppResult, BreakpointCoordinator, BreakpointValidationPort,
    CaptureQuery, CertificateOverviewViewModel, CertificateServicePort,
    CertificateValidationViewModel, ChannelPresentationViewModel, EventHub, EventSubscription,
    FaultServicePort, FileExportPort, OperationResultViewModel, PageRequest, ProxyState,
    ProxyStatusViewModel, ProxySupervisorPort, RuleRepositoryPort, SessionQueryPort,
    SettingsRepositoryPort, SettingsViewModel, UiEventEnvelope, UiEventPayload,
};

mod rules;
mod settings;
mod traffic;
mod validation;

use validation::{normalize_sans, require_confirmation};

#[derive(Debug)]
pub struct Application {
    product_name: String,
    proxy: Arc<dyn ProxySupervisorPort>,
    capture: Arc<dyn crate::CaptureRepositoryPort>,
    sessions: Arc<dyn SessionQueryPort>,
    breakpoints: Arc<BreakpointCoordinator>,
    breakpoint_validation: Arc<dyn BreakpointValidationPort>,
    rules: Arc<dyn RuleRepositoryPort>,
    faults: Arc<dyn FaultServicePort>,
    certificates: Arc<dyn CertificateServicePort>,
    settings: Arc<dyn SettingsRepositoryPort>,
    file_export: Arc<dyn FileExportPort>,
    events: Arc<EventHub>,
    mutation_gate: tokio::sync::Mutex<()>,
}

/// All UI-neutral ports required by the application use-case facade.
///
/// Keeping the dependency graph in one named value makes host composition
/// readable for desktop, TUI, CLI, and headless callers without weakening the
/// individual port boundaries.
#[derive(Debug)]
pub struct ApplicationDependencies {
    pub proxy: Arc<dyn ProxySupervisorPort>,
    pub capture: Arc<dyn crate::CaptureRepositoryPort>,
    pub sessions: Arc<dyn SessionQueryPort>,
    pub breakpoints: Arc<BreakpointCoordinator>,
    pub breakpoint_validation: Arc<dyn BreakpointValidationPort>,
    pub rules: Arc<dyn RuleRepositoryPort>,
    pub faults: Arc<dyn FaultServicePort>,
    pub certificates: Arc<dyn CertificateServicePort>,
    pub settings: Arc<dyn SettingsRepositoryPort>,
    pub file_export: Arc<dyn FileExportPort>,
    pub events: Arc<EventHub>,
}

impl Application {
    pub fn new(product_name: String, dependencies: ApplicationDependencies) -> Self {
        Self {
            product_name,
            proxy: dependencies.proxy,
            capture: dependencies.capture,
            sessions: dependencies.sessions,
            breakpoints: dependencies.breakpoints,
            breakpoint_validation: dependencies.breakpoint_validation,
            rules: dependencies.rules,
            faults: dependencies.faults,
            certificates: dependencies.certificates,
            settings: dependencies.settings,
            file_export: dependencies.file_export,
            events: dependencies.events,
            mutation_gate: tokio::sync::Mutex::new(()),
        }
    }

    pub async fn app_bootstrap(&self) -> AppResult<AppBootstrapViewModel> {
        let proxy = self.proxy.status().await?;
        let recent_capture = self
            .capture_query(CaptureQuery {
                keyword: None,
                terminal_ip: None,
                channel: None,
                stage: None,
                result: None,
                rule_id: None,
                after_event_id: None,
                sort: crate::CaptureSort::OccurredAt,
                direction: crate::SortDirection::Desc,
                page: PageRequest {
                    page: 1,
                    page_size: 5,
                },
            })
            .await?;
        let pending_breakpoints = self
            .breakpoints
            .query(proxy.runtime_epoch)
            .into_iter()
            .collect();
        let certificate = self.certificates.overview().await?;
        let settings = self.settings.get().await?;
        let channel_catalog = settings
            .stored
            .channels
            .iter()
            .map(|channel| ChannelPresentationViewModel {
                id: channel.id.clone(),
                display_name: channel.display_name.clone(),
            })
            .collect();
        Ok(AppBootstrapViewModel {
            product_name: self.product_name.clone(),
            proxy,
            channel_catalog,
            recent_capture,
            pending_breakpoints,
            certificate,
            settings,
            event_cursor: self.events.current_cursor(),
        })
    }

    pub fn app_subscribe_events(&self, after_event_id: u64) -> AppResult<EventSubscription> {
        self.events.subscribe_default(after_event_id)
    }

    pub fn app_take_subscription_failure(&self, subscription_id: u64) -> Option<UiEventEnvelope> {
        self.events.take_subscription_failure(subscription_id)
    }

    pub fn app_unsubscribe_events(&self, subscription_id: u64) -> OperationResultViewModel {
        self.events.unsubscribe(subscription_id);
        OperationResultViewModel::success("实时事件订阅已取消。")
    }

    /// Gracefully stops the complete runtime during application exit.
    ///
    /// This bypasses the interactive lifecycle guard so an in-flight start or
    /// stop operation is awaited and then fully cleaned up by the supervisor.
    pub async fn app_shutdown(&self) -> AppResult<ProxyStatusViewModel> {
        let _gate = self.mutation_gate.lock().await;
        let before = self.proxy.status().await?;
        let stop_result = if before.state == ProxyState::Stopped {
            Ok(before.clone())
        } else {
            self.proxy.stop().await
        };
        if let Some(epoch) = before.runtime_epoch {
            for summary in self.breakpoints.proxy_stopped(epoch) {
                self.events.publish(
                    Some(epoch),
                    Utc::now(),
                    Some(summary.breakpoint_id.to_string()),
                    Some(summary.revision),
                    UiEventPayload::BreakpointResolved(summary),
                );
            }
        }
        let clear_result = self.settings.clear_effective().await;
        match (stop_result, clear_result) {
            (Ok(status), Ok(_)) => {
                self.publish_runtime(&status);
                Ok(status)
            }
            (Err(stop_error), Ok(_)) => Err(stop_error),
            (Ok(status), Err(clear_error)) => {
                self.publish_runtime(&status);
                Err(clear_error)
            }
            (Err(stop_error), Err(clear_error)) => Err(AppError::new(
                "APP_SHUTDOWN_FAILED",
                format!(
                    "Proxy 停止失败 [{}] {}；生效设置清理失败 [{}] {}。",
                    stop_error.view_model.code,
                    stop_error.view_model.message,
                    clear_error.view_model.code,
                    clear_error.view_model.message
                ),
            )),
        }
    }

    pub async fn proxy_get_status(&self) -> AppResult<ProxyStatusViewModel> {
        self.proxy.status().await
    }

    pub async fn proxy_start(&self) -> AppResult<ProxyStatusViewModel> {
        let _gate = self.mutation_gate.lock().await;
        self.proxy_start_inner().await
    }

    async fn proxy_start_inner(&self) -> AppResult<ProxyStatusViewModel> {
        let current = self.proxy.status().await?;
        match current.state {
            ProxyState::Running => {
                return Err(AppError::new("PROXY_ALREADY_RUNNING", "Proxy 已在运行。"));
            }
            ProxyState::Starting | ProxyState::Stopping => {
                return Err(AppError::new(
                    "OPERATION_IN_PROGRESS",
                    "Proxy 正在启动或停止，请等待当前操作完成。",
                ));
            }
            ProxyState::Stopped | ProxyState::Faulted => {}
        }
        let settings = self.settings.get().await?.stored;
        let status = self.proxy.start(settings.clone()).await?;
        if let Err(error) = self.settings.apply_effective(settings).await {
            return Err(self
                .cleanup_failed_start(error, "Proxy 启动后无法记录生效设置")
                .await);
        }
        self.publish_runtime(&status);
        Ok(status)
    }

    pub async fn proxy_stop(&self) -> AppResult<ProxyStatusViewModel> {
        let _gate = self.mutation_gate.lock().await;
        self.proxy_stop_inner().await
    }

    async fn proxy_stop_inner(&self) -> AppResult<ProxyStatusViewModel> {
        let current = self.proxy.status().await?;
        if current.state == ProxyState::Stopped {
            return Ok(current);
        }
        if matches!(current.state, ProxyState::Starting | ProxyState::Stopping) {
            return Err(AppError::new(
                "OPERATION_IN_PROGRESS",
                "Proxy 正在启动或停止，请等待当前操作完成。",
            ));
        }
        let status = self.proxy.stop().await?;
        let clear_effective_result = self.settings.clear_effective().await;
        if let Some(epoch) = current.runtime_epoch {
            for summary in self.breakpoints.proxy_stopped(epoch) {
                self.events.publish(
                    Some(epoch),
                    Utc::now(),
                    Some(summary.breakpoint_id.to_string()),
                    Some(summary.revision),
                    UiEventPayload::BreakpointResolved(summary),
                );
            }
        }
        self.publish_runtime(&status);
        clear_effective_result?;
        Ok(status)
    }

    pub async fn proxy_restart(&self) -> AppResult<ProxyStatusViewModel> {
        let _gate = self.mutation_gate.lock().await;
        let current = self.proxy.status().await?;
        if matches!(current.state, ProxyState::Starting | ProxyState::Stopping) {
            return Err(AppError::new(
                "OPERATION_IN_PROGRESS",
                "Proxy 正在启动或停止，请等待当前操作完成。",
            ));
        }
        let settings = self.settings.get().await?;
        let restart_settings = if current.state == ProxyState::Running {
            settings.effective.unwrap_or(settings.stored)
        } else {
            settings.stored
        };
        if current.state != ProxyState::Stopped {
            self.proxy_stop_inner().await?;
        }
        let status = self.proxy.start(restart_settings.clone()).await?;
        if let Err(error) = self.settings.apply_effective(restart_settings).await {
            return Err(self
                .cleanup_failed_start(error, "Proxy 重启后无法记录生效设置")
                .await);
        }
        self.publish_runtime(&status);
        Ok(status)
    }

    pub async fn certificate_overview(&self) -> AppResult<CertificateOverviewViewModel> {
        self.certificates.overview().await
    }

    pub async fn certificate_generate_ca(
        &self,
        sans: Vec<String>,
    ) -> AppResult<CertificateOverviewViewModel> {
        let _gate = self.mutation_gate.lock().await;
        self.ensure_proxy_stopped_for_write().await?;
        let overview = self.certificates.generate_ca(normalize_sans(sans)).await?;
        self.publish_certificate(&overview);
        Ok(overview)
    }

    pub async fn certificate_export_ca(&self) -> AppResult<OperationResultViewModel> {
        self.certificates.export_ca().await
    }

    pub async fn certificate_reissue_leaf(
        &self,
        expected_revision: u64,
        sans: Vec<String>,
    ) -> AppResult<CertificateOverviewViewModel> {
        let _gate = self.mutation_gate.lock().await;
        self.ensure_proxy_stopped_for_write().await?;
        let overview = self
            .certificates
            .reissue_leaf(expected_revision, normalize_sans(sans))
            .await?;
        self.publish_certificate(&overview);
        Ok(overview)
    }

    pub async fn certificate_import_pkcs12(
        &self,
        password: String,
    ) -> AppResult<CertificateOverviewViewModel> {
        let _gate = self.mutation_gate.lock().await;
        self.ensure_proxy_stopped_for_write().await?;
        let overview = self.certificates.import_pkcs12(password).await?;
        self.publish_certificate(&overview);
        Ok(overview)
    }

    pub async fn certificate_import_upstream_ca(&self) -> AppResult<CertificateOverviewViewModel> {
        let _gate = self.mutation_gate.lock().await;
        self.ensure_proxy_stopped_for_write().await?;
        let overview = self.certificates.import_upstream_ca().await?;
        self.publish_certificate(&overview);
        Ok(overview)
    }

    pub async fn certificate_validate(&self) -> AppResult<CertificateValidationViewModel> {
        self.certificates.validate().await
    }

    pub async fn certificate_reset_ca(
        &self,
        expected_revision: u64,
        confirmed: bool,
    ) -> AppResult<CertificateOverviewViewModel> {
        require_confirmation(confirmed, "重新初始化会替换本机服务端私钥和叶子证书。")?;
        let _gate = self.mutation_gate.lock().await;
        self.ensure_proxy_stopped_for_write().await?;
        let overview = self.certificates.reset_ca(expected_revision).await?;
        self.publish_certificate(&overview);
        Ok(overview)
    }

    fn publish_runtime(&self, status: &ProxyStatusViewModel) {
        self.events.publish(
            status.runtime_epoch,
            Utc::now(),
            None,
            Some(status.revision),
            UiEventPayload::RuntimeStatusChanged(Box::new(status.clone())),
        );
    }

    fn publish_certificate(&self, overview: &CertificateOverviewViewModel) {
        self.events.publish(
            None,
            Utc::now(),
            Some("certificates".into()),
            Some(overview.revision),
            UiEventPayload::CertificateStatusChanged(overview.clone()),
        );
    }

    fn publish_settings(&self, settings: &SettingsViewModel) {
        self.events.publish(
            None,
            Utc::now(),
            Some("settings".into()),
            Some(settings.revision),
            UiEventPayload::SettingsChanged(Box::new(settings.clone())),
        );
    }

    async fn cleanup_failed_start(&self, primary: AppError, context: &str) -> AppError {
        let stop = self.proxy.stop().await;
        let clear = if stop.is_ok() {
            self.settings.clear_effective().await.map(|_| ())
        } else {
            Ok(())
        };
        match (stop, clear) {
            (Ok(status), Ok(())) => {
                self.publish_runtime(&status);
                primary
            }
            (stop, clear) => {
                let stop_message = stop
                    .err()
                    .map_or_else(|| "停止成功".to_owned(), |error| error.view_model.message);
                let clear_message = clear.err().map_or_else(
                    || "生效快照清理成功或未执行".to_owned(),
                    |error| error.view_model.message,
                );
                AppError::new(
                    "PROXY_START_CLEANUP_FAILED",
                    format!(
                        "{context}：{}；清理结果：{stop_message}；{clear_message}。",
                        primary.view_model.message
                    ),
                )
                .retryable("请先确认 Proxy 实际状态，再停止 Proxy 并检查设置存储。")
            }
        }
    }

    async fn ensure_proxy_stopped_for_write(&self) -> AppResult<()> {
        let state = self.proxy.status().await?.state;
        if state != ProxyState::Stopped {
            return Err(AppError::new(
                "OPERATION_IN_PROGRESS",
                "只有 Proxy 已停止时才能变更证书。",
            ));
        }
        Ok(())
    }

    async fn ensure_settings_write_allowed(&self) -> AppResult<()> {
        let state = self.proxy.status().await?.state;
        if matches!(state, ProxyState::Starting | ProxyState::Stopping) {
            return Err(AppError::new(
                "OPERATION_IN_PROGRESS",
                "Proxy 正在启动或停止，暂时不能修改设置。",
            ));
        }
        Ok(())
    }

    async fn ensure_rule_or_fault_write_allowed(&self) -> AppResult<()> {
        let state = self.proxy.status().await?.state;
        if matches!(state, ProxyState::Starting | ProxyState::Stopping) {
            return Err(AppError::new(
                "OPERATION_IN_PROGRESS",
                "Proxy 正在启动或停止，暂时不能修改规则或故障配置。",
            ));
        }
        Ok(())
    }
}
