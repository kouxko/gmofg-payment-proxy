use super::Application;
use crate::{AndroidRuntimeOwnerViewModel, AppResult};

impl Application {
    pub async fn device_network_runtime_owner(
        &self,
    ) -> AppResult<Option<AndroidRuntimeOwnerViewModel>> {
        self.android.runtime_owner().await
    }
}
