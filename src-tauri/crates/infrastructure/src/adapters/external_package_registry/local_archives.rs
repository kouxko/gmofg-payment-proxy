//! 本地 Sidecar ZIP 的持久化生命周期。

use chrono::Utc;
use intercept_proxy_application::AppResult;
use intercept_proxy_domain::ProtocolPackageRef;
use intercept_proxy_package_contract::PackageManifest;

use super::{ExternalPackageRegistryAdapter, app_error, not_found};
use crate::sqlite::external_packages::StoredLocalPackageInstallOutcome;

impl ExternalPackageRegistryAdapter {
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

    pub(crate) async fn record_local_process_failure(
        &self,
        package: &ProtocolPackageRef,
        code: &'static str,
        message: &'static str,
    ) -> AppResult<()> {
        let selected = package.clone();
        let updated = self
            .executor
            .execute(move |store| {
                store.record_external_package_recent_error(&selected, code, message, Utc::now())
            })
            .await
            .map_err(app_error)?;
        if updated {
            self.publish_catalog_changed(package);
            Ok(())
        } else {
            Err(not_found(package))
        }
    }
}
