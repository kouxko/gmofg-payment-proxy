//! 本地 Wasm Component 的持久化与进程内实例生命周期。

use chrono::Utc;
use intercept_proxy_application::AppResult;
use intercept_proxy_domain::ProtocolPackageRef;
use intercept_proxy_package_contract::PackageManifest;
use intercept_proxy_package_runtime::{WasmPackageRuntime, read_package_component};
use std::collections::HashMap;
use std::sync::Arc;

use super::{ExternalPackageRegistryAdapter, OnlineConnection, app_error, package_error};
use crate::adapters::InProcessWasmRuntime;
use crate::sqlite::external_packages::StoredLocalPackageInstallOutcome;

impl ExternalPackageRegistryAdapter {
    pub(super) async fn load_local_component(
        package: &ProtocolPackageRef,
        bytes: &[u8],
    ) -> AppResult<WasmPackageRuntime> {
        let component =
            read_package_component(bytes).map_err(intercept_proxy_application::AppError::from)?;
        if component.manifest().package().identity() != *package {
            return Err(package_error(
                "PROTOCOL_PACKAGE_IDENTITY_CONFLICT",
                "Wasm Component 内嵌身份与待启动的精确版本不一致。",
                package,
            ));
        }
        WasmPackageRuntime::load(&component)
            .await
            .map_err(intercept_proxy_application::AppError::from)
    }

    fn reject_remote_identity(&self, package: &ProtocolPackageRef) -> AppResult<()> {
        if matches!(
            self.online.lock().get(package),
            Some(OnlineConnection::Active { .. } | OnlineConnection::Closing { .. })
        ) {
            Err(package_error(
                "EXTERNAL_PACKAGE_ALREADY_ONLINE",
                "相同协议包精确版本已有远端在线连接。",
                package,
            ))
        } else {
            Ok(())
        }
    }

    pub(super) fn publish_local_runtime(
        &self,
        package: &ProtocolPackageRef,
        runtime: WasmPackageRuntime,
    ) {
        let existing = self.local_runtimes.lock().get(package).cloned();
        if let Some(existing) = existing {
            existing.replace(runtime);
        } else {
            self.local_runtimes.lock().insert(
                package.clone(),
                Arc::new(InProcessWasmRuntime::new(runtime)),
            );
        }
        self.online_changed.notify_waiters();
        self.publish_catalog_changed(package);
    }

    pub(crate) async fn activate_local_component(
        &self,
        package: &ProtocolPackageRef,
        bytes: &[u8],
    ) -> AppResult<()> {
        let runtime = Self::load_local_component(package, bytes).await?;
        let _environment_apply_gate = self.acquire_environment_apply_package_gate(package).await;
        let gate = self.connection_mutation(package);
        let _mutation = gate.lock().await;
        self.reject_remote_identity(package)?;
        self.publish_local_runtime(package, runtime);
        Ok(())
    }

    pub(crate) async fn set_local_component_enabled(
        &self,
        package: &ProtocolPackageRef,
        bytes: &[u8],
        enabled: bool,
    ) -> AppResult<bool> {
        let runtime = if enabled {
            Some(Self::load_local_component(package, bytes).await?)
        } else {
            None
        };
        let _environment_apply_gate = self.acquire_environment_apply_package_gate(package).await;
        let gate = self.connection_mutation(package);
        let _mutation = gate.lock().await;
        self.reject_remote_identity(package)?;

        let selected = package.clone();
        let updated = self
            .executor
            .execute(move |store| store.set_external_package_enabled(&selected, enabled))
            .await
            .map_err(app_error)?;
        if !updated {
            return Ok(false);
        }
        if let Some(runtime) = runtime {
            self.publish_local_runtime(package, runtime);
        } else {
            self.deactivate_local_component(package);
        }
        self.publish_catalog_changed(package);
        Ok(true)
    }

    pub(crate) async fn restart_local_component(
        &self,
        package: &ProtocolPackageRef,
        bytes: &[u8],
    ) -> AppResult<()> {
        let _environment_apply_gate = self.acquire_environment_apply_package_gate(package).await;
        let gate = self.connection_mutation(package);
        let _mutation = gate.lock().await;
        self.reject_remote_identity(package)?;
        let current = { self.local_runtimes.lock().get(package).cloned() };
        if let Some(runtime) = current {
            runtime.deactivate();
        }
        let runtime = Self::load_local_component(package, bytes).await?;
        self.publish_local_runtime(package, runtime);
        Ok(())
    }

    pub(crate) fn deactivate_local_component(&self, package: &ProtocolPackageRef) -> bool {
        let runtime = self.local_runtimes.lock().get(package).cloned();
        let deactivated = match runtime {
            Some(runtime) => runtime.deactivate(),
            None => false,
        };
        if deactivated {
            self.publish_catalog_changed(package);
        }
        deactivated
    }

    pub(crate) fn remove_local_component(&self, package: &ProtocolPackageRef) -> bool {
        let runtime = self.local_runtimes.lock().remove(package);
        let removed = match runtime {
            Some(runtime) => runtime.deactivate(),
            None => false,
        };
        if removed {
            self.publish_catalog_changed(package);
        }
        removed
    }

    pub(crate) fn deactivate_all_local_components(&self) {
        let runtimes = self.local_runtimes.lock().drain().collect::<Vec<_>>();
        for (package, runtime) in runtimes {
            runtime.deactivate();
            self.publish_catalog_changed(&package);
        }
    }

    pub(super) fn reconcile_local_runtimes(
        &self,
        runtimes: Vec<(ProtocolPackageRef, WasmPackageRuntime)>,
    ) {
        let mut desired = runtimes.into_iter().collect::<HashMap<_, _>>();
        let mut changed = Vec::new();
        {
            let mut current = self.local_runtimes.lock();
            current.retain(|package, runtime| {
                if let Some(replacement) = desired.remove(package) {
                    runtime.replace(replacement);
                    changed.push(package.clone());
                    true
                } else {
                    runtime.deactivate();
                    changed.push(package.clone());
                    false
                }
            });
            for (package, runtime) in desired {
                current.insert(
                    package.clone(),
                    Arc::new(InProcessWasmRuntime::new(runtime)),
                );
                changed.push(package);
            }
        }
        self.online_changed.notify_waiters();
        for package in changed {
            self.publish_catalog_changed(&package);
        }
    }

    pub(crate) async fn install_and_activate_local_component(
        &self,
        registration: &PackageManifest,
        archive: &[u8],
    ) -> AppResult<StoredLocalPackageInstallOutcome> {
        let package = registration.package().identity();
        let runtime = Self::load_local_component(&package, archive).await?;
        let _environment_apply_gate = self.acquire_environment_apply_package_gate(&package).await;
        let gate = self.connection_mutation(&package);
        let _mutation = gate.lock().await;
        self.reject_remote_identity(&package)?;

        let stored_registration = registration.clone();
        let stored_archive = archive.to_vec();
        let outcome = self
            .executor
            .execute(move |store| {
                store.install_local_external_package(
                    &stored_registration,
                    &stored_archive,
                    Utc::now(),
                )
            })
            .await
            .map_err(app_error)?;
        if outcome == StoredLocalPackageInstallOutcome::IdentityConflict {
            return Ok(outcome);
        }
        self.publish_local_runtime(&package, runtime);
        Ok(outcome)
    }

    pub(crate) fn active_local_runtime(
        &self,
        package: &ProtocolPackageRef,
    ) -> Option<Arc<InProcessWasmRuntime>> {
        self.local_runtimes
            .lock()
            .get(package)
            .filter(|runtime| runtime.is_active())
            .cloned()
    }

    pub(crate) fn has_active_local_runtime(&self, package: &ProtocolPackageRef) -> bool {
        self.local_runtimes
            .lock()
            .get(package)
            .is_some_and(|runtime| runtime.is_active())
    }

    pub(crate) async fn preview_local_archive(
        &self,
        registration: &PackageManifest,
        archive: &[u8],
    ) -> AppResult<StoredLocalPackageInstallOutcome> {
        let package = registration.package().identity();
        let selected = package.clone();
        let stored = self
            .executor
            .execute(move |store| store.get_external_package(&selected))
            .await
            .map_err(app_error)?;
        Ok(match stored {
            None => StoredLocalPackageInstallOutcome::Installed,
            Some(stored)
                if stored.registration == *registration
                    && stored.local_archive.as_deref() == Some(archive) =>
            {
                StoredLocalPackageInstallOutcome::Reused
            }
            Some(_) => StoredLocalPackageInstallOutcome::IdentityConflict,
        })
    }

    pub(crate) async fn install_local_archive(
        &self,
        registration: &PackageManifest,
        archive: &[u8],
    ) -> AppResult<StoredLocalPackageInstallOutcome> {
        let package = registration.package().identity();
        let registration = registration.clone();
        let archive = archive.to_vec();
        let outcome = self
            .executor
            .execute(move |store| {
                store.install_local_external_package(&registration, &archive, Utc::now())
            })
            .await
            .map_err(app_error)?;
        self.publish_catalog_changed(&package);
        Ok(outcome)
    }

    pub(crate) async fn enabled_local_archives(
        &self,
    ) -> AppResult<Vec<(ProtocolPackageRef, Vec<u8>)>> {
        self.executor
            .execute(crate::SqliteStore::list_external_packages)
            .await
            .map_err(app_error)
            .map(|records| {
                records
                    .into_iter()
                    .filter_map(|record| {
                        record
                            .enabled
                            .then_some(record.local_archive)
                            .flatten()
                            .map(|archive| (record.registration.package().identity(), archive))
                    })
                    .collect()
            })
    }

    pub(crate) async fn local_archive(
        &self,
        package: &ProtocolPackageRef,
    ) -> AppResult<Option<Vec<u8>>> {
        let selected = package.clone();
        self.executor
            .execute(move |store| store.get_external_package(&selected))
            .await
            .map_err(app_error)
            .map(|stored| stored.and_then(|record| record.local_archive))
    }
}
