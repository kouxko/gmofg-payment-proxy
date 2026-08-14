use std::collections::HashMap;

use super::*;

mod portability;

#[derive(Debug, Default)]
pub(super) struct ProtocolPortFailures {
    pub list: Option<AppError>,
    pub get: Option<AppError>,
    pub compile: Option<AppError>,
    pub describe: Option<AppError>,
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
    compilation_results: parking_lot::Mutex<
        HashMap<ProtocolPackageRef, AppResult<ProtocolPackageCompilationReceipt>>,
    >,
    descriptions:
        parking_lot::Mutex<HashMap<ProtocolPackageRef, ProtocolPackageDescriptionViewModel>>,
    import_responses:
        parking_lot::Mutex<VecDeque<AppResult<Option<ProtocolPackageImportPreviewViewModel>>>>,
    import_commit_responses:
        parking_lot::Mutex<VecDeque<AppResult<ProtocolPackageImportViewModel>>>,
    pub failures: parking_lot::Mutex<ProtocolPortFailures>,
    pub get_calls: AtomicUsize,
    pub compile_calls: AtomicUsize,
    pub describe_calls: AtomicUsize,
    pub import_calls: AtomicUsize,
    pub usage_calls: AtomicUsize,
    pub usage_count_calls: AtomicUsize,
    pub set_enabled_calls: AtomicUsize,
    pub delete_calls: AtomicUsize,
    pub workspace_export_calls: AtomicUsize,
    pub application_export_calls: AtomicUsize,
    pub exported_workspace_refs: parking_lot::Mutex<Vec<ProtocolPackageRef>>,
    pub exact_calls: parking_lot::Mutex<Vec<ProtocolPackageRef>>,
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

    pub fn set_compilation_result(
        &self,
        package: ProtocolPackageRef,
        result: AppResult<ProtocolPackageCompilationReceipt>,
    ) {
        self.compilation_results.lock().insert(package, result);
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
}

#[async_trait]
impl ProtocolPackageStorePort for FakeProtocolPackageServices {
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
}

#[async_trait]
impl ProtocolPackageCompilerPort for FakeProtocolPackageServices {
    async fn validate_for_enable(
        &self,
        package: &ProtocolPackageRef,
    ) -> AppResult<ProtocolPackageCompilationReceipt> {
        self.compile_calls.fetch_add(1, Ordering::SeqCst);
        self.record_call(package);
        if let Some(error) = self.failures.lock().compile.clone() {
            return Err(error);
        }
        if let Some(result) = self.compilation_results.lock().get(package).cloned() {
            return result;
        }
        let record = self
            .records
            .lock()
            .get(package)
            .cloned()
            .ok_or_else(|| AppError::new("PROTOCOL_PACKAGE_NOT_FOUND", "测试记录不存在。"))?;
        Ok(ProtocolPackageCompilationReceipt {
            package: package.clone(),
            host_api: record.host_api,
            compatible: true,
        })
    }

    async fn describe(
        &self,
        package: &ProtocolPackageRef,
    ) -> AppResult<ProtocolPackageDescriptionViewModel> {
        self.describe_calls.fetch_add(1, Ordering::SeqCst);
        self.record_call(package);
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

pub(super) fn package(id: &str, version: &str) -> ProtocolPackageRef {
    ProtocolPackageRef {
        id: ProtocolPackageId::new(id).unwrap(),
        version: ProtocolPackageVersion::new(version).unwrap(),
    }
}

pub(super) fn record(
    package: ProtocolPackageRef,
    enabled: bool,
) -> ProtocolPackageVersionViewModel {
    ProtocolPackageVersionViewModel {
        name: format!("{} {}", package.id, package.version),
        package,
        host_api: 1,
        enabled,
        validation: ProtocolPackageValidationViewModel::Valid,
        installed_at: Utc.timestamp_opt(1_700_000_000, 0).single().unwrap(),
    }
}

pub(super) fn usage(
    workspace_id: WorkspaceId,
    listener_id: ListenerId,
    runtime_state: ListenerRuntimeState,
) -> ProtocolPackageUsageViewModel {
    ProtocolPackageUsageViewModel {
        workspace_id,
        workspace_name: format!("Workspace {workspace_id}"),
        listener_id,
        listener_name: format!("Listener {listener_id}"),
        listener_enabled: runtime_state != ListenerRuntimeState::Stopped,
        runtime_state,
    }
}

pub(super) fn description(package: ProtocolPackageRef) -> ProtocolPackageDescriptionViewModel {
    ProtocolPackageDescriptionViewModel {
        package,
        capabilities: ProtocolPackageCapabilitiesViewModel {
            upstream: ProtocolPackageDirectionCapabilitiesViewModel {
                frame: true,
                decode: true,
                encode: true,
            },
            downstream: ProtocolPackageDirectionCapabilitiesViewModel {
                frame: true,
                decode: true,
                encode: false,
            },
            display: true,
        },
        schema: ProtocolPackageSchemaViewModel {
            id: "payments".into(),
            version: 1,
            title: "Payments".into(),
            fields: [
                (
                    "trace_id",
                    ProtocolPackageSchemaFieldTypeViewModel::String,
                    "Trace ID",
                ),
                (
                    "amount",
                    ProtocolPackageSchemaFieldTypeViewModel::Int,
                    "Amount",
                ),
                (
                    "approved",
                    ProtocolPackageSchemaFieldTypeViewModel::Bool,
                    "Approved",
                ),
            ]
            .into_iter()
            .map(
                |(name, field_type, label)| ProtocolPackageSchemaFieldViewModel {
                    name: name.into(),
                    label: label.into(),
                    field_type,
                },
            )
            .collect(),
        },
    }
}

pub(super) fn application(
    services: Arc<FakeProtocolPackageServices>,
    workspaces: Arc<InMemoryWorkspaceStore>,
    runtime: Arc<InMemoryListenerRuntime>,
) -> Application {
    let ports = Arc::new(FakePorts::default());
    application_with_proxy_ports(services, workspaces, runtime, ports)
}

pub(super) fn application_with_proxy_ports(
    services: Arc<FakeProtocolPackageServices>,
    workspaces: Arc<InMemoryWorkspaceStore>,
    runtime: Arc<InMemoryListenerRuntime>,
    ports: Arc<FakePorts>,
) -> Application {
    Application::new(
        "Protocol lifecycle test".into(),
        ApplicationDependencies {
            proxy: ports.clone(),
            capture: ports.clone(),
            sessions: ports.clone(),
            breakpoints: Arc::new(BreakpointCoordinator::default()),
            breakpoint_validation: ports.clone(),
            rules: ports.clone(),
            faults: ports.clone(),
            certificates: ports.clone(),
            settings: ports.clone(),
            workspaces,
            workspace_documents: Arc::new(InMemoryWorkspaceDocumentStore::default()),
            listener_runtime: runtime,
            listener_certificates: ports,
            protocol_packages: ProtocolPackageApplicationServices {
                store: services.clone(),
                compiler: services.clone(),
                importer: services.clone(),
                usage_query: services.clone(),
                portability: services,
            },
            events: Arc::new(EventHub::default()),
        },
    )
}
pub(super) fn error_code(error: &AppError) -> &str {
    &error.view_model.code
}
