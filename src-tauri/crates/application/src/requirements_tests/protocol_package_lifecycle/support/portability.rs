use super::*;

#[async_trait]
impl ProtocolPackagePortabilityPort for FakeProtocolPackageServices {
    async fn export_workspace_packages(
        &self,
        packages: &[ProtocolPackageRef],
    ) -> AppResult<Vec<PortableProtocolPackage>> {
        self.workspace_export_calls.fetch_add(1, Ordering::SeqCst);
        *self.exported_workspace_refs.lock() = packages.to_vec();
        let records = self.records.lock();
        packages
            .iter()
            .map(|package| {
                records.get(package).ok_or_else(|| {
                    AppError::new("PROTOCOL_PACKAGE_NOT_FOUND", "测试记录不存在。")
                })?;
                Ok(PortableProtocolPackage {
                    package: package.clone(),
                    files: vec![portable_file()],
                })
            })
            .collect()
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
                files: vec![portable_file()],
                enabled: record.enabled,
            })
            .collect())
    }

    async fn preflight_workspace_packages(
        &self,
        packages: &[PortableProtocolPackage],
    ) -> AppResult<Vec<ProtocolPackageDescriptionViewModel>> {
        self.preflight(packages, |package| &package.package)
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
        if self.block_installed_preflight.load(Ordering::SeqCst) {
            self.installed_preflight_entered.notify_one();
            self.continue_installed_preflight.notified().await;
        }
        if let Some(error) = self.failures.lock().installed_preflight.clone() {
            return Err(error);
        }
        self.preflight(packages, |package| package)
    }

    async fn commit_workspace_bundle(
        &self,
        _: Vec<PortableProtocolPackage>,
        _: ProxyWorkspace,
    ) -> AppResult<()> {
        unused()
    }

    async fn commit_legacy_workspace(&self, _: ProxyWorkspace) -> AppResult<()> {
        unused()
    }

    async fn replace_application_bundle(
        &self,
        _: Vec<PortableApplicationProtocolPackage>,
        _: ApplicationConfigurationDocument,
    ) -> AppResult<()> {
        unused()
    }

    async fn replace_legacy_application_configuration(
        &self,
        _: ApplicationConfigurationDocument,
    ) -> AppResult<()> {
        unused()
    }

    async fn reset_application_bundle(&self, _: ApplicationConfigurationDocument) -> AppResult<()> {
        unused()
    }
}

impl FakeProtocolPackageServices {
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

fn portable_file() -> PortableProtocolPackageFile {
    PortableProtocolPackageFile {
        path: "manifest.toml".into(),
        contents_base64: "bWFuaWZlc3Q=".into(),
    }
}
