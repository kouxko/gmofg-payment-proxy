use async_trait::async_trait;

use crate::{
    AndroidAdbViewModel, AndroidCompanionInstallViewModel, AndroidDeviceTarget,
    AndroidDeviceViewModel, AndroidNetworkActivation, AndroidNetworkStatusViewModel,
    AndroidPackageViewModel, AndroidRuntimeEndpointViewModel, AndroidRuntimeOwnerViewModel,
    AndroidRuntimeTarget, AppResult,
};

#[async_trait]
pub trait AndroidControlPort: Send + Sync + std::fmt::Debug {
    async fn adb_get(&self) -> AppResult<AndroidAdbViewModel>;
    async fn adb_select(&self, serial: String) -> AppResult<AndroidAdbViewModel>;
    async fn device_list(&self) -> AppResult<Vec<AndroidDeviceViewModel>>;
    async fn package_list(
        &self,
        target: AndroidDeviceTarget,
    ) -> AppResult<Vec<AndroidPackageViewModel>>;
    async fn package_get(
        &self,
        target: AndroidDeviceTarget,
        package_name: String,
    ) -> AppResult<AndroidPackageViewModel>;
    async fn companion_install(
        &self,
        target: AndroidDeviceTarget,
        update: bool,
    ) -> AppResult<AndroidCompanionInstallViewModel>;
    async fn vpn_open_consent(
        &self,
        target: AndroidDeviceTarget,
    ) -> AppResult<AndroidNetworkStatusViewModel>;
    async fn network_start(
        &self,
        target: AndroidDeviceTarget,
        activation: AndroidNetworkActivation,
    ) -> AppResult<AndroidNetworkStatusViewModel>;
    async fn network_apply(
        &self,
        target: AndroidRuntimeTarget,
        activation: AndroidNetworkActivation,
    ) -> AppResult<AndroidNetworkStatusViewModel>;
    async fn network_runtime_ready(
        &self,
        target: AndroidDeviceTarget,
        activation: &AndroidNetworkActivation,
        status: &AndroidNetworkStatusViewModel,
    ) -> AppResult<bool>;
    async fn network_stop(
        &self,
        target: AndroidRuntimeTarget,
    ) -> AppResult<AndroidNetworkStatusViewModel>;
    async fn emergency_restore(
        &self,
        target: AndroidRuntimeTarget,
    ) -> AppResult<AndroidNetworkStatusViewModel>;
    async fn network_status(
        &self,
        target: AndroidDeviceTarget,
    ) -> AppResult<AndroidNetworkStatusViewModel>;
    async fn runtime_owners(&self) -> AppResult<Vec<AndroidRuntimeOwnerViewModel>>;
    async fn network_runtime_endpoints(
        &self,
        target: AndroidDeviceTarget,
        activation: Option<AndroidNetworkActivation>,
    ) -> AppResult<Vec<AndroidRuntimeEndpointViewModel>>;
}
