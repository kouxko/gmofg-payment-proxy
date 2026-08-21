//! 内置与外部注册表的统一精确版本查询。

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
        let internal = self.protocol_package_store.get(package).await?;
        let external = self.external_packages.get(package).await?;
        match (internal, external) {
            (Some(_), Some(_)) => Err(AppError::new(
                "PROTOCOL_PACKAGE_SOURCE_CONFLICT",
                "同一精确协议包版本不能同时来自内置注册表和外部注册表。",
            )
            .entity(package_entity(package))),
            (Some(version), None) => {
                ensure_source_kind(package, version.source, false)?;
                Ok(version)
            }
            (None, Some(version)) => {
                ensure_source_kind(package, version.source, true)?;
                Ok(version)
            }
            (None, None) => Err(protocol_package_not_found(package)),
        }
    }

    pub(super) async fn protocol_package_versions(
        &self,
    ) -> AppResult<Vec<ProtocolPackageVersionViewModel>> {
        let mut versions = self.protocol_package_store.list().await?;
        for version in &versions {
            ensure_source_kind(&version.package, version.source, false)?;
        }
        let external = self.external_packages.list().await?;
        for version in &external {
            ensure_source_kind(&version.package, version.source, true)?;
        }
        versions.extend(external);
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

fn ensure_source_kind(
    package: &ProtocolPackageRef,
    source: ProtocolPackageSourceViewModel,
    expected_external: bool,
) -> AppResult<()> {
    if source.is_external() == expected_external {
        return Ok(());
    }
    Err(AppError::new(
        "PROTOCOL_PACKAGE_SOURCE_INVALID",
        "协议包端口返回了与注册表不一致的执行来源。",
    )
    .entity(package_entity(package)))
}
