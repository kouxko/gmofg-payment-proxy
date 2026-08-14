use super::*;
use std::collections::HashMap;

#[derive(Debug)]
pub(in crate::requirements_tests) struct FakeProtocolPackagePortability {
    workspaces: Arc<dyn WorkspaceRepositoryPort>,
    configuration_store: Arc<dyn ApplicationConfigurationStorePort>,
    pub(in crate::requirements_tests) descriptions:
        parking_lot::Mutex<HashMap<ProtocolPackageRef, ProtocolPackageDescriptionViewModel>>,
    pub(in crate::requirements_tests) workspace_packages:
        parking_lot::Mutex<HashMap<ProtocolPackageRef, PortableProtocolPackage>>,
    pub(in crate::requirements_tests) application_packages:
        parking_lot::Mutex<Vec<PortableApplicationProtocolPackage>>,
    pub(in crate::requirements_tests) preflight_calls: AtomicUsize,
    pub(in crate::requirements_tests) installed_preflight_calls: AtomicUsize,
    pub(in crate::requirements_tests) commit_calls: AtomicUsize,
    pub(in crate::requirements_tests) legacy_commit_calls: AtomicUsize,
    pub(in crate::requirements_tests) replace_calls: AtomicUsize,
    pub(in crate::requirements_tests) legacy_replace_calls: AtomicUsize,
    pub(in crate::requirements_tests) reset_calls: AtomicUsize,
    pub(in crate::requirements_tests) compiler_validate_calls: AtomicUsize,
    pub(in crate::requirements_tests) compiler_describe_calls: AtomicUsize,
    pub(in crate::requirements_tests) fail_commit: AtomicBool,
    pub(in crate::requirements_tests) fail_preflight: AtomicBool,
    pub(in crate::requirements_tests) block_workspace_export: AtomicBool,
    pub(in crate::requirements_tests) workspace_export_entered: tokio::sync::Notify,
    pub(in crate::requirements_tests) continue_workspace_export: tokio::sync::Notify,
    pub(in crate::requirements_tests) block_application_export: AtomicBool,
    pub(in crate::requirements_tests) application_export_entered: tokio::sync::Notify,
    pub(in crate::requirements_tests) continue_application_export: tokio::sync::Notify,
}

impl FakeProtocolPackagePortability {
    pub(in crate::requirements_tests) fn new(
        workspaces: Arc<dyn WorkspaceRepositoryPort>,
        configuration_store: Arc<dyn ApplicationConfigurationStorePort>,
    ) -> Self {
        Self {
            workspaces,
            configuration_store,
            descriptions: parking_lot::Mutex::new(HashMap::new()),
            workspace_packages: parking_lot::Mutex::new(HashMap::new()),
            application_packages: parking_lot::Mutex::new(Vec::new()),
            preflight_calls: AtomicUsize::new(0),
            installed_preflight_calls: AtomicUsize::new(0),
            commit_calls: AtomicUsize::new(0),
            legacy_commit_calls: AtomicUsize::new(0),
            replace_calls: AtomicUsize::new(0),
            legacy_replace_calls: AtomicUsize::new(0),
            reset_calls: AtomicUsize::new(0),
            compiler_validate_calls: AtomicUsize::new(0),
            compiler_describe_calls: AtomicUsize::new(0),
            fail_commit: AtomicBool::new(false),
            fail_preflight: AtomicBool::new(false),
            block_workspace_export: AtomicBool::new(false),
            workspace_export_entered: tokio::sync::Notify::new(),
            continue_workspace_export: tokio::sync::Notify::new(),
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
        self.workspace_packages.lock().insert(
            package.package.clone(),
            PortableProtocolPackage {
                package: package.package.clone(),
                files: package.files.clone(),
            },
        );
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
        packages
            .iter()
            .map(|package| {
                descriptions.get(identity(package)).cloned().ok_or_else(|| {
                    AppError::new("PROTOCOL_PACKAGE_NOT_FOUND", "测试协议包没有对应编译描述。")
                })
            })
            .collect()
    }
}

#[async_trait]
impl ProtocolPackageCompilerPort for FakeProtocolPackagePortability {
    async fn validate_for_enable(
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
impl ProtocolPackagePortabilityPort for FakeProtocolPackagePortability {
    async fn export_workspace_packages(
        &self,
        packages: &[ProtocolPackageRef],
    ) -> AppResult<Vec<PortableProtocolPackage>> {
        if self.block_workspace_export.load(Ordering::SeqCst) {
            self.workspace_export_entered.notify_one();
            self.continue_workspace_export.notified().await;
        }
        let registry = self.workspace_packages.lock();
        packages
            .iter()
            .map(|package| {
                registry.get(package).cloned().ok_or_else(|| {
                    AppError::new(
                        "PROTOCOL_PACKAGE_NOT_FOUND",
                        "Workspace 引用的测试协议包未安装。",
                    )
                })
            })
            .collect()
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

    async fn preflight_workspace_packages(
        &self,
        packages: &[PortableProtocolPackage],
    ) -> AppResult<Vec<ProtocolPackageDescriptionViewModel>> {
        self.preflight_calls.fetch_add(1, Ordering::SeqCst);
        self.descriptions_for(packages, |package| &package.package)
    }

    async fn preflight_application_packages(
        &self,
        packages: &[PortableApplicationProtocolPackage],
    ) -> AppResult<Vec<ProtocolPackageDescriptionViewModel>> {
        self.preflight_calls.fetch_add(1, Ordering::SeqCst);
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

    async fn commit_workspace_bundle(
        &self,
        _: Vec<PortableProtocolPackage>,
        workspace: ProxyWorkspace,
    ) -> AppResult<()> {
        self.commit_calls.fetch_add(1, Ordering::SeqCst);
        if self.fail_commit.load(Ordering::SeqCst) {
            return Err(AppError::new(
                "ATOMIC_COMMIT_FAILED",
                "测试注入：提交失败。",
            ));
        }
        // 内存 Workspace fake 仍会重映射一次；生产 adapter 接收的是 facade 已重映射的聚合。
        // 需求测试只用它观察是否发生写入，不依赖导入结果的具体 UUID。
        self.workspaces.import_workspace(workspace).await?;
        Ok(())
    }

    async fn commit_legacy_workspace(&self, workspace: ProxyWorkspace) -> AppResult<()> {
        self.legacy_commit_calls.fetch_add(1, Ordering::SeqCst);
        if self.fail_commit.load(Ordering::SeqCst) {
            return Err(AppError::new(
                "ATOMIC_COMMIT_FAILED",
                "测试注入：历史 Workspace 提交失败。",
            ));
        }
        self.workspaces.import_workspace(workspace).await?;
        Ok(())
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

    async fn replace_legacy_application_configuration(
        &self,
        mut document: ApplicationConfigurationDocument,
    ) -> AppResult<()> {
        self.legacy_replace_calls.fetch_add(1, Ordering::SeqCst);
        if self.fail_commit.load(Ordering::SeqCst) {
            return Err(AppError::new(
                "ATOMIC_COMMIT_FAILED",
                "测试注入：历史完整配置替换失败。",
            ));
        }
        // 生产事务保留 registry；测试 store 用完整文档记录这一可观察语义。
        document.protocol_packages = self.application_packages.lock().clone();
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
