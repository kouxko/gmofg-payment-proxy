use super::{Application, package_entity};
use crate::{
    AppError, AppResult, ProtocolPackageRef, ProtocolPackageSourceViewModel,
    ProtocolPackageVersionViewModel,
};

impl Application {
    /// 手动重启 Proxy 拥有的本地 exact Sidecar；远端注册进程不受 Proxy 接管。
    pub async fn protocol_package_restart(
        &self,
        package: ProtocolPackageRef,
    ) -> AppResult<ProtocolPackageVersionViewModel> {
        let _gate = self.mutation_gate.lock().await;
        let stored = self.require_protocol_package(&package).await?;
        if !stored.enabled {
            return Err(AppError::new(
                "PROTOCOL_PACKAGE_DISABLED",
                "协议包已停用，不能重启本地 Sidecar。",
            )
            .entity(package_entity(&package)));
        }
        if !matches!(
            stored.source,
            ProtocolPackageSourceViewModel::External { .. }
        ) || !self.external_packages.detail(&package).await?.local_process
        {
            return Err(AppError::new(
                "EXTERNAL_PACKAGE_RESTART_UNAVAILABLE",
                "只有 Proxy 拥有的本地 Sidecar 可以手动重启。",
            )
            .entity(package_entity(&package)));
        }
        self.external_packages.restart(&package).await?;
        self.external_packages
            .get(&package)
            .await?
            .ok_or_else(|| AppError::new("PROTOCOL_PACKAGE_NOT_FOUND", "未找到协议包精确版本。"))
    }
}
