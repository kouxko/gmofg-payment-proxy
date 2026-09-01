//! 外部协议包注册表到 Application 生命周期与详情端口的适配。

use async_trait::async_trait;
use base64::{Engine as _, engine::general_purpose::STANDARD};
use intercept_proxy_application::{
    AppError, AppResult, ApplicationBackupProtocolPackageBaseline,
    ApplicationConfigurationDocument, ExternalPackageApplicationPort,
    ExternalPackageDetailViewModel, ExternalPackageServiceStatusViewModel, PORTABLE_COMPONENT_PATH,
    PortableApplicationProtocolPackage, PortableProtocolPackageFile,
    ProtocolPackageDescriptionViewModel, ProtocolPackageVersionViewModel,
};
use intercept_proxy_domain::ProtocolPackageRef;
use intercept_proxy_package_runtime::read_package_component;
use uuid::Uuid;

use super::{
    ExternalPackageConnectionId, ExternalPackageRegistryAdapter, OnlineConnection, app_error,
    not_found,
    views::{application_description, application_detail, application_summary},
};
use crate::adapters::{WorkspaceRepositoryAdapter, settings::serialize_settings};
use crate::sqlite::external_packages::LocalApplicationPackageRecord;

fn unsupported_remote_backup() -> AppError {
    AppError::new(
        "APPLICATION_BACKUP_REMOTE_PACKAGES_UNSUPPORTED",
        "远端调试软件包不属于应用托管资产，不能写入或替换应用备份。",
    )
}

fn component_generation(bytes: &[u8]) -> Uuid {
    let digest = ring::digest::digest(&ring::digest::SHA256, bytes);
    let mut generation = [0_u8; 16];
    generation.copy_from_slice(&digest.as_ref()[..16]);
    Uuid::from_bytes(generation)
}

fn sort_stored_packages(records: &mut [crate::sqlite::external_packages::StoredExternalPackage]) {
    records.sort_by(|left, right| {
        let left = left.registration.package().identity();
        let right = right.registration.package().identity();
        left.id
            .as_str()
            .cmp(right.id.as_str())
            .then_with(|| left.version.semantic_cmp(&right.version))
            .then_with(|| left.version.as_str().cmp(right.version.as_str()))
    });
}

fn decode_portable_component(package: &PortableApplicationProtocolPackage) -> AppResult<Vec<u8>> {
    let [
        PortableProtocolPackageFile {
            path,
            contents_base64,
        },
    ] = package.files.as_slice()
    else {
        return Err(AppError::new(
            "PORTABLE_PROTOCOL_PACKAGE_INVALID",
            "协议包必须且只能包含 component.wasm。",
        ));
    };
    if path != PORTABLE_COMPONENT_PATH {
        return Err(AppError::new(
            "PORTABLE_PROTOCOL_PACKAGE_INVALID",
            "协议包必须且只能包含 component.wasm。",
        ));
    }
    let bytes = STANDARD.decode(contents_base64).map_err(|_| {
        AppError::new(
            "PORTABLE_PROTOCOL_PACKAGE_INVALID",
            "component.wasm 不是有效的标准 Base64。",
        )
    })?;
    if STANDARD.encode(&bytes) != *contents_base64 {
        return Err(AppError::new(
            "PORTABLE_PROTOCOL_PACKAGE_INVALID",
            "component.wasm 必须使用规范的标准 Base64。",
        ));
    }
    Ok(bytes)
}

#[async_trait]
impl ExternalPackageApplicationPort for ExternalPackageRegistryAdapter {
    async fn service_status(&self) -> AppResult<ExternalPackageServiceStatusViewModel> {
        Ok(self.service_status_snapshot())
    }

    async fn list(&self) -> AppResult<Vec<ProtocolPackageVersionViewModel>> {
        self.executor
            .execute(crate::SqliteStore::list_external_packages)
            .await
            .map_err(app_error)
            .map(|mut records| {
                sort_stored_packages(&mut records);
                records
                    .into_iter()
                    .map(|record| {
                        let online = self.is_online(&record.registration.package().identity());
                        application_summary(&record, online)
                    })
                    .collect()
            })
    }

    async fn get(
        &self,
        package: &ProtocolPackageRef,
    ) -> AppResult<Option<ProtocolPackageVersionViewModel>> {
        let selected = package.clone();
        self.executor
            .execute(move |store| store.get_external_package(&selected))
            .await
            .map_err(app_error)
            .map(|record| {
                record.map(|record| application_summary(&record, self.is_online(package)))
            })
    }

    async fn describe(
        &self,
        package: &ProtocolPackageRef,
    ) -> AppResult<ProtocolPackageDescriptionViewModel> {
        let selected = package.clone();
        let stored = self
            .executor
            .execute(move |store| store.get_external_package(&selected))
            .await
            .map_err(app_error)?
            .ok_or_else(|| not_found(package))?;
        Ok(application_description(&stored.registration))
    }

    async fn detail(
        &self,
        package: &ProtocolPackageRef,
    ) -> AppResult<ExternalPackageDetailViewModel> {
        let selected = package.clone();
        let stored = self
            .executor
            .execute(move |store| store.get_external_package(&selected))
            .await
            .map_err(app_error)?
            .ok_or_else(|| not_found(package))?;
        let detail = self.connection_details.lock().get(package).cloned();
        Ok(application_detail(
            &stored,
            detail.as_ref(),
            self.is_online(package),
        ))
    }

    async fn set_enabled(&self, package: &ProtocolPackageRef, enabled: bool) -> AppResult<()> {
        if let Some(component) = self.local_archive(package).await? {
            return if self
                .set_local_component_enabled(package, &component, enabled)
                .await?
            {
                Ok(())
            } else {
                Err(not_found(package))
            };
        }
        let environment_apply_gate = self.acquire_environment_apply_package_gate(package).await;
        let selected = package.clone();
        if self
            .executor
            .execute(move |store| store.set_external_package_enabled(&selected, enabled))
            .await
            .map_err(app_error)?
        {
            drop(environment_apply_gate);
            self.publish_catalog_changed(package);
            Ok(())
        } else {
            Err(not_found(package))
        }
    }

    async fn disconnect(&self, package: &ProtocolPackageRef) -> AppResult<()> {
        if self.deactivate_local_component(package) {
            return Ok(());
        }
        let environment_apply_gate = self.acquire_environment_apply_package_gate(package).await;
        let mut completion = self
            .begin_disconnect(package, Some(environment_apply_gate))
            .await;
        Self::wait_for_closing(&mut completion).await;
        Ok(())
    }

    async fn restart(&self, package: &ProtocolPackageRef) -> AppResult<()> {
        let component = self.local_archive(package).await?.ok_or_else(|| {
            AppError::new(
                "EXTERNAL_PACKAGE_RESTART_UNAVAILABLE",
                "远端外部软件包连接不由 Proxy 重启。",
            )
        })?;
        self.restart_local_component(package, &component).await
    }

    async fn delete(&self, package: &ProtocolPackageRef) -> AppResult<()> {
        self.remove_local_component(package);
        let environment_apply_gate = self.acquire_environment_apply_package_gate(package).await;
        let registry = self.clone();
        let package = package.clone();
        let cleanup = tokio::spawn(async move {
            loop {
                let mut closing = registry.begin_disconnect(&package, None).await;
                Self::wait_for_closing(&mut closing).await;

                let gate = registry.connection_mutation(&package);
                let mutation = gate.lock().await;
                if registry.online.lock().contains_key(&package) {
                    drop(mutation);
                    continue;
                }

                let deletion_id = ExternalPackageConnectionId(Uuid::new_v4());
                let (completed, completion) = tokio::sync::watch::channel(false);
                registry.online.lock().insert(
                    package.clone(),
                    OnlineConnection::Closing {
                        id: deletion_id,
                        completion,
                    },
                );
                drop(mutation);
                let selected = package.clone();
                let deletion = registry
                    .executor
                    .execute(move |store| store.delete_external_package(&selected))
                    .await
                    .map_err(app_error);
                let mutation = gate.lock().await;
                let mut online = registry.online.lock();
                if matches!(
                    online.get(&package),
                    Some(OnlineConnection::Closing { id, .. }) if *id == deletion_id
                ) {
                    online.remove(&package);
                }
                drop(online);
                drop(mutation);
                let _ = completed.send(true);
                registry.publish_catalog_changed(&package);
                registry.publish_service_status();
                #[cfg(test)]
                registry.deletion_complete.notify_one();
                drop(environment_apply_gate);
                return deletion.map(|_| ());
            }
        });
        cleanup.await.map_err(|error| {
            AppError::new(
                "INTERNAL_ERROR",
                format!("外部协议包删除任务异常终止：{error}"),
            )
        })?
    }

    async fn application_backup_baseline(
        &self,
    ) -> AppResult<Vec<ApplicationBackupProtocolPackageBaseline>> {
        let mut records = self
            .executor
            .execute(crate::SqliteStore::list_external_packages)
            .await
            .map_err(app_error)?;
        sort_stored_packages(&mut records);
        records
            .into_iter()
            .map(|record| {
                let archive = record.local_archive.ok_or_else(unsupported_remote_backup)?;
                Ok(ApplicationBackupProtocolPackageBaseline {
                    package: record.registration.package().identity(),
                    enabled: record.enabled,
                    generation: component_generation(&archive),
                })
            })
            .collect()
    }

    async fn export_application_packages(
        &self,
    ) -> AppResult<Vec<PortableApplicationProtocolPackage>> {
        let mut records = self
            .executor
            .execute(crate::SqliteStore::list_external_packages)
            .await
            .map_err(app_error)?;
        sort_stored_packages(&mut records);
        records
            .into_iter()
            .map(|record| {
                let archive = record.local_archive.ok_or_else(unsupported_remote_backup)?;
                Ok(PortableApplicationProtocolPackage {
                    package: record.registration.package().identity(),
                    files: vec![PortableProtocolPackageFile {
                        path: PORTABLE_COMPONENT_PATH.to_owned(),
                        contents_base64: STANDARD.encode(archive),
                    }],
                    enabled: record.enabled,
                })
            })
            .collect()
    }

    async fn preflight_application_packages(
        &self,
        packages: &[PortableApplicationProtocolPackage],
    ) -> AppResult<Vec<ProtocolPackageDescriptionViewModel>> {
        let mut descriptions = Vec::with_capacity(packages.len());
        for package in packages {
            let bytes = decode_portable_component(package)?;
            let component = read_package_component(&bytes)
                .map_err(intercept_proxy_application::AppError::from)?;
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
            Self::load_local_component(&package.package, &bytes).await?;
            descriptions.push(application_description(component.manifest()));
        }
        Ok(descriptions)
    }

    async fn preflight_installed_packages(
        &self,
        packages: &[ProtocolPackageRef],
    ) -> AppResult<Vec<ProtocolPackageDescriptionViewModel>> {
        let mut descriptions = Vec::with_capacity(packages.len());
        for package in packages {
            descriptions.push(self.describe(package).await?);
        }
        Ok(descriptions)
    }

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
            let component = read_package_component(&bytes)
                .map_err(intercept_proxy_application::AppError::from)?;
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
            let runtime = Self::load_local_component(&package.package, &bytes).await?;
            stored_packages.push(LocalApplicationPackageRecord {
                registration: component.manifest().clone(),
                archive: bytes,
                enabled: package.enabled,
            });
            if package.enabled {
                enabled_runtimes.push((package.package, runtime));
            }
        }

        let _bundle_mutation = self.application_bundle_mutation.lock().await;
        if !self.online.lock().is_empty() {
            return Err(unsupported_remote_backup());
        }
        self.executor
            .execute(move |store| {
                store.replace_application_bundle(selected_id, &records, &settings, &stored_packages)
            })
            .await
            .map_err(app_error)?;
        self.reconcile_local_runtimes(enabled_runtimes);
        self.publish_service_status();
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
        let _bundle_mutation = self.application_bundle_mutation.lock().await;
        if !self.online.lock().is_empty() {
            return Err(AppError::new(
                "APPLICATION_RESET_REMOTE_PACKAGES_ONLINE",
                "远端调试软件包仍在线，无法原子清除应用数据。",
            ));
        }
        self.executor
            .execute(move |store| store.reset_application_data(selected_id, &records, &settings))
            .await
            .map_err(app_error)?;
        self.deactivate_all_local_components();
        self.publish_service_status();
        Ok(())
    }
}
