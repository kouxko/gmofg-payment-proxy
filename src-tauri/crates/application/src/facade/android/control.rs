//! Android companion lifecycle and VPN consent control.

use super::Application;
use crate::{AndroidCompanionInstallViewModel, AndroidNetworkStatusViewModel, AppResult};

impl Application {
    pub async fn android_companion_install(&self) -> AppResult<AndroidCompanionInstallViewModel> {
        self.android.companion_install(false).await
    }

    pub async fn android_companion_update(&self) -> AppResult<AndroidCompanionInstallViewModel> {
        self.android.companion_install(true).await
    }

    pub async fn android_vpn_open_consent(&self) -> AppResult<AndroidNetworkStatusViewModel> {
        let status = self.android.vpn_open_consent().await?;
        self.publish_android_vpn_status(&status);
        Ok(status)
    }
}
