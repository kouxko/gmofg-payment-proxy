use async_trait::async_trait;
use intercept_proxy_application::{
    AppResult, EnvironmentAffectedListenerBaseline, EnvironmentAndroidOwnerBaseline,
    EnvironmentApplyBaselineCapturePort, EnvironmentApplyBaselineCaptureRequest,
    EnvironmentExactPackageBaseline, EnvironmentMaterialInventoryBaseline,
    EnvironmentValidatedApplyBaseline, EnvironmentValidatedApplyBaselineCollector,
};

use super::{
    EnvironmentApplyRuntimeAdapter,
    environment_configuration_lease::{
        EnvironmentApplyLeaseResourceKey, EnvironmentApplyLeaseResourceObservation,
        EnvironmentApplyLeaseRuntime, package_unavailable,
    },
};

#[async_trait]
impl EnvironmentApplyBaselineCapturePort for EnvironmentApplyRuntimeAdapter {
    async fn capture(
        &self,
        request: EnvironmentApplyBaselineCaptureRequest,
    ) -> AppResult<EnvironmentValidatedApplyBaseline> {
        let workspace = &request.candidate_workspace;
        let generations = self.observe_generations(workspace.id.as_uuid()).await?;
        let statuses = self.listeners.statuses().await?;
        let affected_listener_ids = request.affected_listener_ids();
        let affected_listeners = affected_listener_ids
            .iter()
            .map(|listener_id| {
                let status = statuses
                    .iter()
                    .find(|status| status.listener_id == *listener_id);
                EnvironmentAffectedListenerBaseline::observed(
                    listener_id.as_uuid(),
                    status.and_then(|status| status.runtime_epoch),
                    status.map_or(0, |status| status.active_connections),
                )
            })
            .collect();
        let android_owners = self
            .android
            .runtime_owners()
            .await?
            .into_iter()
            .map(|owner| {
                EnvironmentAndroidOwnerBaseline::observed(
                    owner.profile_id,
                    owner.serial,
                    owner.epoch,
                    format!("{:?}", owner.state).to_ascii_lowercase(),
                )
            })
            .collect();
        let package_refs = request.exact_package_refs();
        let mut packages = Vec::with_capacity(package_refs.len());
        for package in package_refs {
            let observation = self
                .observe_resource(&EnvironmentApplyLeaseResourceKey::ExactPackage(
                    package.clone(),
                ))
                .await?;
            let EnvironmentApplyLeaseResourceObservation::ExactPackage {
                generation,
                enabled,
                online,
                service_epoch,
                description_fingerprint,
                online_generation,
                lease_generation,
            } = observation
            else {
                return Err(package_unavailable());
            };
            packages.push(EnvironmentExactPackageBaseline::observed_projection(
                package,
                generation,
                enabled,
                online,
                service_epoch,
                description_fingerprint,
                online_generation,
                lease_generation,
            ));
        }
        let structural_hash = request.persisted_workspace_structural_hash();
        let material_inventory = vec![
            inventory_baseline("certificate_inventory", generations.certificate_inventory),
            inventory_baseline(
                "protected_secret_inventory",
                generations.protected_secret_inventory,
            ),
        ];
        EnvironmentValidatedApplyBaselineCollector::collect(
            workspace.id.as_uuid(),
            generations,
            structural_hash,
            affected_listeners,
            android_owners,
            packages,
            material_inventory,
        )
    }
}

fn inventory_baseline(reference: &str, generation: u64) -> EnvironmentMaterialInventoryBaseline {
    let mut fingerprint = [0_u8; 32];
    fingerprint[..8].copy_from_slice(&generation.to_be_bytes());
    EnvironmentMaterialInventoryBaseline::observed(reference.to_owned(), fingerprint)
}
