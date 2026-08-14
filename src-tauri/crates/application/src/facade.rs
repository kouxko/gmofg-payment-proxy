//! 与界面无关的应用用例门面。
//!
//! `Application` 是桌面 UI、未来 TUI/CLI 和无界面测试共同入口。它仅依赖端口 trait，
//! 不知道 Tauri、WebView 或具体数据库；实现按规则、设置、流量、校验分在子模块中。

use std::sync::Arc;

use chrono::Utc;

use crate::{
    AndroidControlPort, AppError, AppResult, ApplicationConfigurationStorePort,
    BreakpointCoordinator, BreakpointValidationPort, CertificateOverviewViewModel,
    CertificateServicePort, CertificateValidationViewModel, ChannelPresentationViewModel, EventHub,
    FaultServicePort, ListenerCertificateImportPort, ListenerRuntimePort, OperationResultViewModel,
    ProtectedSecretPort, ProtocolPackageApplicationServices, ProtocolPackageCompilerPort,
    ProtocolPackageStorePort, ProtocolPackageUsageQueryPort, ProxyState, ProxyStatusViewModel,
    ProxySupervisorPort, RuleRepositoryPort, SessionQueryPort, SettingsRepositoryPort,
    SettingsViewModel, UiEventPayload, UnavailableApplicationConfigurationStore,
    WorkspaceDocumentPort, WorkspaceRepositoryPort,
};

mod android;
mod bootstrap;
mod certificate_portability;
mod certificates;
mod configuration;
mod diagnostics;
mod lifecycle;
mod listener_certificates;
mod listeners;
mod protocol_packages;
mod rules;
mod secrets;
mod settings;
mod traffic;
mod validation;
mod workspaces;

use validation::{normalize_sans, require_confirmation};

#[derive(Debug)]
/// 全部业务用例的统一入口。
///
/// 调用者应通过公开用例方法操作，不能绕过权限检查、事件发布和事务顺序直接使用端口。
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
    workspaces: Arc<dyn WorkspaceRepositoryPort>,
    workspace_documents: Arc<dyn WorkspaceDocumentPort>,
    configuration_store: Arc<dyn ApplicationConfigurationStorePort>,
    android: Arc<dyn AndroidControlPort>,
    /// 当前已选择设备的完整应用清单。
    ///
    /// Android 包清单按设备选择缓存；显式刷新与启动校验会重新读取。
    /// 因此首次读取后由 Rust 应用层缓存；切换（或重新选择）设备时立即失效。
    /// UI、未来 CLI/TUI 都只能通过同一组用例读取和筛选，不能各自维护业务缓存。
    android_package_cache: tokio::sync::Mutex<Option<Vec<crate::AndroidPackageViewModel>>>,
    listener_runtime: Arc<dyn ListenerRuntimePort>,
    listener_certificates: Arc<dyn ListenerCertificateImportPort>,
    protocol_package_store: Arc<dyn ProtocolPackageStorePort>,
    protocol_package_compiler: Arc<dyn ProtocolPackageCompilerPort>,
    protocol_package_usage: Arc<dyn ProtocolPackageUsageQueryPort>,
    protected_secrets: Arc<dyn ProtectedSecretPort>,
    events: Arc<EventHub>,
    mutation_gate: tokio::sync::Mutex<()>,
}

/// 应用门面所需的全部、与 UI 无关的依赖。
///
/// 使用具名字段而不是十几个位置参数，使桌面、TUI、CLI 和无界面测试的装配代码容易
/// 阅读，也避免交换两个同类型依赖。每个具体能力仍由独立端口约束。
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
    pub workspaces: Arc<dyn WorkspaceRepositoryPort>,
    pub workspace_documents: Arc<dyn WorkspaceDocumentPort>,
    pub listener_runtime: Arc<dyn ListenerRuntimePort>,
    pub listener_certificates: Arc<dyn ListenerCertificateImportPort>,
    pub protocol_packages: ProtocolPackageApplicationServices,
    pub events: Arc<EventHub>,
}

impl Application {
    pub fn new(product_name: String, dependencies: ApplicationDependencies) -> Self {
        Self::new_with_android(
            product_name,
            dependencies,
            Arc::new(crate::UnavailableAndroidControlPort),
        )
    }

    pub fn new_with_android(
        product_name: String,
        dependencies: ApplicationDependencies,
        android: Arc<dyn AndroidControlPort>,
    ) -> Self {
        Self::new_with_android_and_secrets(
            product_name,
            dependencies,
            android,
            Arc::new(crate::UnavailableProtectedSecretPort),
        )
    }

    /// 生产宿主使用的完整构造器；测试仍可沿用 `new_with_android` 获得显式不可用端口。
    pub fn new_with_android_and_secrets(
        product_name: String,
        dependencies: ApplicationDependencies,
        android: Arc<dyn AndroidControlPort>,
        protected_secrets: Arc<dyn ProtectedSecretPort>,
    ) -> Self {
        Self::new_with_platform_services(
            product_name,
            dependencies,
            android,
            protected_secrets,
            Arc::new(UnavailableApplicationConfigurationStore),
        )
    }

    pub fn new_with_platform_services(
        product_name: String,
        dependencies: ApplicationDependencies,
        android: Arc<dyn AndroidControlPort>,
        protected_secrets: Arc<dyn ProtectedSecretPort>,
        configuration_store: Arc<dyn ApplicationConfigurationStorePort>,
    ) -> Self {
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
            workspaces: dependencies.workspaces,
            workspace_documents: dependencies.workspace_documents,
            configuration_store,
            android,
            android_package_cache: tokio::sync::Mutex::new(None),
            listener_runtime: dependencies.listener_runtime,
            listener_certificates: dependencies.listener_certificates,
            protocol_package_store: dependencies.protocol_packages.store,
            protocol_package_compiler: dependencies.protocol_packages.compiler,
            protocol_package_usage: dependencies.protocol_packages.usage_query,
            protected_secrets,
            events: dependencies.events,
            mutation_gate: tokio::sync::Mutex::new(()),
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
        let active_listeners = self.listener_runtime.statuses().await?;
        // `proxy` 只剩测试/嵌入兼容端口；生产 Host 注入的退役适配器永远为 Stopped。
        // 同时检查它可以保留旧嵌入方的安全契约，而真实桌面安全判断以动态入口为准。
        let compatibility_state = self.proxy.status().await?.state;
        if !active_listeners.is_empty() || compatibility_state != ProxyState::Stopped {
            return Err(AppError::new(
                "OPERATION_IN_PROGRESS",
                "只有全部 Workspace 代理入口停止后才能变更证书。",
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
