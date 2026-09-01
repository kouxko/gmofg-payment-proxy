use intercept_proxy_application::{AppResult, ProtocolPackageRef, ProtocolPackageVersionViewModel};
use uuid::Uuid;

use super::{
    EnvironmentApplyLeaseResourceObservation, EnvironmentApplyRuntimeAdapter, package_unavailable,
};

impl EnvironmentApplyRuntimeAdapter {
    pub(super) async fn observe_exact_package(
        &self,
        package: &ProtocolPackageRef,
    ) -> AppResult<EnvironmentApplyLeaseResourceObservation> {
        if let Some(current) = self
            .external_packages
            .environment_apply_projection(package)
            .await
            .map_err(|_| package_unavailable())?
        {
            let online = current.source.online();
            let projection = self.observe_projection(package, &current, online);
            return Ok(exact_observation(
                Uuid::from_u128(u128::from(projection.lease_generation)),
                &current,
                online,
                projection,
            ));
        }
        Err(package_unavailable())
    }

    fn observe_projection(
        &self,
        package: &ProtocolPackageRef,
        current: &ProtocolPackageVersionViewModel,
        online: bool,
    ) -> super::super::environment_apply_resources::ExactPackageProjection {
        let fingerprint = serde_json::to_string(current)
            .expect("typed package projection serialization cannot fail");
        self.resource_gates.observe_exact_package_projection(
            package,
            fingerprint,
            package_description_fingerprint(current),
            online,
        )
    }
}

fn exact_observation(
    generation: Uuid,
    current: &ProtocolPackageVersionViewModel,
    online: bool,
    projection: super::super::environment_apply_resources::ExactPackageProjection,
) -> EnvironmentApplyLeaseResourceObservation {
    EnvironmentApplyLeaseResourceObservation::ExactPackage {
        generation,
        enabled: current.enabled,
        online,
        service_epoch: projection.service_epoch,
        description_fingerprint: projection.description_fingerprint,
        online_generation: projection.online_generation,
        lease_generation: projection.lease_generation,
    }
}

fn package_description_fingerprint(package: &ProtocolPackageVersionViewModel) -> [u8; 32] {
    let description = serde_json::to_vec(&(
        &package.package,
        &package.name,
        package.host_api,
        &package.kind,
        &package.validation,
        package.installed_at,
    ))
    .expect("typed package description serialization cannot fail");
    ring::digest::digest(&ring::digest::SHA256, &description)
        .as_ref()
        .try_into()
        .expect("SHA-256 digest has a fixed 32-byte length")
}
