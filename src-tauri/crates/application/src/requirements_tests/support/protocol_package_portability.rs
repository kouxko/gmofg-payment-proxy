use super::*;
use std::collections::HashMap;

#[derive(Debug)]
pub(in crate::requirements_tests) struct FakeProtocolPackagePortability {
    configuration_store: Arc<dyn ApplicationConfigurationStorePort>,
    pub(in crate::requirements_tests) descriptions:
        parking_lot::Mutex<HashMap<ProtocolPackageRef, ProtocolPackageDescriptionViewModel>>,
    pub(in crate::requirements_tests) application_packages:
        parking_lot::Mutex<Vec<PortableApplicationProtocolPackage>>,
    pub(in crate::requirements_tests) preflight_calls: AtomicUsize,
    pub(in crate::requirements_tests) installed_preflight_calls: AtomicUsize,
    pub(in crate::requirements_tests) replace_calls: AtomicUsize,
    pub(in crate::requirements_tests) reset_calls: AtomicUsize,
    pub(in crate::requirements_tests) compiler_validate_calls: AtomicUsize,
    pub(in crate::requirements_tests) compiler_describe_calls: AtomicUsize,
    pub(in crate::requirements_tests) fail_commit: AtomicBool,
    pub(in crate::requirements_tests) fail_preflight: AtomicBool,
    pub(in crate::requirements_tests) fail_preflight_at: parking_lot::Mutex<Option<usize>>,
    pub(in crate::requirements_tests) block_preflight: AtomicBool,
    pub(in crate::requirements_tests) preflight_entered: tokio::sync::Notify,
    pub(in crate::requirements_tests) continue_preflight: tokio::sync::Notify,
    pub(in crate::requirements_tests) block_backup_baseline: AtomicBool,
    pub(in crate::requirements_tests) backup_baseline_entered: tokio::sync::Notify,
    pub(in crate::requirements_tests) continue_backup_baseline: tokio::sync::Notify,
    pub(in crate::requirements_tests) block_application_export: AtomicBool,
    pub(in crate::requirements_tests) application_export_entered: tokio::sync::Notify,
    pub(in crate::requirements_tests) continue_application_export: tokio::sync::Notify,
}

impl FakeProtocolPackagePortability {
    pub(in crate::requirements_tests) fn new(
        configuration_store: Arc<dyn ApplicationConfigurationStorePort>,
    ) -> Self {
        Self {
            configuration_store,
            descriptions: parking_lot::Mutex::new(HashMap::new()),
            application_packages: parking_lot::Mutex::new(Vec::new()),
            preflight_calls: AtomicUsize::new(0),
            installed_preflight_calls: AtomicUsize::new(0),
            replace_calls: AtomicUsize::new(0),
            reset_calls: AtomicUsize::new(0),
            compiler_validate_calls: AtomicUsize::new(0),
            compiler_describe_calls: AtomicUsize::new(0),
            fail_commit: AtomicBool::new(false),
            fail_preflight: AtomicBool::new(false),
            fail_preflight_at: parking_lot::Mutex::new(None),
            block_preflight: AtomicBool::new(false),
            preflight_entered: tokio::sync::Notify::new(),
            continue_preflight: tokio::sync::Notify::new(),
            block_backup_baseline: AtomicBool::new(false),
            backup_baseline_entered: tokio::sync::Notify::new(),
            continue_backup_baseline: tokio::sync::Notify::new(),
            block_application_export: AtomicBool::new(false),
            application_export_entered: tokio::sync::Notify::new(),
            continue_application_export: tokio::sync::Notify::new(),
        }
    }

    pub(in crate::requirements_tests) fn register(
        &self,
        package: PortableApplicationProtocolPackage,
        description: ProtocolPackageDescriptionViewModel,
    ) {
        self.descriptions
            .lock()
            .insert(package.package.clone(), description);
        self.application_packages.lock().push(package);
    }

    fn descriptions_for<T>(
        &self,
        packages: &[T],
        identity: impl Fn(&T) -> &ProtocolPackageRef,
    ) -> AppResult<Vec<ProtocolPackageDescriptionViewModel>> {
        if self.fail_preflight.load(Ordering::SeqCst) {
            return Err(AppError::new(
                "SCRIPT_SYNTAX_INVALID",
                "测试注入：协议包预检失败。",
            ));
        }
        let descriptions = self.descriptions.lock();
        let fail_at = *self.fail_preflight_at.lock();
        packages
            .iter()
            .enumerate()
            .map(|(index, package)| {
                if fail_at == Some(index) {
                    return Err(AppError::new(
                        "SCRIPT_SYNTAX_INVALID",
                        "测试注入：指定协议包预检失败。",
                    ));
                }
                descriptions.get(identity(package)).cloned().ok_or_else(|| {
                    AppError::new("PROTOCOL_PACKAGE_NOT_FOUND", "测试协议包没有对应编译描述。")
                })
            })
            .collect()
    }
}

#[async_trait]
impl ProtocolPackageStorePort for FakeProtocolPackagePortability {
    async fn list(&self) -> AppResult<Vec<ProtocolPackageVersionViewModel>> {
        Ok(Vec::new())
    }

    async fn get(
        &self,
        _: &ProtocolPackageRef,
    ) -> AppResult<Option<ProtocolPackageVersionViewModel>> {
        Ok(None)
    }

    async fn set_enabled(&self, _: &ProtocolPackageRef, _: bool) -> AppResult<()> {
        Err(AppError::new("TEST_READ_ONLY", "测试替身只读。"))
    }

    async fn delete(&self, _: &ProtocolPackageRef) -> AppResult<()> {
        Err(AppError::new("TEST_READ_ONLY", "测试替身只读。"))
    }
}

#[async_trait]
impl ProtocolPackageCompilerPort for FakeProtocolPackagePortability {
    async fn compile_fresh(
        &self,
        package: &ProtocolPackageRef,
    ) -> AppResult<ProtocolPackageCompilationReceipt> {
        self.compiler_validate_calls.fetch_add(1, Ordering::SeqCst);
        let description = self
            .descriptions
            .lock()
            .get(package)
            .cloned()
            .ok_or_else(|| {
                AppError::new("PROTOCOL_PACKAGE_NOT_FOUND", "测试协议包未安装或无法恢复。")
            })?;
        Ok(ProtocolPackageCompilationReceipt {
            package: description.package,
            host_api: 1,
            compatible: true,
        })
    }

    async fn describe(
        &self,
        package: &ProtocolPackageRef,
    ) -> AppResult<ProtocolPackageDescriptionViewModel> {
        self.compiler_describe_calls.fetch_add(1, Ordering::SeqCst);
        self.descriptions
            .lock()
            .get(package)
            .cloned()
            .ok_or_else(|| {
                AppError::new("PROTOCOL_PACKAGE_NOT_FOUND", "测试协议包没有对应编译描述。")
            })
    }
}

#[async_trait]
impl ProtocolPackageImportPort for FakeProtocolPackagePortability {
    async fn prepare_zip(&self) -> AppResult<Option<ProtocolPackageImportPreviewViewModel>> {
        unused()
    }

    async fn commit_zip(
        &self,
        _: ProtocolPackageImportToken,
    ) -> AppResult<ProtocolPackageImportViewModel> {
        unused()
    }

    async fn discard_zip(&self, _: ProtocolPackageImportToken) -> AppResult<()> {
        unused()
    }
}

#[async_trait]
impl BuiltinProtocolPackagePort for FakeProtocolPackagePortability {
    async fn restore_builtin(&self) -> AppResult<ProtocolPackageImportViewModel> {
        unused()
    }
}

#[async_trait]
impl ProtocolPackageUsageQueryPort for FakeProtocolPackagePortability {
    async fn usages(
        &self,
        _: &ProtocolPackageRef,
    ) -> AppResult<Vec<ProtocolPackageUsageViewModel>> {
        Ok(Vec::new())
    }

    async fn usage_counts(&self) -> AppResult<Vec<ProtocolPackageUsageCount>> {
        Ok(Vec::new())
    }
}

#[async_trait]
impl ProtocolPackagePortabilityPort for FakeProtocolPackagePortability {
    async fn application_backup_baseline(
        &self,
    ) -> AppResult<Vec<ApplicationBackupProtocolPackageBaseline>> {
        if self.block_backup_baseline.load(Ordering::SeqCst) {
            self.backup_baseline_entered.notify_one();
            self.continue_backup_baseline.notified().await;
        }
        Ok(self
            .application_packages
            .lock()
            .iter()
            .map(|package| ApplicationBackupProtocolPackageBaseline {
                package: package.package.clone(),
                enabled: package.enabled,
                generation: uuid::Uuid::nil(),
            })
            .collect())
    }

    async fn export_application_packages(
        &self,
    ) -> AppResult<Vec<PortableApplicationProtocolPackage>> {
        if self.block_application_export.load(Ordering::SeqCst) {
            self.application_export_entered.notify_one();
            self.continue_application_export.notified().await;
        }
        Ok(self.application_packages.lock().clone())
    }

    async fn preflight_application_packages(
        &self,
        packages: &[PortableApplicationProtocolPackage],
    ) -> AppResult<Vec<ProtocolPackageDescriptionViewModel>> {
        self.preflight_calls.fetch_add(1, Ordering::SeqCst);
        if self.block_preflight.load(Ordering::SeqCst) {
            self.preflight_entered.notify_one();
            self.continue_preflight.notified().await;
        }
        self.descriptions_for(packages, |package| &package.package)
    }

    async fn preflight_installed_packages(
        &self,
        packages: &[ProtocolPackageRef],
    ) -> AppResult<Vec<ProtocolPackageDescriptionViewModel>> {
        self.installed_preflight_calls
            .fetch_add(1, Ordering::SeqCst);
        self.descriptions_for(packages, |package| package)
    }

    async fn replace_application_bundle(
        &self,
        _: Vec<PortableApplicationProtocolPackage>,
        document: ApplicationConfigurationDocument,
    ) -> AppResult<()> {
        self.replace_calls.fetch_add(1, Ordering::SeqCst);
        if self.fail_commit.load(Ordering::SeqCst) {
            return Err(AppError::new(
                "ATOMIC_COMMIT_FAILED",
                "测试注入：替换失败。",
            ));
        }
        self.configuration_store.replace_all(document).await
    }

    async fn reset_application_bundle(
        &self,
        document: ApplicationConfigurationDocument,
    ) -> AppResult<()> {
        self.reset_calls.fetch_add(1, Ordering::SeqCst);
        self.configuration_store.reset_all(document).await
    }
}
