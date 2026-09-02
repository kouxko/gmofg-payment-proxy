use intercept_proxy_application::ExternalPackageServiceStateViewModel;

use super::{ExternalPackageRegistryAdapter, ExternalPackageServiceSnapshot};

impl ExternalPackageRegistryAdapter {
    /// Records the WebSocket address after the Host has successfully bound it.
    pub async fn mark_service_listening(&self, websocket_url: impl Into<String>) {
        let _environment_apply_gates = self
            .environment_apply_resource_gates
            .acquire_known_exact_package_gates()
            .await;
        let websocket_url = websocket_url.into();
        *self.service.lock() = ExternalPackageServiceSnapshot {
            websocket_url: websocket_url.clone(),
            state: ExternalPackageServiceStateViewModel::Listening,
        };
        self.publish_service_status();
        self.publish_service_listening(&websocket_url);
        self.environment_apply_resource_gates
            .advance_exact_package_service_epoch();
    }

    /// Records a non-fatal Host bind failure; internal packages remain available.
    pub async fn mark_service_failed(
        &self,
        websocket_url: impl Into<String>,
        error: impl Into<String>,
    ) {
        let _environment_apply_gates = self
            .environment_apply_resource_gates
            .acquire_known_exact_package_gates()
            .await;
        let websocket_url = websocket_url.into();
        let error = error.into();
        *self.service.lock() = ExternalPackageServiceSnapshot {
            websocket_url: websocket_url.clone(),
            state: ExternalPackageServiceStateViewModel::Failed {
                error: error.clone(),
            },
        };
        self.publish_service_status();
        self.publish_service_failed(&websocket_url);
        self.environment_apply_resource_gates
            .advance_exact_package_service_epoch();
    }
}
