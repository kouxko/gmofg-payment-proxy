//! 与界面无关的应用用例门面。
//!
//! `Application` 是桌面 UI、未来 TUI/CLI 和无界面测试共同入口。它仅依赖端口 trait，
//! 不知道 Tauri、WebView 或具体数据库；实现按规则、设置、流量、校验分在子模块中。

use std::{collections::BTreeMap, sync::Arc};

use chrono::Utc;

use crate::{
    AndroidControlPort, AppError, AppResult, BreakpointCoordinator, BreakpointValidationPort,
    BuiltinProtocolPackagePort, CertificateOverviewViewModel, CertificateServicePort,
    CertificateValidationViewModel, ChannelPresentationViewModel, EventHub,
    ExternalPackageApplicationPort, FaultServicePort, ListenerCertificateImportPort,
    ListenerRuntimePort, OperationResultViewModel, ProtectedSecretPort,
    ProtocolPackageApplicationServices, ProtocolPackageCompilerPort, ProtocolPackageImportPort,
    ProtocolPackagePortabilityPort, ProtocolPackageStorePort, ProtocolPackageUsageQueryPort,
    RuleRepositoryPort, SessionQueryPort, SettingsRepositoryPort, SettingsViewModel,
    UiEventPayload, WorkspaceRepositoryPort,
};

mod android;
mod application_backup;
mod application_backup_import;
mod application_snapshot;
mod bootstrap;
mod certificate_portability;
mod certificates;
mod configuration;
mod diagnostic_report;
mod diagnostics;
mod environment_candidates;
mod exchange_observations;
pub use exchange_observations::ExchangeObservationQueries;
mod lifecycle;
mod listener_certificates;
mod listeners;
mod protocol_package_portability;
pub use protocol_package_portability::validate_portable_protocol_bindings;
mod protocol_packages;
mod protocol_rule_values;
mod protocol_rules;
mod rule_capabilities;
mod rules;
mod secrets;
mod settings;
pub use protocol_rule_values::parse_protocol_rule_value;
mod traffic;
mod validation;
mod workspaces;

use validation::{normalize_sans, require_confirmation};

/// 全部业务用例的统一入口。
///
/// 调用者应通过公开用例方法操作，不能绕过权限检查、事件发布和事务顺序直接使用端口。
pub struct Application {
    product_name: String,
    capture: Arc<dyn crate::CaptureRepositoryPort>,
    sessions: Arc<dyn SessionQueryPort>,
    breakpoints: Arc<BreakpointCoordinator>,
    breakpoint_validation: Arc<dyn BreakpointValidationPort>,
    rules: Arc<dyn RuleRepositoryPort>,
    faults: Arc<dyn FaultServicePort>,
    certificates: Arc<dyn CertificateServicePort>,
    settings: Arc<dyn SettingsRepositoryPort>,
    workspaces: Arc<dyn WorkspaceRepositoryPort>,
    android: Arc<dyn AndroidControlPort>,
    /// 按设备序列号隔离的完整应用清单缓存。缓存不是运行所有权来源。
    android_package_cache:
        tokio::sync::Mutex<BTreeMap<String, Vec<crate::AndroidPackageViewModel>>>,
    listener_runtime: Arc<dyn ListenerRuntimePort>,
    listener_certificates: Arc<dyn ListenerCertificateImportPort>,
    protocol_package_store: Arc<dyn ProtocolPackageStorePort>,
    protocol_package_compiler: Arc<dyn ProtocolPackageCompilerPort>,
    protocol_package_importer: Arc<dyn ProtocolPackageImportPort>,
    protocol_package_builtin: Arc<dyn BuiltinProtocolPackagePort>,
    protocol_package_usage: Arc<dyn ProtocolPackageUsageQueryPort>,
    protocol_package_portability: Arc<dyn ProtocolPackagePortabilityPort>,
    external_packages: Arc<dyn ExternalPackageApplicationPort>,
    protected_secrets: Arc<dyn ProtectedSecretPort>,
    events: Arc<EventHub>,
    mutation_gate: Arc<ApplicationMutationGate>,
    environment_candidates: crate::environment_configuration::EnvironmentCandidateRegistry,
    environment_baseline_capture: Arc<dyn crate::EnvironmentApplyBaselineCapturePort>,
    environment_identity_allocator: crate::EnvironmentIdentityAllocator,
    environment_apply_lease: Arc<dyn crate::EnvironmentApplyLeasePort>,
    environment_material_preparer: Arc<dyn crate::EnvironmentProtectedMaterialPreparePort>,
    environment_commit: Arc<dyn crate::EnvironmentCommitPort>,
    environment_validator: Arc<dyn crate::EnvironmentValidationLayerPort>,
}

#[derive(Debug, Default)]
pub(crate) struct ApplicationMutationGate(tokio::sync::RwLock<()>);

impl ApplicationMutationGate {
    pub(crate) async fn lock(&self) -> tokio::sync::RwLockWriteGuard<'_, ()> {
        self.0.write().await
    }

    async fn read(&self) -> tokio::sync::RwLockReadGuard<'_, ()> {
        self.0.read().await
    }
}

impl std::fmt::Debug for Application {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("Application")
            .field("product_name", &self.product_name)
            .finish_non_exhaustive()
    }
}

/// 应用门面所需的全部、与 UI 无关的依赖。
///
/// 使用具名字段而不是十几个位置参数，使桌面、TUI、CLI 和无界面测试的装配代码容易
/// 阅读，也避免交换两个同类型依赖。每个具体能力仍由独立端口约束。
pub struct ApplicationDependencies {
    pub capture: Arc<dyn crate::CaptureRepositoryPort>,
    pub sessions: Arc<dyn SessionQueryPort>,
    pub breakpoints: Arc<BreakpointCoordinator>,
    pub breakpoint_validation: Arc<dyn BreakpointValidationPort>,
    pub rules: Arc<dyn RuleRepositoryPort>,
    pub faults: Arc<dyn FaultServicePort>,
    pub certificates: Arc<dyn CertificateServicePort>,
    pub settings: Arc<dyn SettingsRepositoryPort>,
    pub workspaces: Arc<dyn WorkspaceRepositoryPort>,
    pub listener_runtime: Arc<dyn ListenerRuntimePort>,
    pub listener_certificates: Arc<dyn ListenerCertificateImportPort>,
    pub protocol_packages: ProtocolPackageApplicationServices,
    pub events: Arc<EventHub>,
    pub environment_baseline_capture: Arc<dyn crate::EnvironmentApplyBaselineCapturePort>,
    pub environment_identity_allocator: crate::EnvironmentIdentityAllocator,
    pub environment_apply_lease: Arc<dyn crate::EnvironmentApplyLeasePort>,
    pub environment_material_preparer: Arc<dyn crate::EnvironmentProtectedMaterialPreparePort>,
    pub environment_commit: Arc<dyn crate::EnvironmentCommitPort>,
    pub environment_validator: Arc<dyn crate::EnvironmentValidationLayerPort>,
}

/// Application-owned environment configuration ports supplied by an outer composition root.
///
/// Production uses Infrastructure implementations. Tests and embedding hosts may replace the
/// complete group to exercise the same public candidate lifecycle with controlled boundaries;
/// the registry and transition methods remain private to Application.
#[derive(Clone)]
pub struct EnvironmentConfigurationApplicationServices {
    pub baseline_capture: Arc<dyn crate::EnvironmentApplyBaselineCapturePort>,
    pub identity_allocator: crate::EnvironmentIdentityAllocator,
    pub apply_lease: Arc<dyn crate::EnvironmentApplyLeasePort>,
    pub material_preparer: Arc<dyn crate::EnvironmentProtectedMaterialPreparePort>,
    pub commit: Arc<dyn crate::EnvironmentCommitPort>,
    pub validator: Arc<dyn crate::EnvironmentValidationLayerPort>,
}

impl std::fmt::Debug for EnvironmentConfigurationApplicationServices {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("EnvironmentConfigurationApplicationServices")
            .finish_non_exhaustive()
    }
}

impl std::fmt::Debug for ApplicationDependencies {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("ApplicationDependencies")
            .finish_non_exhaustive()
    }
}

impl Application {
    pub fn new(
        product_name: String,
        dependencies: ApplicationDependencies,
        android: Arc<dyn AndroidControlPort>,
        protected_secrets: Arc<dyn ProtectedSecretPort>,
    ) -> Self {
        Self {
            product_name,
            capture: dependencies.capture,
            sessions: dependencies.sessions,
            breakpoints: dependencies.breakpoints,
            breakpoint_validation: dependencies.breakpoint_validation,
            rules: dependencies.rules,
            faults: dependencies.faults,
            certificates: dependencies.certificates,
            settings: dependencies.settings,
            workspaces: dependencies.workspaces,
            android,
            android_package_cache: tokio::sync::Mutex::new(BTreeMap::new()),
            listener_runtime: dependencies.listener_runtime,
            listener_certificates: dependencies.listener_certificates,
            protocol_package_store: dependencies.protocol_packages.store,
            protocol_package_compiler: dependencies.protocol_packages.compiler,
            protocol_package_importer: dependencies.protocol_packages.importer,
            protocol_package_builtin: dependencies.protocol_packages.builtin,
            protocol_package_usage: dependencies.protocol_packages.usage_query,
            protocol_package_portability: dependencies.protocol_packages.portability,
            external_packages: dependencies.protocol_packages.external,
            protected_secrets,
            events: dependencies.events,
            mutation_gate: Arc::new(ApplicationMutationGate::default()),
            environment_candidates:
                crate::environment_configuration::EnvironmentCandidateRegistry::default(),
            environment_baseline_capture: dependencies.environment_baseline_capture,
            environment_identity_allocator: dependencies.environment_identity_allocator,
            environment_apply_lease: dependencies.environment_apply_lease,
            environment_material_preparer: dependencies.environment_material_preparer,
            environment_commit: dependencies.environment_commit,
            environment_validator: dependencies.environment_validator,
        }
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

    async fn ensure_proxy_stopped_for_write(&self) -> AppResult<()> {
        let active_listeners = self.listener_runtime.statuses().await?;
        if !active_listeners.is_empty() {
            return Err(AppError::new(
                "OPERATION_IN_PROGRESS",
                "只有全部 Workspace 代理入口停止后才能变更证书。",
            ));
        }
        Ok(())
    }
}
