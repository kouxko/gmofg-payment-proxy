//! 基础设施服务的统一装配包。
//!
//! 它共享数据库、对话框、事件中心等长生命周期资源，并确保各适配器拿到同一份状态；
//! 构造失败会整体返回，避免应用只启动一半服务。

use std::sync::Arc;

use intercept_proxy_application::{CapacityLedger, InMemorySessionStore};
use intercept_proxy_product_api::ProductProfile;

use crate::{SecretProtector, SqliteStore};

use super::{
    CaptureRepositoryAdapter, CertificateServiceAdapter, FaultServiceAdapter,
    ListenerRuntimeAdapter, ManagedListenerCertificateAdapter, NativeFileDialog,
    ProtectedSecretAdapter, ProtocolPackageImportAdapter, ProtocolPackageRepositoryAdapter,
    ProtocolPackageUsageQueryAdapter, RuleRepositoryAdapter, SettingsRepositoryAdapter,
    SocketCaptureRepositoryAdapter, WorkspaceBodyCodecResolver, WorkspaceRepositoryAdapter,
};

#[derive(Debug)]
pub struct InfrastructureServiceBundle {
    pub settings: Arc<SettingsRepositoryAdapter>,
    pub workspaces: Arc<WorkspaceRepositoryAdapter>,
    pub workspace_body_codecs: Arc<WorkspaceBodyCodecResolver>,
    pub listener_runtime: Arc<ListenerRuntimeAdapter>,
    pub listener_certificates: Arc<ManagedListenerCertificateAdapter>,
    pub protected_secrets: Arc<ProtectedSecretAdapter>,
    /// 应用级协议包文件、启用位和可重建编译缓存；生命周期约束由 T14 Application 用例接管。
    pub protocol_packages: Arc<ProtocolPackageRepositoryAdapter>,
    /// 原生文件选择与有界 ZIP 读取；路径和 ZIP 字节不越过 Application 端口。
    pub protocol_package_import: Arc<ProtocolPackageImportAdapter>,
    /// 汇总全部 Workspace 的精确引用，并与 Listener 运行态合并。
    pub protocol_package_usage: Arc<ProtocolPackageUsageQueryAdapter>,
    pub rules: Arc<RuleRepositoryAdapter>,
    pub faults: Arc<FaultServiceAdapter>,
    pub certificates: Arc<CertificateServiceAdapter>,
    pub capture: Arc<CaptureRepositoryAdapter>,
    pub sessions: Arc<InMemorySessionStore>,
    pub capacity: Arc<CapacityLedger>,
}

impl InfrastructureServiceBundle {
    #[must_use]
    pub fn new(
        store: Arc<SqliteStore>,
        protector: Arc<dyn SecretProtector>,
        dialog: Arc<dyn NativeFileDialog>,
        product: Arc<dyn ProductProfile>,
        capacity: Arc<CapacityLedger>,
        builtin_protocol_package: Option<Arc<[u8]>>,
    ) -> Self {
        let body_codec = product.body_codec();
        let sessions = Arc::new(InMemorySessionStore::with_capacity_ledger(
            InMemorySessionStore::DEFAULT_MAX_SESSIONS,
            Arc::clone(&capacity),
        ));
        let socket_capture = Arc::new(SocketCaptureRepositoryAdapter::new(Arc::clone(&store)));
        let capture = Arc::new(CaptureRepositoryAdapter::new(
            sessions.clone(),
            Arc::clone(&socket_capture),
        ));
        let rules = Arc::new(RuleRepositoryAdapter::new(
            Arc::clone(&store),
            Arc::clone(&dialog),
            sessions.clone(),
            product.channels(),
        ));
        let settings = Arc::new(SettingsRepositoryAdapter::new(
            Arc::clone(&store),
            product.as_ref(),
        ));
        let faults = Arc::new(FaultServiceAdapter::new(
            Arc::clone(&rules),
            body_codec,
            product.as_ref(),
        ));
        let certificates = Arc::new(CertificateServiceAdapter::new(
            Arc::clone(&store),
            Arc::clone(&protector),
            Arc::clone(&dialog),
            product,
        ));
        let protected_secrets = Arc::new(ProtectedSecretAdapter::new(
            Arc::clone(&store),
            Arc::clone(&protector),
        ));
        let workspaces = Arc::new(WorkspaceRepositoryAdapter::new(Arc::clone(&store)));
        let protocol_packages =
            ProtocolPackageRepositoryAdapter::with_default_limits(Arc::clone(&store));
        let protocol_packages = match builtin_protocol_package {
            Some(archive) => protocol_packages.with_builtin_archive(archive),
            None => protocol_packages,
        };
        let protocol_packages = Arc::new(protocol_packages);
        let protocol_package_import = Arc::new(ProtocolPackageImportAdapter::new(
            protocol_packages.clone(),
            Arc::clone(&dialog),
        ));
        let listener_certificates = Arc::new(ManagedListenerCertificateAdapter::new(
            Arc::clone(&store),
            protector,
            Arc::clone(&dialog),
        ));
        let workspace_body_codecs = Arc::new(WorkspaceBodyCodecResolver::new(Arc::clone(&store)));
        let listener_runtime = Arc::new(
            ListenerRuntimeAdapter::new(
                store,
                protected_secrets.clone(),
                protocol_packages.clone(),
            )
            .with_mitm_certificate_authority(certificates.clone())
            .with_installation_server_identity(certificates.clone())
            .with_managed_listener_certificates(listener_certificates.clone()),
        );
        listener_runtime.set_socket_capture_repository(socket_capture);
        let protocol_package_usage = Arc::new(ProtocolPackageUsageQueryAdapter::new(
            workspaces.clone(),
            listener_runtime.clone(),
        ));
        Self {
            workspaces,
            workspace_body_codecs,
            listener_runtime,
            listener_certificates,
            protected_secrets,
            protocol_packages,
            protocol_package_import,
            protocol_package_usage,
            settings,
            faults,
            certificates,
            capture,
            sessions,
            capacity,
            rules,
        }
    }
}
