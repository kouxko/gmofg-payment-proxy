//! 外部协议包注册表到 Application 生命周期与详情端口的适配。

use async_trait::async_trait;
use intercept_proxy_application::{
    AppResult, ExternalPackageApplicationPort, ExternalPackageDetailViewModel,
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
        self.store
            .list_external_packages()
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
        self.store
            .get_external_package(package)
            .map_err(app_error)
            .map(|record| {
                record.map(|record| application_summary(&record, self.is_online(package)))
            })
    }

    async fn describe(
        &self,
        package: &ProtocolPackageRef,
    ) -> AppResult<ProtocolPackageDescriptionViewModel> {
        let stored = self
            .store
            .get_external_package(package)
            .map_err(app_error)?
            .ok_or_else(|| not_found(package))?;
        Ok(application_description(&stored.registration))
    }

    async fn detail(
        &self,
        package: &ProtocolPackageRef,
    ) -> AppResult<ExternalPackageDetailViewModel> {
        let stored = self
            .store
            .get_external_package(package)
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
        if self
            .store
            .set_external_package_enabled(package, enabled)
            .map_err(app_error)?
        {
            self.publish_catalog_changed(package);
            Ok(())
        } else {
            Err(not_found(package))
        }
    }

    async fn disconnect(&self, package: &ProtocolPackageRef) -> AppResult<()> {
        self.disconnect_active(package, false).await;
        Ok(())
    }

    async fn delete(&self, package: &ProtocolPackageRef) -> AppResult<()> {
        // SQLite 删除完成前保留 Closing 门禁，防止并发重注册在断连与删除之间重建记录，
        // 或发布一个不再受注册表跟踪的在线 client。
        {
            let mut online = self.online.lock();
            online
                .entry(package.clone())
                .or_insert_with(|| OnlineConnection::Closing {
                    id: ExternalPackageConnectionId(Uuid::new_v4()),
                    client: None,
                });
        }
        self.disconnect_active(package, true).await;
        let deletion = self
            .store
            .delete_external_package(package)
            .map_err(app_error);
        self.online.lock().remove(package);
        deletion.map(|_| {
            self.publish_catalog_changed(package);
            self.publish_service_status();
        })
    }
}
