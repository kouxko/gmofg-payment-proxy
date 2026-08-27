//! 外部协议包注册表到 Application 生命周期与详情端口的适配。

use async_trait::async_trait;
use intercept_proxy_application::{
    AppError, AppResult, ExternalPackageApplicationPort, ExternalPackageDetailViewModel,
    ExternalPackageServiceStatusViewModel, ProtocolPackageDescriptionViewModel,
    ProtocolPackageVersionViewModel,
};
use intercept_proxy_domain::ProtocolPackageRef;
use uuid::Uuid;

use super::{
    ExternalPackageConnectionId, ExternalPackageRegistryAdapter, OnlineConnection, app_error,
    not_found,
    views::{application_description, application_detail, application_summary},
};

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
            .map(|records| {
                records
                    .into_iter()
                    .map(|record| {
                        let online = self.is_online(record.registration.package().identity());
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
        let _environment_apply_gate = self.acquire_environment_apply_package_gate(package).await;
        let selected = package.clone();
        if self
            .executor
            .execute(move |store| store.set_external_package_enabled(&selected, enabled))
            .await
            .map_err(app_error)?
        {
            self.publish_catalog_changed(package);
            Ok(())
        } else {
            Err(not_found(package))
        }
    }

    async fn disconnect(&self, package: &ProtocolPackageRef) -> AppResult<()> {
        let environment_apply_gate = self.acquire_environment_apply_package_gate(package).await;
        let mut completion = self
            .begin_disconnect(package, Some(environment_apply_gate))
            .await;
        Self::wait_for_closing(&mut completion).await;
        Ok(())
    }

    async fn delete(&self, package: &ProtocolPackageRef) -> AppResult<()> {
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
}
