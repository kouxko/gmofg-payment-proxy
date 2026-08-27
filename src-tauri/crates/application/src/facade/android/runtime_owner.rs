use super::Application;
use crate::{AndroidRuntimeOwnerViewModel, AppResult};

impl Application {
    pub async fn device_network_runtime_owners(
        &self,
    ) -> AppResult<Vec<AndroidRuntimeOwnerViewModel>> {
        let mut owners = self.android.runtime_owners().await?;
        owners.sort_by(|left, right| left.serial.cmp(&right.serial));
        Ok(owners)
    }
}
