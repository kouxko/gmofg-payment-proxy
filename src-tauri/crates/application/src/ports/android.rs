use async_trait::async_trait;

use crate::{
    AndroidAdbViewModel, AndroidCompanionInstallViewModel, AndroidDeviceViewModel,
    AndroidNetworkActivation, AndroidNetworkStatusViewModel, AndroidPackageViewModel,
    AndroidRuntimeEndpointViewModel, AndroidRuntimeOwnerViewModel, AppResult,
};

#[async_trait]
pub trait AndroidControlPort: Send + Sync + std::fmt::Debug {
    async fn adb_get(&self) -> AppResult<AndroidAdbViewModel>;
    async fn adb_select(&self, serial: String) -> AppResult<AndroidAdbViewModel>;
    async fn device_list(&self) -> AppResult<Vec<AndroidDeviceViewModel>>;
    async fn package_list(&self) -> AppResult<Vec<AndroidPackageViewModel>>;
    async fn package_get(&self, package_name: String) -> AppResult<AndroidPackageViewModel>;
    async fn companion_install(&self, update: bool) -> AppResult<AndroidCompanionInstallViewModel>;
    async fn vpn_open_consent(&self) -> AppResult<AndroidNetworkStatusViewModel>;
    async fn network_start(
        &self,
        activation: AndroidNetworkActivation,
    ) -> AppResult<AndroidNetworkStatusViewModel>;
    async fn network_apply(
        &self,
        activation: AndroidNetworkActivation,
    ) -> AppResult<AndroidNetworkStatusViewModel>;
    async fn network_runtime_ready(
        &self,
        activation: &AndroidNetworkActivation,
        status: &AndroidNetworkStatusViewModel,
    ) -> AppResult<bool>;
    async fn network_stop(&self) -> AppResult<AndroidNetworkStatusViewModel>;
    async fn emergency_restore(&self) -> AppResult<AndroidNetworkStatusViewModel>;
    async fn network_status(&self) -> AppResult<AndroidNetworkStatusViewModel>;
    async fn runtime_owner(&self) -> AppResult<Option<AndroidRuntimeOwnerViewModel>>;
    async fn network_runtime_endpoints(
        &self,
        activation: Option<AndroidNetworkActivation>,
    ) -> AppResult<Vec<AndroidRuntimeEndpointViewModel>>;
}
