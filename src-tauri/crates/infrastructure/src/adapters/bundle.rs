use std::sync::Arc;

use gmofg_proxy_application::InMemorySessionStore;

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
}

impl InfrastructureServiceBundle {
    #[must_use]
    pub fn new(
        store: Arc<SqliteStore>,
        protector: Arc<dyn SecretProtector>,
        dialog: Arc<dyn NativeFileDialog>,
    ) -> Self {
        let sessions = Arc::new(InMemorySessionStore::default());
        let capture = Arc::new(CaptureRepositoryAdapter::new(sessions.clone()));
        let rules = Arc::new(RuleRepositoryAdapter::new(
            Arc::clone(&store),
            Arc::clone(&dialog),
            sessions.clone(),
        ));
        Self {
            settings: Arc::new(SettingsRepositoryAdapter::new(Arc::clone(&store))),
            faults: Arc::new(FaultServiceAdapter::new(Arc::clone(&rules))),
            certificates: Arc::new(CertificateServiceAdapter::new(
                store,
                protector,
                Arc::clone(&dialog),
            )),
            file_export: Arc::new(FileExportAdapter::new(dialog)),
            capture,
            sessions,
            rules,
        }
    }
}
