//! 外部注册表的统一精确版本查询。

use super::{Application, package_entity, protocol_package_not_found};
use crate::{
    AppError, AppResult, ProtocolPackageRef, ProtocolPackageSourceViewModel,
    ProtocolPackageVersionViewModel,
};

impl Application {
    pub(in crate::facade) async fn require_protocol_package(
        &self,
        package: &ProtocolPackageRef,
    ) -> AppResult<ProtocolPackageVersionViewModel> {
        match self.external_packages.get(package).await? {
            Some(version) => {
                ensure_external_source(package, version.source)?;
                Ok(version)
            }
            None => Err(protocol_package_not_found(package)),
        }
    }

    pub(super) async fn protocol_package_versions(
        &self,
    ) -> AppResult<Vec<ProtocolPackageVersionViewModel>> {
        let versions = self.external_packages.list().await?;
        for version in &versions {
            ensure_external_source(&version.package, version.source)?;
        }
        if versions.iter().enumerate().any(|(index, version)| {
            versions[index + 1..]
                .iter()
                .any(|other| other.package == version.package)
        }) {
            return Err(AppError::new(
                "PROTOCOL_PACKAGE_SOURCE_CONFLICT",
                "协议包目录包含跨来源重复的精确版本，已拒绝使用。",
            ));
        }
        Ok(versions)
    }
}

fn ensure_external_source(
    package: &ProtocolPackageRef,
    source: ProtocolPackageSourceViewModel,
) -> AppResult<()> {
    if source.is_external() {
        return Ok(());
    }
    Err(AppError::new(
        "PROTOCOL_PACKAGE_SOURCE_INVALID",
        "协议包端口返回了与注册表不一致的执行来源。",
    )
    .entity(package_entity(package)))
}
