//! 基础设施服务的统一装配包。
//!
//! 它共享数据库、对话框、事件中心等长生命周期资源，并确保各适配器拿到同一份状态；
//! 构造失败会整体返回，避免应用只启动一半服务。

use std::sync::Arc;

use gmofg_proxy_application::{CapacityLedger, InMemorySessionStore};
use gmofg_proxy_product_api::ProductProfile;

use crate::{SecretProtector, SqliteStore};

use super::{
    CaptureRepositoryAdapter, CertificateServiceAdapter, FaultServiceAdapter, FileExportAdapter,
    NativeFileDialog, RuleRepositoryAdapter, SettingsRepositoryAdapter,
};

#[derive(Debug)]
pub struct InfrastructureServiceBundle {
    pub settings: Arc<SettingsRepositoryAdapter>,
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
        Self {
            settings: Arc::new(SettingsRepositoryAdapter::new(
                Arc::clone(&store),
                product.as_ref(),
            )),
            faults: Arc::new(FaultServiceAdapter::new(
                Arc::clone(&rules),
                body_codec,
                product.as_ref(),
            )),
            certificates: Arc::new(CertificateServiceAdapter::new(
                store,
                protector,
                Arc::clone(&dialog),
                product,
            )),
            file_export: Arc::new(FileExportAdapter::new(dialog)),
            capture,
            sessions,
            capacity,
            rules,
        }
    }
}
