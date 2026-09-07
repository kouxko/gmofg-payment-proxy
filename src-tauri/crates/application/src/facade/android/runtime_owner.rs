use super::Application;
use crate::{AndroidNetworkControlOverviewViewModel, AndroidRuntimeOwnerViewModel, AppResult};

impl Application {
    pub async fn device_network_control_overview(
        &self,
    ) -> AppResult<AndroidNetworkControlOverviewViewModel> {
        let owners = self.device_network_runtime_owners().await?;
        Ok(AndroidNetworkControlOverviewViewModel::from_owners(owners))
    }

    pub async fn device_network_runtime_owners(
        &self,
    ) -> AppResult<Vec<AndroidRuntimeOwnerViewModel>> {
        let mut owners = self.android.runtime_owners().await?;
        owners.sort_by(|left, right| left.serial.cmp(&right.serial));
        Ok(owners)
    }
}
