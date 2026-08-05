use async_trait::async_trait;

use crate::{
    AndroidAdbViewModel, AndroidCompanionInstallViewModel, AndroidDeviceViewModel,
    AndroidNetworkActivation, AndroidNetworkStatusViewModel, AndroidPackageViewModel, AppError,
    AppResult,
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
}

#[derive(Debug, Default)]
pub(crate) struct UnavailableAndroidControlPort;

impl UnavailableAndroidControlPort {
    fn unavailable<T>() -> AppResult<T> {
        Err(AppError::new(
            "ANDROID_CONTROL_UNAVAILABLE",
            "当前 Host 未注入 Android ADB 控制适配器。",
        ))
    }
}

#[async_trait]
impl AndroidControlPort for UnavailableAndroidControlPort {
    async fn adb_get(&self) -> AppResult<AndroidAdbViewModel> {
        Self::unavailable()
    }
    async fn adb_select(&self, _: String) -> AppResult<AndroidAdbViewModel> {
        Self::unavailable()
    }
    async fn device_list(&self) -> AppResult<Vec<AndroidDeviceViewModel>> {
        Self::unavailable()
    }
    async fn package_list(&self) -> AppResult<Vec<AndroidPackageViewModel>> {
        Self::unavailable()
    }
    async fn package_get(&self, _: String) -> AppResult<AndroidPackageViewModel> {
        Self::unavailable()
    }
    async fn companion_install(&self, _: bool) -> AppResult<AndroidCompanionInstallViewModel> {
        Self::unavailable()
    }
    async fn vpn_open_consent(&self) -> AppResult<AndroidNetworkStatusViewModel> {
        Self::unavailable()
    }
    async fn network_start(
        &self,
        _: AndroidNetworkActivation,
    ) -> AppResult<AndroidNetworkStatusViewModel> {
        Self::unavailable()
    }
    async fn network_apply(
        &self,
        _: AndroidNetworkActivation,
    ) -> AppResult<AndroidNetworkStatusViewModel> {
        Self::unavailable()
    }
    async fn network_runtime_ready(
        &self,
        _: &AndroidNetworkActivation,
        _: &AndroidNetworkStatusViewModel,
    ) -> AppResult<bool> {
        Self::unavailable()
    }
    async fn network_stop(&self) -> AppResult<AndroidNetworkStatusViewModel> {
        Self::unavailable()
    }
    async fn emergency_restore(&self) -> AppResult<AndroidNetworkStatusViewModel> {
        Self::unavailable()
    }
    async fn network_status(&self) -> AppResult<AndroidNetworkStatusViewModel> {
        Self::unavailable()
    }
}
