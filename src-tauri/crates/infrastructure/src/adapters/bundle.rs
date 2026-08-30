//! 基础设施服务的统一装配包。
//!
//! 它共享数据库、对话框、事件中心等长生命周期资源，并确保各适配器拿到同一份状态；
//! 构造失败会整体返回，避免应用只启动一半服务。

use std::{
    collections::BTreeMap,
    net::{IpAddr, SocketAddr},
    path::PathBuf,
    sync::Arc,
    time::Duration,
};

use intercept_proxy_application::{
    AppError, AppResult, Application, ApplicationDependencies, BreakpointCoordinator,
    BreakpointValidator, CapacityLedger, CertificateServicePort, EventHub, InMemorySessionStore,
    ProtocolPackageApplicationServices, SettingsRepositoryPort, WorkspaceRepositoryPort,
};
use intercept_proxy_product_api::ProductProfile;

use crate::{InfrastructureError, SecretProtector, SqliteExecutor, SqliteStore};

use super::{
    AndroidAdbAdapter, CaptureRepositoryAdapter, CertificateServiceAdapter,
    EnvironmentApplyLeaseAdapter, EnvironmentApplyRuntimeAdapter,
    EnvironmentConfigurationMaterialPreparer, EnvironmentConfigurationValidationAdapter,
    ExternalPackageRegistryAdapter, ExternalPackageServer, ExternalPackageServerConfig,
    FaultServiceAdapter, HeaderBodyCodecResolver, ListenerRuntimeAdapter, LocalPackageSupervisor,
    ManagedListenerCertificateAdapter, NativeFileDialog, PackageTransportConfig,
    ProtectedSecretAdapter, ProtocolPackageImportAdapter, ProtocolPackageRepositoryAdapter,
    ProtocolPackageUsageQueryAdapter, RuleRepositoryAdapter, RuntimePipelineAdapter,
    RuntimePipelineProductHooks, SettingsRepositoryAdapter, WorkspaceBodyCodecResolver,
    WorkspaceRepositoryAdapter,
};

#[derive(Debug)]
pub struct InfrastructureServiceBundle {
    store: Arc<SqliteStore>,
    sqlite: SqliteExecutor,
    settings: Arc<SettingsRepositoryAdapter>,
    workspaces: Arc<WorkspaceRepositoryAdapter>,
    workspace_body_codecs: Arc<WorkspaceBodyCodecResolver>,
    listener_runtime: Arc<ListenerRuntimeAdapter>,
    listener_certificates: Arc<ManagedListenerCertificateAdapter>,
    protected_secrets: Arc<ProtectedSecretAdapter>,
    /// 应用级协议包文件、启用位和可重建编译缓存；生命周期约束由 T14 Application 用例接管。
    protocol_packages: Arc<ProtocolPackageRepositoryAdapter>,
    /// 原生文件选择与有界 ZIP 读取；路径和 ZIP 字节不越过 Application 端口。
    protocol_package_import: Arc<ProtocolPackageImportAdapter>,
    /// 汇总全部 Workspace 的精确引用，并与 Listener 运行态合并。
    protocol_package_usage: Arc<ProtocolPackageUsageQueryAdapter>,
    /// 外部软件包持久化元数据、在线连接与服务状态的唯一注册表。
    external_packages: Arc<ExternalPackageRegistryAdapter>,
    rules: Arc<RuleRepositoryAdapter>,
    faults: Arc<FaultServiceAdapter>,
    certificates: Arc<CertificateServiceAdapter>,
    capture: Arc<CaptureRepositoryAdapter>,
    sessions: Arc<InMemorySessionStore>,
    environment_apply_resource_gates: Arc<super::EnvironmentApplyResourceGateRegistry>,
    environment_material_preparer: Arc<EnvironmentConfigurationMaterialPreparer>,
    environment_commit: Arc<crate::sqlite::EnvironmentConfigurationCommitAdapter>,
    environment_validator: Arc<EnvironmentConfigurationValidationAdapter>,
}

impl InfrastructureServiceBundle {
    #[must_use]
    #[expect(
        clippy::too_many_lines,
        reason = "the composition root keeps construction order explicit and reviewable"
    )]
    pub fn new(
        persistence: impl crate::IntoSqlitePersistence,
        protector: Arc<dyn SecretProtector>,
        dialog: &Arc<dyn NativeFileDialog>,
        product: Arc<dyn ProductProfile>,
        capacity: &Arc<CapacityLedger>,
        builtin_protocol_package: Option<Arc<[u8]>>,
    ) -> Self {
        let (sqlite, store) = persistence.into_sqlite_persistence();
        let environment_apply_resource_gates =
            Arc::new(super::EnvironmentApplyResourceGateRegistry::default());
        let prepared_material_arena = Arc::new(super::PreparedMaterialArena::default());
        let environment_material_preparer =
            Arc::new(EnvironmentConfigurationMaterialPreparer::new(
                Arc::clone(&protector),
                prepared_material_arena.clone(),
            ));
        let environment_commit = Arc::new(
            crate::sqlite::EnvironmentConfigurationCommitAdapter::new(sqlite.clone()),
        );
        let body_codec = product.body_codec();
        let sessions = Arc::new(InMemorySessionStore::with_capacity_ledger(
            InMemorySessionStore::DEFAULT_MAX_SESSIONS,
            Arc::clone(capacity),
        ));
        let capture = Arc::new(CaptureRepositoryAdapter::new(sessions.clone()));
        let rules = Arc::new(RuleRepositoryAdapter::new(
            (sqlite.clone(), Arc::clone(&store)),
            Arc::clone(dialog),
            product.channels(),
        ));
        let settings = Arc::new(SettingsRepositoryAdapter::new(
            sqlite.clone(),
            product.as_ref(),
        ));
        let faults = Arc::new(FaultServiceAdapter::new(body_codec, product.as_ref()));
        let certificates = Arc::new(CertificateServiceAdapter::new(
            (sqlite.clone(), Arc::clone(&store)),
            Arc::clone(&protector),
            Arc::clone(dialog),
            product,
        ));
        let protected_secrets = Arc::new(ProtectedSecretAdapter::new(
            (sqlite.clone(), Arc::clone(&store)),
            Arc::clone(&protector),
        ));
        let workspaces = Arc::new(WorkspaceRepositoryAdapter::new(sqlite.clone()));
        let protocol_packages = ProtocolPackageRepositoryAdapter::with_default_limits((
            sqlite.clone(),
            Arc::clone(&store),
        ));
        let protocol_packages = match builtin_protocol_package {
            Some(archive) => protocol_packages.with_builtin_archive(archive),
            None => protocol_packages,
        };
        let protocol_packages = Arc::new(protocol_packages);
        let listener_certificates = Arc::new(ManagedListenerCertificateAdapter::new(
            (sqlite.clone(), Arc::clone(&store)),
            protector,
            Arc::clone(dialog),
        ));
        let workspace_body_codecs = Arc::new(WorkspaceBodyCodecResolver::new());
        let external_packages =
            ExternalPackageRegistryAdapter::new((sqlite.clone(), Arc::clone(&store)));
        let gates = environment_apply_resource_gates.clone();
        let external_packages = external_packages.with_environment_apply_resource_gates(gates);
        let external_packages = Arc::new(external_packages);
        let protocol_package_import = Arc::new(ProtocolPackageImportAdapter::new(
            protocol_packages.clone(),
            external_packages.clone(),
            Arc::clone(dialog),
        ));
        let environment_validator = Arc::new(EnvironmentConfigurationValidationAdapter::new(
            protocol_packages.clone(),
            external_packages.clone(),
            certificates.clone(),
        ));
        let listener_runtime = ListenerRuntimeAdapter::new(
            Arc::clone(&store),
            protected_secrets.clone(),
            protocol_packages.clone(),
        );
        let gates = environment_apply_resource_gates.clone();
        let listener_runtime = listener_runtime.with_environment_apply_resource_gates(gates);
        let listener_runtime = Arc::new(
            listener_runtime
                .with_mitm_certificate_authority(certificates.clone())
                .with_managed_listener_certificates(listener_certificates.clone()),
        );
        listener_runtime.set_external_package_provider(external_packages.clone());
        let protocol_package_usage = Arc::new(ProtocolPackageUsageQueryAdapter::new(
            workspaces.clone(),
            listener_runtime.clone(),
        ));
        Self {
            store,
            sqlite,
            settings,
            workspaces,
            workspace_body_codecs,
            listener_runtime,
            listener_certificates,
            protected_secrets,
            protocol_packages,
            protocol_package_import,
            protocol_package_usage,
            external_packages,
            rules,
            faults,
            certificates,
            capture,
            sessions,
            environment_apply_resource_gates,
            environment_material_preparer,
            environment_commit,
            environment_validator,
        }
    }

    pub async fn initialize_installation_state(&self) -> AppResult<()> {
        // 先完整解码现有配置，再执行内建包、证书或默认 Workspace 的任何写入。
        // 不兼容的持久化记录必须 fail-closed，并保持数据库及 sidecar 原样等待用户处理。
        let workspaces = self.workspaces.list().await?;
        let stored = self.settings.get().await?.stored;
        self.protocol_packages.ensure_builtin_seeded_async().await?;
        if let Err(error) = self
            .certificates
            .synchronize_installation_ca(vec!["localhost".into(), "127.0.0.1".into()])
            .await
        {
            if recoverable_secret_store_error(&error) {
                tracing::warn!(
                    code = %error.view_model.code,
                    message = %error.view_model.message,
                    "installation certificate synchronization was deferred"
                );
            } else {
                return Err(error);
            }
        }
        if workspaces.is_empty() {
            self.workspaces.create("默认 Workspace".into()).await?;
        }
        self.sessions
            .set_limits(stored.max_sessions, stored.max_memory_bytes)?;
        Ok(())
    }

    pub fn configure_runtime(
        &self,
        product: &dyn ProductProfile,
        breakpoints: Arc<BreakpointCoordinator>,
        events: Arc<EventHub>,
    ) {
        let channel_labels = product
            .channels()
            .iter()
            .map(|channel| (channel.id.to_owned(), channel.display_name.to_owned()))
            .collect::<BTreeMap<_, _>>();
        let pipeline = Arc::new(
            RuntimePipelineAdapter::new(
                RuntimePipelineProductHooks {
                    body_codec: product.body_codec(),
                    request_classifier: product.request_classifier(),
                    channel_labels,
                },
                self.rules.clone(),
                self.sessions.clone(),
                breakpoints,
                events.clone(),
                self.capture.clone(),
            )
            .with_body_codec_resolver(self.workspace_body_codecs.clone())
            .with_joint_http_rules(self.listener_runtime.joint_http_rules()),
        );
        self.listener_runtime
            .set_body_codec_resolver(self.workspace_body_codecs.clone());
        self.listener_runtime.set_pipeline_ports(pipeline);
        self.listener_runtime
            .set_socket_diagnostic_events(events.clone());
        self.external_packages.set_event_hub(events);
    }

    pub async fn start_external_package_server(&self) -> AppResult<ExternalPackageServer> {
        let stored = self.settings.get().await?.stored;
        let max_body_bytes = stored.max_body_bytes;
        let external = stored.external_package_service;
        let ip = external.bind_address.parse::<IpAddr>().map_err(|_| {
            AppError::new("CONFIG_INVALID", "外部软件包服务监听地址不是有效 IP 地址。")
        })?;
        let rpc_message_bytes = usize::try_from(max_body_bytes)
            .unwrap_or(usize::MAX / 2)
            .saturating_mul(4)
            .div_ceil(3)
            .saturating_add(64 * 1024);
        let connection = PackageTransportConfig::new(
            Duration::from_secs(30),
            Duration::from_secs(10),
            Duration::from_secs(30),
            usize::try_from(max_body_bytes).unwrap_or(usize::MAX),
            rpc_message_bytes,
            1024 * 1024,
            128 * 1024,
        );
        let websocket_url = format!("ws://{}:{}/packages", ip, external.port);
        let executable = std::env::current_exe()
            .map_err(|error| AppError::new("EXTERNAL_PACKAGE_PROCESS_FAILED", error.to_string()))?
            .with_file_name(if cfg!(windows) {
                "intercept-proxy-package-sidecar.exe"
            } else {
                "intercept-proxy-package-sidecar"
            });
        let supervisor = Arc::new(LocalPackageSupervisor::new(
            executable,
            websocket_url,
            self.external_packages.clone(),
        ));
        self.external_packages.set_local_supervisor(&supervisor);
        self.protocol_package_import
            .set_supervisor(supervisor.clone());
        let enabled = self.external_packages.enabled_local_archives().await?;
        let server = ExternalPackageServer::start(
            ExternalPackageServerConfig {
                bind_address: SocketAddr::new(ip, external.port),
                connection,
            },
            self.external_packages.clone(),
            self.protocol_package_usage.clone(),
            self.listener_runtime.clone(),
        )
        .await
        .with_local_supervisor(supervisor.clone());
        supervisor.start_enabled(enabled);
        Ok(server)
    }

    pub async fn into_application(
        self,
        product_name: String,
        android_companion_apk: Option<PathBuf>,
        breakpoints: Arc<BreakpointCoordinator>,
        events: Arc<EventHub>,
    ) -> Result<Application, InfrastructureError> {
        self.into_application_inner(
            product_name,
            android_companion_apk,
            breakpoints,
            events,
            None,
        )
        .await
    }

    /// Builds the real Application while replacing its complete environment-configuration port
    /// group. This is an embedding/test assembly seam; it does not expose registry internals or
    /// alter candidate lifecycle behavior.
    pub async fn into_application_with_environment_configuration_services(
        self,
        product_name: String,
        android_companion_apk: Option<PathBuf>,
        breakpoints: Arc<BreakpointCoordinator>,
        events: Arc<EventHub>,
        environment: intercept_proxy_application::EnvironmentConfigurationApplicationServices,
    ) -> Result<Application, InfrastructureError> {
        self.into_application_inner(
            product_name,
            android_companion_apk,
            breakpoints,
            events,
            Some(environment),
        )
        .await
    }

    async fn into_application_inner(
        self,
        product_name: String,
        android_companion_apk: Option<PathBuf>,
        breakpoints: Arc<BreakpointCoordinator>,
        events: Arc<EventHub>,
        environment_override: Option<
            intercept_proxy_application::EnvironmentConfigurationApplicationServices,
        >,
    ) -> Result<Application, InfrastructureError> {
        let apply_sqlite = self.sqlite.clone();
        let android =
            AndroidAdbAdapter::new(android_companion_apk, (self.sqlite, self.store)).await?;
        let gates = self.environment_apply_resource_gates.clone();
        let android = android.with_environment_apply_resource_gates(gates);
        let android = Arc::new(android);
        AndroidAdbAdapter::start_control_lease_heartbeat(&android);
        let apply_runtime = Arc::new(EnvironmentApplyRuntimeAdapter::new(
            self.listener_runtime.clone(),
            android.clone(),
            self.protocol_packages.clone(),
            self.external_packages.clone(),
            apply_sqlite,
            self.environment_apply_resource_gates.clone(),
        ));
        let environment = environment_override.unwrap_or_else(|| {
            let environment_apply_lease =
                Arc::new(EnvironmentApplyLeaseAdapter::with_resource_gates(
                    apply_runtime.clone(),
                    self.environment_apply_resource_gates,
                ));
            intercept_proxy_application::EnvironmentConfigurationApplicationServices {
                baseline_capture: apply_runtime,
                identity_allocator:
                    intercept_proxy_application::EnvironmentIdentityAllocator::random(),
                apply_lease: environment_apply_lease,
                material_preparer: self.environment_material_preparer,
                commit: self.environment_commit,
                validator: self.environment_validator,
            }
        });
        let protocol_packages = ProtocolPackageApplicationServices {
            store: self.protocol_packages.clone(),
            compiler: self.protocol_packages.clone(),
            importer: self.protocol_package_import,
            builtin: self.protocol_packages.clone(),
            usage_query: self.protocol_package_usage,
            portability: self.protocol_packages,
            external: self.external_packages,
        };
        Ok(Application::new(
            product_name,
            ApplicationDependencies {
                capture: self.capture,
                sessions: self.sessions,
                breakpoints,
                breakpoint_validation: Arc::new(BreakpointValidator::new_with_resolver(Arc::new(
                    HeaderBodyCodecResolver,
                ))),
                faults: self.faults,
                certificates: self.certificates,
                settings: self.settings,
                workspaces: self.workspaces,
                listener_runtime: self.listener_runtime,
                listener_certificates: self.listener_certificates,
                protocol_packages,
                events,
                environment_baseline_capture: environment.baseline_capture,
                environment_identity_allocator: environment.identity_allocator,
                environment_apply_lease: environment.apply_lease,
                environment_material_preparer: environment.material_preparer,
                environment_commit: environment.commit,
                environment_validator: environment.validator,
            },
            android,
            self.protected_secrets,
        ))
    }
}

fn recoverable_secret_store_error(error: &AppError) -> bool {
    matches!(
        error.view_model.code.as_str(),
        "KEYCHAIN_PROTECT_FAILED"
            | "KEYCHAIN_UNPROTECT_FAILED"
            | "DPAPI_PROTECT_FAILED"
            | "DPAPI_UNPROTECT_FAILED"
    )
}
