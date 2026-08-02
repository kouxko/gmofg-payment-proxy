//! 基础设施服务的统一装配包。
//!
//! 它共享数据库、对话框、事件中心等长生命周期资源，并确保各适配器拿到同一份状态；
//! 构造失败会整体返回，避免应用只启动一半服务。

use std::sync::Arc;

use intercept_proxy_application::{CapacityLedger, InMemorySessionStore};
use intercept_proxy_product_api::ProductProfile;

use crate::{SecretProtector, SqliteStore};

use super::{
    CaptureRepositoryAdapter, CertificateServiceAdapter, FaultServiceAdapter, FileExportAdapter,
    ListenerRuntimeAdapter, NativeFileDialog, ProtectedSecretAdapter, RuleRepositoryAdapter,
    SettingsRepositoryAdapter, WorkspaceBodyCodecResolver, WorkspaceDocumentAdapter,
    WorkspaceRepositoryAdapter, WorkspaceRuntimePolicyResolver,
};

#[derive(Debug)]
pub struct InfrastructureServiceBundle {
    pub settings: Arc<SettingsRepositoryAdapter>,
    pub workspaces: Arc<WorkspaceRepositoryAdapter>,
    pub workspace_documents: Arc<WorkspaceDocumentAdapter>,
    pub workspace_body_codecs: Arc<WorkspaceBodyCodecResolver>,
    pub workspace_runtime_policies: Arc<WorkspaceRuntimePolicyResolver>,
    pub listener_runtime: Arc<ListenerRuntimeAdapter>,
    pub protected_secrets: Arc<ProtectedSecretAdapter>,
    pub rules: Arc<RuleRepositoryAdapter>,
    pub faults: Arc<FaultServiceAdapter>,
    pub certificates: Arc<CertificateServiceAdapter>,
    pub file_export: Arc<FileExportAdapter>,
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
    ) -> Self {
        let body_codec = product.body_codec();
        let sessions = Arc::new(InMemorySessionStore::with_capacity_ledger(
            InMemorySessionStore::DEFAULT_MAX_SESSIONS,
            Arc::clone(&capacity),
        ));
        let capture = Arc::new(CaptureRepositoryAdapter::new(sessions.clone()));
        let rules = Arc::new(RuleRepositoryAdapter::new(
            Arc::clone(&store),
            Arc::clone(&dialog),
            sessions.clone(),
            product.channels(),
            product.persistence_migrations().terminal_body_fields,
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
        let protected_secrets =
            Arc::new(ProtectedSecretAdapter::new(Arc::clone(&store), protector));
        let workspace_body_codecs = Arc::new(WorkspaceBodyCodecResolver::new(Arc::clone(&store)));
        let workspace_runtime_policies =
            Arc::new(WorkspaceRuntimePolicyResolver::new(Arc::clone(&store)));
        let listener_runtime = Arc::new(
            ListenerRuntimeAdapter::new(Arc::clone(&store))
                .with_mitm_certificate_authority(certificates.clone())
                .with_protected_secrets(protected_secrets.clone()),
        );
        Self {
            workspaces: Arc::new(WorkspaceRepositoryAdapter::new(store)),
            workspace_documents: Arc::new(WorkspaceDocumentAdapter::new(Arc::clone(&dialog))),
            workspace_body_codecs,
            workspace_runtime_policies,
            listener_runtime,
            protected_secrets,
            settings,
            faults,
            certificates,
            file_export: Arc::new(FileExportAdapter::new(dialog)),
            capture,
            sessions,
            capacity,
            rules,
        }
    }
}
