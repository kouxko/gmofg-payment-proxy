//! 与界面无关的应用用例门面。
//!
//! `Application` 是桌面 UI、未来 TUI/CLI 和无界面测试共同入口。它仅依赖端口 trait，
//! 不知道 Tauri、WebView 或具体数据库；实现按规则、设置、流量、校验分在子模块中。

use std::sync::Arc;

use chrono::Utc;

use crate::{
    AndroidControlPort, AppBootstrapViewModel, AppError, AppResult,
    ApplicationConfigurationStorePort, BreakpointCoordinator, BreakpointValidationPort,
    CaptureQuery, CertificateOverviewViewModel, CertificateServicePort,
    CertificateValidationViewModel, ChannelPresentationViewModel, EventHub, EventSubscription,
    FaultServicePort, FileExportPort, ListenerCertificateImportPort, ListenerRuntimePort,
    ListenerRuntimeState, OperationResultViewModel, PageRequest, ProtectedSecretPort, ProxyState,
    ProxyStatusViewModel, ProxySupervisorPort, RuleRepositoryPort, SessionQueryPort,
    SettingsRepositoryPort, SettingsViewModel, UiEventEnvelope, UiEventPayload,
    UnavailableApplicationConfigurationStore, WorkspaceDocumentPort, WorkspaceRepositoryPort,
};

mod android;
mod bootstrap;
mod certificate_portability;
mod certificates;
mod configuration;
mod diagnostics;
mod listener_certificates;
mod listeners;
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
    file_export: Arc<dyn FileExportPort>,
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
    pub file_export: Arc<dyn FileExportPort>,
    pub workspaces: Arc<dyn WorkspaceRepositoryPort>,
    pub workspace_documents: Arc<dyn WorkspaceDocumentPort>,
    pub listener_runtime: Arc<dyn ListenerRuntimePort>,
    pub listener_certificates: Arc<dyn ListenerCertificateImportPort>,
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
            file_export: dependencies.file_export,
            workspaces: dependencies.workspaces,
            workspace_documents: dependencies.workspace_documents,
            configuration_store,
            android,
            android_package_cache: tokio::sync::Mutex::new(None),
            listener_runtime: dependencies.listener_runtime,
            listener_certificates: dependencies.listener_certificates,
            protected_secrets,
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
        // 动态入口各自拥有运行 epoch；启动快照必须聚合全部待处理断点，不能再以已退役
        // 的单实例代理 epoch 过滤，否则界面会漏掉真实入口产生的断点。
        let pending_breakpoints = self.breakpoints.query(None).into_iter().collect();
        // 启动快照只读取证书的非敏感元数据。不能为了画状态栏就解密私钥并触发
        // Keychain/DPAPI 授权，否则用户取消系统提示会让整个展示层无法启动。
        let certificate = self.certificates.status().await?;
        let settings = self.settings.get().await?;
        // 规则和故障动作的通道必须引用当前 Workspace 的 Listener UUID。
        // 旧产品设置中的静态通道只服务于兼容状态展示，不能再作为可提交的配置来源；
        // 否则 UI 会生成领域层必然拒绝、且永远无法命中动态 Listener 的规则。
        let channel_catalog = self.selected_workspace_channel_catalog().await?;
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

    /// Stops the runtime on exit after waiting for any in-flight mutation.
    pub async fn app_shutdown(&self) -> AppResult<ProxyStatusViewModel> {
        let _gate = self.mutation_gate.lock().await;
        self.app_shutdown_inner().await
    }

    async fn app_shutdown_inner(&self) -> AppResult<ProxyStatusViewModel> {
        let mut listener_cleanup_errors = Vec::new();
        match self.listener_runtime.statuses().await {
            Ok(statuses) => {
                for status in statuses {
                    if status.state == ListenerRuntimeState::Stopped {
                        continue;
                    }
                    if let Err(error) = self.listener_runtime.stop(status.listener_id).await {
                        listener_cleanup_errors.push(format!(
                            "入口 {} 停止失败 [{}] {}",
                            status.listener_id, error.view_model.code, error.view_model.message
                        ));
                    }
                }
            }
            Err(error) => listener_cleanup_errors.push(format!(
                "入口状态读取失败 [{}] {}",
                error.view_model.code, error.view_model.message
            )),
        }
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
        let legacy_result = match (stop_result, clear_result) {
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
        };
        if listener_cleanup_errors.is_empty() {
            legacy_result
        } else {
            let listener_detail = listener_cleanup_errors.join("；");
            match legacy_result {
                Ok(_) => Err(AppError::new(
                    "APP_SHUTDOWN_FAILED",
                    format!("动态代理入口清理失败：{listener_detail}。"),
                )),
                Err(error) => Err(AppError::new(
                    "APP_SHUTDOWN_FAILED",
                    format!(
                        "动态代理入口清理失败：{listener_detail}；其他退出清理失败 [{}] {}。",
                        error.view_model.code, error.view_model.message
                    ),
                )),
            }
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
