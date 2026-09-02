//! Android companion lifecycle and VPN consent control.

use super::Application;
use crate::{
    AndroidCompanionInstallViewModel, AndroidDeviceTarget, AndroidNetworkStatusViewModel, AppResult,
};

impl Application {
    pub async fn android_companion_install(
        &self,
        serial: String,
    ) -> AppResult<AndroidCompanionInstallViewModel> {
        super::validate_serial(&serial)?;
        let result = self
            .android
            .companion_install(
                AndroidDeviceTarget {
                    serial: serial.clone(),
                },
                false,
            )
            .await;
        match result {
            Ok(value) => Ok(value),
            Err(error) => Err(self.android_error_context(&serial, error).await),
        }
    }

    pub async fn android_companion_update(
        &self,
        serial: String,
    ) -> AppResult<AndroidCompanionInstallViewModel> {
        super::validate_serial(&serial)?;
        let result = self
            .android
            .companion_install(
                AndroidDeviceTarget {
                    serial: serial.clone(),
                },
                true,
            )
            .await;
        match result {
            Ok(value) => Ok(value),
            Err(error) => Err(self.android_error_context(&serial, error).await),
        }
    }

    pub async fn android_vpn_open_consent(
        &self,
        serial: String,
    ) -> AppResult<AndroidNetworkStatusViewModel> {
        super::validate_serial(&serial)?;
        let status = self
            .android
            .vpn_open_consent(AndroidDeviceTarget {
                serial: serial.clone(),
            })
            .await;
        let status = match status {
            Ok(status) => status,
            Err(error) => return Err(self.android_error_context(&serial, error).await),
        };
        self.publish_android_vpn_status(&status);
        Ok(status)
    }
}
