use std::sync::Arc;

use async_trait::async_trait;
use intercept_proxy_application::{
    AppError, AppResult, ApplicationBundleReplacementPort, ApplicationConfigurationDocument,
    PortableApplicationProtocolPackage,
};
use intercept_proxy_package_runtime::read_package_component;

use super::{
    ExternalPackageRegistryAdapter,
    application_port::{decode_portable_component, unsupported_remote_backup},
};
use crate::{
    adapters::{WorkspaceRepositoryAdapter, common::app_error, settings::serialize_settings},
    sqlite::external_packages::LocalApplicationPackageRecord,
};

#[derive(Clone, Debug)]
pub struct ApplicationBundleReplacementAdapter {
    registry: Arc<ExternalPackageRegistryAdapter>,
}

impl ApplicationBundleReplacementAdapter {
    pub fn new(registry: Arc<ExternalPackageRegistryAdapter>) -> Self {
        Self { registry }
    }
}

#[async_trait]
impl ApplicationBundleReplacementPort for ApplicationBundleReplacementAdapter {
    async fn replace_application_bundle(
        &self,
        packages: Vec<PortableApplicationProtocolPackage>,
        document: ApplicationConfigurationDocument,
    ) -> AppResult<()> {
        document.validate()?;
        let records = document
            .workspaces
            .iter()
            .map(WorkspaceRepositoryAdapter::record)
            .collect::<AppResult<Vec<_>>>()?;
        let settings = serialize_settings(&document.settings.to_draft(None)).map_err(|error| {
            AppError::new(
                "APPLICATION_CONFIGURATION_INVALID",
                format!("完整配置中的 Settings 无法持久化：{error}"),
            )
        })?;
        let selected_id = document.selected_workspace_id.as_uuid();
        let mut stored_packages = Vec::with_capacity(packages.len());
        let mut enabled_runtimes = Vec::new();
        for package in packages {
            let bytes = decode_portable_component(&package)?;
            let component = read_package_component(&bytes).map_err(AppError::from)?;
            if component.manifest().package().identity() != package.package {
                return Err(AppError::new(
                    "PROTOCOL_PACKAGE_IDENTITY_CONFLICT",
                    "备份声明身份与 component.wasm 内嵌身份不一致。",
                )
                .entity(format!(
                    "{}@{}",
                    package.package.id.as_str(),
                    package.package.version.as_str()
                )));
            }
            let runtime =
                ExternalPackageRegistryAdapter::load_local_component(&package.package, &bytes)
                    .await?;
            stored_packages.push(LocalApplicationPackageRecord {
                registration: component.manifest().clone(),
                archive: bytes,
                enabled: package.enabled,
            });
            if package.enabled {
                enabled_runtimes.push((package.package, runtime));
            }
        }
        let _bundle_mutation = self.registry.application_bundle_mutation.lock().await;
        if !self.registry.online.lock().is_empty() {
            return Err(unsupported_remote_backup());
        }
        self.registry
            .executor
            .execute(move |store| {
                store.replace_application_bundle(selected_id, &records, &settings, &stored_packages)
            })
            .await
            .map_err(app_error)?;
        self.registry.reconcile_local_runtimes(enabled_runtimes);
        self.registry.publish_service_status();
        Ok(())
    }

    async fn reset_application_bundle(
        &self,
        document: ApplicationConfigurationDocument,
    ) -> AppResult<()> {
        document.validate()?;
        let records = document
            .workspaces
            .iter()
            .map(WorkspaceRepositoryAdapter::record)
            .collect::<AppResult<Vec<_>>>()?;
        let settings = serialize_settings(&document.settings.to_draft(None)).map_err(|error| {
            AppError::new(
                "APPLICATION_CONFIGURATION_INVALID",
                format!("默认 Settings 无法持久化：{error}"),
            )
        })?;
        let selected_id = document.selected_workspace_id.as_uuid();
        let _bundle_mutation = self.registry.application_bundle_mutation.lock().await;
        if !self.registry.online.lock().is_empty() {
            return Err(AppError::new(
                "APPLICATION_RESET_REMOTE_PACKAGES_ONLINE",
                "远端调试软件包仍在线，无法原子清除应用数据。",
            ));
        }
        self.registry
            .executor
            .execute(move |store| store.reset_application_data(selected_id, &records, &settings))
            .await
            .map_err(app_error)?;
        self.registry.deactivate_all_local_components();
        self.registry.publish_service_status();
        Ok(())
    }
}
