use std::collections::BTreeSet;

use uuid::Uuid;

use super::EnvironmentApplyGenerations;
use crate::{AppError, AppResult, ENVIRONMENT_VALIDATION_ENGINE_VERSION, ProtocolPackageRef};

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct EnvironmentAffectedListenerBaseline {
    listener_id: Uuid,
    runtime_epoch: Option<Uuid>,
    active_count: u32,
}

impl EnvironmentAffectedListenerBaseline {
    pub fn observed(listener_id: Uuid, runtime_epoch: Option<Uuid>, active_count: u32) -> Self {
        Self {
            listener_id,
            runtime_epoch,
            active_count,
        }
    }

    pub fn listener_id(&self) -> Uuid {
        self.listener_id
    }

    pub fn runtime_epoch(&self) -> Option<Uuid> {
        self.runtime_epoch
    }

    pub const fn active_count(&self) -> u32 {
        self.active_count
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct EnvironmentAndroidOwnerBaseline {
    profile_id: String,
    serial: String,
    owner_epoch: Uuid,
    state: String,
}

impl EnvironmentAndroidOwnerBaseline {
    pub fn observed(profile_id: String, serial: String, owner_epoch: Uuid, state: String) -> Self {
        Self {
            profile_id,
            serial,
            owner_epoch,
            state,
        }
    }

    pub fn profile_id(&self) -> &str {
        &self.profile_id
    }

    pub fn serial(&self) -> &str {
        &self.serial
    }

    pub fn owner_epoch(&self) -> Uuid {
        self.owner_epoch
    }

    pub fn state(&self) -> &str {
        &self.state
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct EnvironmentExactPackageBaseline {
    package: ProtocolPackageRef,
    generation: Uuid,
    enabled: bool,
    online: bool,
    service_epoch: u64,
    description_fingerprint: [u8; 32],
    online_generation: u64,
    lease_generation: u64,
}

impl EnvironmentExactPackageBaseline {
    pub fn observed(
        package: ProtocolPackageRef,
        generation: Uuid,
        enabled: bool,
        online: bool,
    ) -> Self {
        Self::observed_projection(package, generation, enabled, online, 0, [0; 32], 0, 0)
    }

    #[allow(clippy::too_many_arguments)]
    pub fn observed_projection(
        package: ProtocolPackageRef,
        generation: Uuid,
        enabled: bool,
        online: bool,
        service_epoch: u64,
        description_fingerprint: [u8; 32],
        online_generation: u64,
        lease_generation: u64,
    ) -> Self {
        Self {
            package,
            generation,
            enabled,
            online,
            service_epoch,
            description_fingerprint,
            online_generation,
            lease_generation,
        }
    }

    pub fn package_id(&self) -> &str {
        self.package.id.as_str()
    }

    pub fn version(&self) -> &str {
        self.package.version.as_str()
    }

    pub fn package_ref(&self) -> &ProtocolPackageRef {
        &self.package
    }

    pub fn generation(&self) -> Uuid {
        self.generation
    }

    pub const fn enabled(&self) -> bool {
        self.enabled
    }

    pub const fn online(&self) -> bool {
        self.online
    }

    pub const fn service_epoch(&self) -> u64 {
        self.service_epoch
    }

    pub const fn description_fingerprint(&self) -> &[u8; 32] {
        &self.description_fingerprint
    }

    pub const fn online_generation(&self) -> u64 {
        self.online_generation
    }

    pub const fn lease_generation(&self) -> u64 {
        self.lease_generation
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct EnvironmentMaterialInventoryBaseline {
    reference: String,
    fingerprint: [u8; 32],
}

impl EnvironmentMaterialInventoryBaseline {
    pub fn observed(reference: String, fingerprint: [u8; 32]) -> Self {
        Self {
            reference,
            fingerprint,
        }
    }

    #[cfg(test)]
    pub(crate) fn frozen(reference: String, fingerprint: [u8; 32]) -> Self {
        Self::observed(reference, fingerprint)
    }

    pub fn reference(&self) -> &str {
        &self.reference
    }

    pub fn fingerprint(&self) -> &[u8; 32] {
        &self.fingerprint
    }
}

/// Full-shape collector used after validation has observed every guarded resource. Callers cannot
/// construct a partial baseline: all persisted and runtime families are mandatory inputs, and the
/// collector canonicalizes identities before sealing the authority value.
#[derive(Clone, Copy, Debug, Default)]
pub struct EnvironmentValidatedApplyBaselineCollector;

impl EnvironmentValidatedApplyBaselineCollector {
    pub fn collect(
        workspace_id: Uuid,
        generations: EnvironmentApplyGenerations,
        workspace_structural_hash: [u8; 32],
        mut affected_listeners: Vec<EnvironmentAffectedListenerBaseline>,
        mut android_owners: Vec<EnvironmentAndroidOwnerBaseline>,
        mut exact_packages: Vec<EnvironmentExactPackageBaseline>,
        mut material_inventory: Vec<EnvironmentMaterialInventoryBaseline>,
    ) -> AppResult<EnvironmentValidatedApplyBaseline> {
        if workspace_id.is_nil()
            || generations == EnvironmentApplyGenerations::default()
            || workspace_structural_hash == [0; 32]
            || material_inventory.is_empty()
        {
            return Err(invalid_baseline());
        }
        affected_listeners.sort_by_key(EnvironmentAffectedListenerBaseline::listener_id);
        android_owners.sort_by(|left, right| {
            left.profile_id()
                .cmp(right.profile_id())
                .then_with(|| left.serial().cmp(right.serial()))
        });
        exact_packages.sort_by(|left, right| {
            left.package_id()
                .cmp(right.package_id())
                .then_with(|| {
                    crate::ProtocolPackageVersion::semantic_cmp(
                        &left.package.version,
                        &right.package.version,
                    )
                })
                .then_with(|| left.version().cmp(right.version()))
        });
        material_inventory.sort_by(|left, right| left.reference().cmp(right.reference()));
        ensure_unique(
            affected_listeners
                .iter()
                .map(EnvironmentAffectedListenerBaseline::listener_id),
        )?;
        ensure_unique(
            android_owners
                .iter()
                .map(|owner| (owner.profile_id(), owner.serial())),
        )?;
        ensure_unique(
            exact_packages
                .iter()
                .map(|package| (package.package_id(), package.version())),
        )?;
        ensure_unique(
            material_inventory
                .iter()
                .map(EnvironmentMaterialInventoryBaseline::reference),
        )?;
        if android_owners.iter().any(|owner| {
            owner.profile_id().trim().is_empty()
                || owner.serial().trim().is_empty()
                || owner.state().trim().is_empty()
        }) {
            return Err(invalid_baseline());
        }
        let mut baseline = EnvironmentValidatedApplyBaseline::validated(
            generations,
            workspace_structural_hash,
            affected_listeners,
            android_owners,
            exact_packages,
            material_inventory,
        );
        baseline.bind_target_workspace(workspace_id);
        Ok(baseline)
    }
}

fn ensure_unique<T: Ord>(values: impl IntoIterator<Item = T>) -> AppResult<()> {
    let mut seen = BTreeSet::new();
    for value in values {
        if !seen.insert(value) {
            return Err(invalid_baseline());
        }
    }
    Ok(())
}

fn invalid_baseline() -> AppError {
    AppError::new(
        "ENVIRONMENT_APPLY_BASELINE_INVALID",
        "环境配置验证基线不完整或包含重复资源。",
    )
}

/// Frozen authority produced by validation and consumed exactly once by the apply worker.
/// Its fields stay private so callers cannot synthesize a partial baseline with defaults.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct EnvironmentValidatedApplyBaseline {
    target_workspace_id: Option<Uuid>,
    candidate_schema_version: u8,
    validation_engine_version: u32,
    pub(super) generations: EnvironmentApplyGenerations,
    pub(super) workspace_structural_hash: [u8; 32],
    pub(super) affected_listeners: Vec<EnvironmentAffectedListenerBaseline>,
    pub(super) android_owners: Vec<EnvironmentAndroidOwnerBaseline>,
    pub(super) exact_packages: Vec<EnvironmentExactPackageBaseline>,
    pub(super) material_inventory: Vec<EnvironmentMaterialInventoryBaseline>,
}

impl EnvironmentValidatedApplyBaseline {
    pub(crate) fn validated(
        generations: EnvironmentApplyGenerations,
        workspace_structural_hash: [u8; 32],
        affected_listeners: Vec<EnvironmentAffectedListenerBaseline>,
        android_owners: Vec<EnvironmentAndroidOwnerBaseline>,
        exact_packages: Vec<EnvironmentExactPackageBaseline>,
        material_inventory: Vec<EnvironmentMaterialInventoryBaseline>,
    ) -> Self {
        Self {
            target_workspace_id: None,
            candidate_schema_version: 1,
            validation_engine_version: ENVIRONMENT_VALIDATION_ENGINE_VERSION,
            generations,
            workspace_structural_hash,
            affected_listeners,
            android_owners,
            exact_packages,
            material_inventory,
        }
    }

    pub fn generations(&self) -> &EnvironmentApplyGenerations {
        &self.generations
    }

    pub const fn candidate_schema_version(&self) -> u8 {
        self.candidate_schema_version
    }

    pub const fn validation_engine_version(&self) -> u32 {
        self.validation_engine_version
    }

    pub fn target_workspace_id(&self) -> Option<Uuid> {
        self.target_workspace_id
    }

    pub(crate) fn bind_target_workspace(&mut self, workspace_id: Uuid) {
        self.target_workspace_id = Some(workspace_id);
    }

    pub fn workspace_structural_hash(&self) -> &[u8; 32] {
        &self.workspace_structural_hash
    }

    pub fn affected_listeners(&self) -> &[EnvironmentAffectedListenerBaseline] {
        &self.affected_listeners
    }

    pub fn android_owners(&self) -> &[EnvironmentAndroidOwnerBaseline] {
        &self.android_owners
    }

    pub fn exact_packages(&self) -> &[EnvironmentExactPackageBaseline] {
        &self.exact_packages
    }

    pub fn material_inventory(&self) -> &[EnvironmentMaterialInventoryBaseline] {
        &self.material_inventory
    }
}
