use std::collections::HashMap;

use super::*;

mod models;
pub(super) use models::*;

#[derive(Debug, Default)]
pub(super) struct ProtocolPortFailures {
    pub list: Option<AppError>,
    pub get: Option<AppError>,
    pub describe: Option<AppError>,
    pub installed_preflight: Option<AppError>,
    pub import: Option<AppError>,
    pub usage: Option<AppError>,
    pub set_enabled: Option<AppError>,
    pub delete: Option<AppError>,
}

#[derive(Debug, Default)]
pub(super) struct FakeProtocolPackageServices {
    records: parking_lot::Mutex<HashMap<ProtocolPackageRef, ProtocolPackageVersionViewModel>>,
    usages: parking_lot::Mutex<HashMap<ProtocolPackageRef, Vec<ProtocolPackageUsageViewModel>>>,
    usage_responses: parking_lot::Mutex<VecDeque<AppResult<Vec<ProtocolPackageUsageViewModel>>>>,
    descriptions:
        parking_lot::Mutex<HashMap<ProtocolPackageRef, ProtocolPackageDescriptionViewModel>>,
    import_responses:
        parking_lot::Mutex<VecDeque<AppResult<Option<ProtocolPackageImportPreviewViewModel>>>>,
    import_commit_responses:
        parking_lot::Mutex<VecDeque<AppResult<ProtocolPackageImportViewModel>>>,
    pub failures: parking_lot::Mutex<ProtocolPortFailures>,
    pub get_calls: AtomicUsize,
    pub describe_calls: AtomicUsize,
    pub installed_preflight_calls: AtomicUsize,
    pub import_calls: AtomicUsize,
    pub usage_calls: AtomicUsize,
    pub usage_count_calls: AtomicUsize,
    pub set_enabled_calls: AtomicUsize,
    pub delete_calls: AtomicUsize,
    pub application_export_calls: AtomicUsize,
    pub exact_calls: parking_lot::Mutex<Vec<ProtocolPackageRef>>,
    pub block_describe: AtomicBool,
    pub describe_entered: tokio::sync::Notify,
    pub continue_describe: tokio::sync::Notify,
    pub block_installed_preflight: AtomicBool,
    pub installed_preflight_entered: tokio::sync::Notify,
    pub continue_installed_preflight: tokio::sync::Notify,
    pub block_usage: AtomicBool,
    pub usage_entered: tokio::sync::Notify,
    pub continue_usage: tokio::sync::Notify,
}

impl FakeProtocolPackageServices {
    pub fn insert(&self, record: ProtocolPackageVersionViewModel) {
        self.records.lock().insert(record.package.clone(), record);
    }

    pub fn record(&self, package: &ProtocolPackageRef) -> Option<ProtocolPackageVersionViewModel> {
        self.records.lock().get(package).cloned()
    }

    pub fn set_usages(
        &self,
        package: ProtocolPackageRef,
        usages: Vec<ProtocolPackageUsageViewModel>,
    ) {
        self.usages.lock().insert(package, usages);
    }

    pub fn usages(&self, package: &ProtocolPackageRef) -> Vec<ProtocolPackageUsageViewModel> {
        self.usages.lock().get(package).cloned().unwrap_or_default()
    }

    pub fn push_usage_response(&self, response: AppResult<Vec<ProtocolPackageUsageViewModel>>) {
        self.usage_responses.lock().push_back(response);
    }

    pub fn set_description(
        &self,
        package: ProtocolPackageRef,
        description: ProtocolPackageDescriptionViewModel,
    ) {
        self.descriptions.lock().insert(package, description);
    }

    pub fn push_import_response(
        &self,
        response: AppResult<Option<ProtocolPackageImportPreviewViewModel>>,
    ) {
        self.import_responses.lock().push_back(response);
    }

    pub fn push_import_commit_response(&self, response: AppResult<ProtocolPackageImportViewModel>) {
        self.import_commit_responses.lock().push_back(response);
    }

    fn record_call(&self, package: &ProtocolPackageRef) {
        self.exact_calls.lock().push(package.clone());
    }

    fn preflight<T>(
        &self,
        packages: &[T],
        identity: impl Fn(&T) -> &ProtocolPackageRef,
    ) -> AppResult<Vec<ProtocolPackageDescriptionViewModel>> {
        let descriptions = self.descriptions.lock();
        packages
            .iter()
            .map(|package| {
                let package = identity(package);
                Ok(descriptions
                    .get(package)
                    .cloned()
                    .unwrap_or_else(|| description(package.clone())))
            })
            .collect()
    }
}

#[async_trait]
impl ExternalPackageApplicationPort for FakeProtocolPackageServices {
    async fn service_status(&self) -> AppResult<ExternalPackageServiceStatusViewModel> {
        unused()
    }
    async fn list(&self) -> AppResult<Vec<ProtocolPackageVersionViewModel>> {
        if let Some(error) = self.failures.lock().list.clone() {
            return Err(error);
        }
        Ok(self.records.lock().values().cloned().collect())
    }

    async fn get(
        &self,
        package: &ProtocolPackageRef,
    ) -> AppResult<Option<ProtocolPackageVersionViewModel>> {
        self.get_calls.fetch_add(1, Ordering::SeqCst);
        self.record_call(package);
        if let Some(error) = self.failures.lock().get.clone() {
            return Err(error);
        }
        Ok(self.records.lock().get(package).cloned())
    }

    async fn describe(
        &self,
        package: &ProtocolPackageRef,
    ) -> AppResult<ProtocolPackageDescriptionViewModel> {
        self.describe_calls.fetch_add(1, Ordering::SeqCst);
        self.record_call(package);
        if self.block_describe.swap(false, Ordering::SeqCst) {
            self.describe_entered.notify_one();
            self.continue_describe.notified().await;
        }
        if let Some(error) = self.failures.lock().describe.clone() {
            return Err(error);
        }
        if !self.records.lock().contains_key(package) {
            return Err(AppError::new(
                "PROTOCOL_PACKAGE_NOT_FOUND",
                "测试记录不存在。",
            ));
        }
        Ok(self
            .descriptions
            .lock()
            .get(package)
            .cloned()
            .unwrap_or_else(|| description(package.clone())))
    }

    async fn detail(&self, _: &ProtocolPackageRef) -> AppResult<ExternalPackageDetailViewModel> {
        let methods = ExternalPackageDirectionMethodsViewModel {
            frame: "hooks.frame".into(),
            decode: "hooks.decode".into(),
            encode: "hooks.encode".into(),
            display: "hooks.display".into(),
        };
        Ok(ExternalPackageDetailViewModel {
            local_process: false,
            remote_address: Some("127.0.0.1:9000".into()),
            connection_id: None,
            first_connected_at: Utc.timestamp_opt(1_700_000_000, 0).single().unwrap(),
            last_connected_at: Utc.timestamp_opt(1_700_000_000, 0).single().unwrap(),
            registration_fingerprint_sha256: "00".repeat(32),
            upstream_methods: methods.clone(),
            downstream_methods: methods,
            recent_error: None,
        })
    }

    async fn set_enabled(&self, package: &ProtocolPackageRef, enabled: bool) -> AppResult<()> {
        self.set_enabled_calls.fetch_add(1, Ordering::SeqCst);
        self.record_call(package);
        if let Some(error) = self.failures.lock().set_enabled.clone() {
            return Err(error);
        }
        let mut records = self.records.lock();
        let record = records
            .get_mut(package)
            .ok_or_else(|| AppError::new("PROTOCOL_PACKAGE_NOT_FOUND", "测试记录不存在。"))?;
        record.enabled = enabled;
        Ok(())
    }

    async fn delete(&self, package: &ProtocolPackageRef) -> AppResult<()> {
        self.delete_calls.fetch_add(1, Ordering::SeqCst);
        self.record_call(package);
        if let Some(error) = self.failures.lock().delete.clone() {
            return Err(error);
        }
        self.records
            .lock()
            .remove(package)
            .ok_or_else(|| AppError::new("PROTOCOL_PACKAGE_NOT_FOUND", "测试记录不存在。"))?;
        Ok(())
    }

    async fn restart(&self, _: &ProtocolPackageRef) -> AppResult<()> {
        unused()
    }
    async fn disconnect(&self, _: &ProtocolPackageRef) -> AppResult<()> {
        Ok(())
    }

    async fn application_backup_baseline(
        &self,
    ) -> AppResult<Vec<ApplicationBackupProtocolPackageBaseline>> {
        Ok(self
            .records
            .lock()
            .values()
            .map(|record| ApplicationBackupProtocolPackageBaseline {
                package: record.package.clone(),
                enabled: record.enabled,
                generation: uuid::Uuid::nil(),
            })
            .collect())
    }
    async fn export_application_packages(
        &self,
    ) -> AppResult<Vec<PortableApplicationProtocolPackage>> {
        self.application_export_calls.fetch_add(1, Ordering::SeqCst);
        Ok(self
            .records
            .lock()
            .values()
            .map(|record| PortableApplicationProtocolPackage {
                package: record.package.clone(),
                files: Vec::new(),
                enabled: record.enabled,
            })
            .collect())
    }
    async fn preflight_application_packages(
        &self,
        packages: &[PortableApplicationProtocolPackage],
    ) -> AppResult<Vec<ProtocolPackageDescriptionViewModel>> {
        self.preflight(packages, |package| &package.package)
    }
    async fn preflight_installed_packages(
        &self,
        packages: &[ProtocolPackageRef],
    ) -> AppResult<Vec<ProtocolPackageDescriptionViewModel>> {
        self.installed_preflight_calls
            .fetch_add(1, Ordering::SeqCst);
        if self.block_installed_preflight.swap(false, Ordering::SeqCst) {
            self.installed_preflight_entered.notify_one();
            self.continue_installed_preflight.notified().await;
        }
        if let Some(error) = self.failures.lock().installed_preflight.clone() {
            return Err(error);
        }
        self.preflight(packages, |package| package)
    }
    async fn replace_application_bundle(
        &self,
        _: Vec<PortableApplicationProtocolPackage>,
        _: ApplicationConfigurationDocument,
    ) -> AppResult<()> {
        unused()
    }
    async fn reset_application_bundle(&self, _: ApplicationConfigurationDocument) -> AppResult<()> {
        unused()
    }
}

#[async_trait]
impl ProtocolPackageImportPort for FakeProtocolPackageServices {
    async fn prepare_zip(&self) -> AppResult<Option<ProtocolPackageImportPreviewViewModel>> {
        self.import_calls.fetch_add(1, Ordering::SeqCst);
        if let Some(error) = self.failures.lock().import.clone() {
            return Err(error);
        }
        self.import_responses.lock().pop_front().unwrap_or(Ok(None))
    }

    async fn commit_zip(
        &self,
        _: ProtocolPackageImportToken,
    ) -> AppResult<ProtocolPackageImportViewModel> {
        self.import_commit_responses
            .lock()
            .pop_front()
            .unwrap_or_else(|| {
                Err(AppError::new(
                    "PROTOCOL_PACKAGE_IMPORT_TOKEN_INVALID",
                    "测试令牌不存在。",
                ))
            })
    }

    async fn discard_zip(&self, _: ProtocolPackageImportToken) -> AppResult<()> {
        Ok(())
    }
}

#[async_trait]
impl ProtocolPackageUsageQueryPort for FakeProtocolPackageServices {
    async fn usages(
        &self,
        package: &ProtocolPackageRef,
    ) -> AppResult<Vec<ProtocolPackageUsageViewModel>> {
        self.usage_calls.fetch_add(1, Ordering::SeqCst);
        self.record_call(package);
        if self.block_usage.load(Ordering::SeqCst) {
            self.usage_entered.notify_one();
            self.continue_usage.notified().await;
        }
        if let Some(response) = self.usage_responses.lock().pop_front() {
            return response;
        }
        if let Some(error) = self.failures.lock().usage.clone() {
            return Err(error);
        }
        Ok(self.usages(package))
    }

    async fn usage_counts(&self) -> AppResult<Vec<ProtocolPackageUsageCount>> {
        self.usage_count_calls.fetch_add(1, Ordering::SeqCst);
        if let Some(error) = self.failures.lock().usage.clone() {
            return Err(error);
        }
        Ok(self
            .usages
            .lock()
            .iter()
            .map(|(package, usages)| ProtocolPackageUsageCount {
                package: package.clone(),
                reference_count: usages.len(),
                active_reference_count: usages
                    .iter()
                    .filter(|usage| usage.blocks_disable())
                    .count(),
            })
            .collect())
    }
}

pub(super) fn application(
    services: Arc<FakeProtocolPackageServices>,
    workspaces: Arc<InMemoryWorkspaceStore>,
    runtime: Arc<InMemoryListenerRuntime>,
) -> Application {
    application_with_listener_runtime(services, workspaces, runtime)
}

pub(super) fn application_with_listener_runtime(
    services: Arc<FakeProtocolPackageServices>,
    workspaces: Arc<InMemoryWorkspaceStore>,
    runtime: Arc<dyn ListenerRuntimePort>,
) -> Application {
    let ports = Arc::new(FakePorts::default());
    application_with_proxy_ports(services, workspaces, runtime, ports)
}

pub(super) fn application_with_proxy_ports(
    services: Arc<FakeProtocolPackageServices>,
    workspaces: Arc<InMemoryWorkspaceStore>,
    runtime: Arc<dyn ListenerRuntimePort>,
    ports: Arc<FakePorts>,
) -> Application {
    Application::new(
        "Protocol lifecycle test".into(),
        ApplicationDependencies {
            capture: ports.clone(),
            sessions: ports.clone(),
            breakpoints: Arc::new(BreakpointCoordinator::default()),
            breakpoint_validation: ports.clone(),
            faults: ports.clone(),
            certificates: ports.clone(),
            settings: ports.clone(),
            workspaces,
            listener_runtime: runtime,
            listener_certificates: ports,
            protocol_packages: ProtocolPackageApplicationServices {
                importer: services.clone(),
                builtin: unused_protocol_package_services().builtin,
                usage_query: services.clone(),
                external: services,
            },
            events: Arc::new(EventHub::default()),
            environment_baseline_capture:
                crate::requirements_tests::test_environment_baseline_capture(),
            environment_identity_allocator:
                crate::requirements_tests::test_environment_identity_allocator(),
            environment_apply_lease: crate::requirements_tests::test_environment_apply_lease(),
            environment_material_preparer:
                crate::requirements_tests::test_environment_material_preparer(),
            environment_commit: crate::requirements_tests::test_environment_commit(),
            environment_validator: crate::requirements_tests::test_environment_validator(),
        },
        Arc::new(UnusedAndroidControlPort),
        Arc::new(UnusedProtectedSecretPort),
    )
}
pub(super) fn error_code(error: &AppError) -> &str {
    &error.view_model.code
}
