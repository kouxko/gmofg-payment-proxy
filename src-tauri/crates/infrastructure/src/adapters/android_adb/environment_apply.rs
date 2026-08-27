use std::sync::Arc;

use intercept_proxy_application::AndroidRuntimeOwnerViewModel;

use super::AndroidAdbAdapter;
use crate::adapters::EnvironmentApplyResourceGateRegistry;

pub(super) fn publish_android_owner_mutation(
    resource_gates: &EnvironmentApplyResourceGateRegistry,
    before: Option<&AndroidRuntimeOwnerViewModel>,
    after: Option<&AndroidRuntimeOwnerViewModel>,
) {
    if let Some(previous) = before
        && after.is_none_or(|current| {
            current.profile_id != previous.profile_id || current.serial != previous.serial
        })
    {
        resource_gates.publish_android_projection(&previous.profile_id, &previous.serial, None);
    }
    if let Some(current) = after {
        resource_gates.publish_android_projection(
            &current.profile_id,
            &current.serial,
            Some(format!(
                "{}:{:?}:{}",
                current.epoch, current.state, current.serial
            )),
        );
    }
}

impl AndroidAdbAdapter {
    pub(crate) fn with_environment_apply_resource_gates(
        mut self,
        gates: Arc<EnvironmentApplyResourceGateRegistry>,
    ) -> Self {
        self.environment_apply_resource_gates = gates;
        self
    }

    pub(super) async fn acquire_environment_apply_gates(
        &self,
        profile_id: Option<&str>,
        serial: Option<&str>,
    ) -> Vec<tokio::sync::OwnedMutexGuard<()>> {
        let keys = match (profile_id, serial) {
            (Some(profile_id), Some(serial)) => {
                vec![EnvironmentApplyResourceGateRegistry::android_owner_key(
                    profile_id, serial,
                )]
            }
            (None, Some(serial)) => self
                .environment_apply_resource_gates
                .leased_android_owner_key_for_device(serial)
                .into_iter()
                .collect(),
            (Some(_) | None, None) => Vec::new(),
        };
        self.environment_apply_resource_gates
            .acquire_all(keys)
            .await
    }

    #[cfg(test)]
    pub(super) async fn clear_owner_if_epoch(
        &self,
        serial: &str,
        expected_epoch: uuid::Uuid,
    ) -> intercept_proxy_application::AppResult<bool> {
        let owner = self.runtime_owner_snapshot_for(serial).await;
        let _resource_guards = if let Some(owner) = owner.as_ref() {
            self.acquire_environment_apply_gates(Some(&owner.profile_id), Some(&owner.serial))
                .await
        } else {
            self.environment_apply_resource_gates
                .acquire_known_android_owner_gates()
                .await
        };
        let gate = self.device_operations.gate(serial);
        let _operation = gate.lock().await;
        self.clear_owner_if_epoch_under_gate(serial, expected_epoch)
            .await
    }
}
