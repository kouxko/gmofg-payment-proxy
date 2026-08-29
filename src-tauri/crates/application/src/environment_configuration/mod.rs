//! Strict Application-owned wire contract for MCP environment configuration.
//!
//! This module intentionally contains no candidate lifecycle or persistence behavior. It gives
//! later use cases one authoritative, fail-closed parser without exposing Domain persistence IDs
//! beyond the explicitly accepted existing-target selectors.

mod android;
mod apply;
mod identity;
mod lifecycle;
mod listener;
mod materials;
mod preview;
mod rules;
mod terminal;
mod validation;
mod workspace_projection;

pub const ENVIRONMENT_VALIDATION_ENGINE_VERSION: u32 = 1;

use std::collections::BTreeMap;
use std::collections::BTreeSet;

use serde::{Deserialize, Serialize};
use serde_json::Value;
use uuid::Uuid;

use crate::{AppError, AppResult};

use android::AndroidNetworkProfileTemplate;
use apply::PreparedMaterialCapabilityHandle;
pub use apply::{
    EnvironmentAffectedListenerBaseline, EnvironmentAndroidOwnerBaseline,
    EnvironmentApplyBaselineCapturePort, EnvironmentApplyBaselineCaptureRequest,
    EnvironmentApplyGenerations, EnvironmentApplyLease, EnvironmentApplyLeaseOutcome,
    EnvironmentApplyLeasePort, EnvironmentApplyLeaseRequest, EnvironmentCommitFailure,
    EnvironmentCommitPort, EnvironmentCommitReceipt, EnvironmentCommitRequest,
    EnvironmentCommitResult, EnvironmentCommitRollbackOutcome, EnvironmentCommitTarget,
    EnvironmentConsumedCommitRequest, EnvironmentConsumedPreparedMaterials,
    EnvironmentExactPackageBaseline, EnvironmentMaterialInventoryBaseline,
    EnvironmentPreparedMaterialCapability, EnvironmentPreparedMaterialKind,
    EnvironmentPreparedMaterialVisitor, EnvironmentPreparedMaterials,
    EnvironmentProtectedMaterialPreparePort, EnvironmentSelectionPolicy,
    EnvironmentValidatedApplyBaseline, EnvironmentValidatedApplyBaselineCollector, MaterialAlias,
    StagedProtectedMaterialHandle,
};
pub use identity::EnvironmentIdentityAllocator;
#[cfg(test)]
pub(crate) use identity::EnvironmentIdentityAllocatorPort;
pub use lifecycle::{
    EnvironmentApplyQueuedResult, EnvironmentApplyTaskId, EnvironmentCancelResult,
    EnvironmentCancelStatus, EnvironmentCandidateCreateResult, EnvironmentCandidateEpoch,
    EnvironmentCandidateId, EnvironmentCandidateLifecycleError, EnvironmentCandidateMetrics,
    EnvironmentCandidatePublicSnapshot, EnvironmentCandidateStatus,
    EnvironmentCandidateStatusResult, EnvironmentConfirmationToken,
    EnvironmentValidationLayerResult,
};
#[cfg(test)]
pub(crate) use lifecycle::{EnvironmentApplyWork, EnvironmentCandidatePolicy};
pub(crate) use lifecycle::{EnvironmentApplyWorker, EnvironmentCandidateRegistry};
use listener::ListenerTemplate;
use materials::EnvironmentMaterials;
pub(crate) use preview::candidate_preview_snapshot;
use rules::{HttpRuleTemplate, ProtocolDocumentRuleTemplate, RuleTemplate};
pub use terminal::{
    DiagnosticSeverity, EnvironmentDiagnostic, EnvironmentDiagnosticScope, EnvironmentStatusCode,
    EnvironmentTerminalResult,
};
#[cfg(test)]
pub(crate) use validation::EnvironmentCpuWorkProbe;
pub use validation::{
    EnvironmentCandidateValidator, EnvironmentDnsTcpTarget, EnvironmentMaterialProbe,
    EnvironmentMaterialProbeKind, EnvironmentTlsMtlsTarget, EnvironmentValidationLayer,
    EnvironmentValidationLayerPort, EnvironmentValidationLayerRequest, EnvironmentValidationReport,
    EnvironmentValidationResult, EnvironmentValidationStatus,
};
pub(crate) use validation::{
    EnvironmentDomainProjectionPort, EnvironmentPreviewBaselinePort,
    EnvironmentPreviewBaselineRequest, EnvironmentProjectedCandidate,
    EnvironmentValidationCheckpoint,
};
pub(crate) use workspace_projection::project_candidate_workspace;

#[derive(Debug, Deserialize, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct EnvironmentConfigurationCandidateV1 {
    schema_version: u8,
    target: EnvironmentTarget,
    workspace: WorkspaceCommitTemplate,
    materials: EnvironmentMaterials,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(tag = "mode", rename_all = "snake_case", deny_unknown_fields)]
enum EnvironmentTarget {
    Existing {
        workspace_id: Uuid,
        expected_revision: u64,
    },
    New {
        name: String,
    },
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
struct WorkspaceCommitTemplate {
    listeners: Vec<ListenerTemplate>,
    rules: Vec<RuleTemplate>,
    android_network_profiles: Vec<AndroidNetworkProfileTemplate>,
}

#[derive(Debug, thiserror::Error)]
pub enum EnvironmentConfigurationParseError {
    #[error("invalid environment configuration candidate: {0}")]
    InvalidJson(#[from] serde_json::Error),
    #[error("unsupported environment configuration candidate schema version")]
    UnsupportedSchemaVersion,
    #[error("new Workspace targets cannot retain persisted listener or rule identifiers")]
    PersistedIdentityForNewTarget,
    #[error("an existing rule selector may appear only once")]
    DuplicateRuleSelector,
    #[error("weak-network numeric values violate the v1 contract")]
    WeakNetworkValueInvalid,
    #[error("candidate contains an unknown field")]
    UnknownField,
    #[error("candidate contains a server-owned forbidden field")]
    ForbiddenField,
    #[error("weak-network wire is invalid")]
    WeakNetworkWireInvalid,
    #[error("certificate material role is unsupported")]
    UnsupportedMaterialRole,
    #[error("secret material role is unsupported")]
    UnsupportedSecretRole,
}

pub fn parse_environment_configuration_candidate_v1(
    bytes: &[u8],
) -> Result<EnvironmentConfigurationCandidateV1, EnvironmentConfigurationParseError> {
    let wire: Value = serde_json::from_slice(bytes)?;
    preflight_wire(&wire)?;
    let candidate: EnvironmentConfigurationCandidateV1 =
        serde_json::from_value(wire).map_err(|error| {
            if error.to_string().contains("unknown field") {
                EnvironmentConfigurationParseError::UnknownField
            } else {
                EnvironmentConfigurationParseError::InvalidJson(error)
            }
        })?;
    candidate.validate_selector_contract()?;
    Ok(candidate)
}

fn preflight_wire(wire: &Value) -> Result<(), EnvironmentConfigurationParseError> {
    if wire.get("validation_request").is_some() {
        return Err(EnvironmentConfigurationParseError::ForbiddenField);
    }
    let materials = &wire["materials"];
    if materials["certificates"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|material| material["role"].as_str())
        .any(|role| {
            !matches!(
                role,
                "downstream_server_identity"
                    | "downstream_client_trust"
                    | "upstream_client_identity"
                    | "upstream_server_trust"
            )
        })
    {
        return Err(EnvironmentConfigurationParseError::UnsupportedMaterialRole);
    }
    if materials["secrets"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|material| material["role"].as_str())
        .any(|role| role != "proxy_basic_auth")
    {
        return Err(EnvironmentConfigurationParseError::UnsupportedSecretRole);
    }
    let workspace = &wire["workspace"];
    if workspace["android_network_profiles"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|profile| profile["weak_network"].as_object())
        .filter_map(|weak| weak.get("burst_loss"))
        .any(|burst| !burst.is_null() && !burst.is_object())
    {
        return Err(EnvironmentConfigurationParseError::WeakNetworkWireInvalid);
    }
    Ok(())
}

impl EnvironmentConfigurationCandidateV1 {
    pub(crate) fn lifecycle_target(&self) -> EnvironmentAdmittedTarget {
        match &self.target {
            EnvironmentTarget::Existing {
                workspace_id,
                expected_revision,
            } => EnvironmentAdmittedTarget::Existing {
                workspace_id: *workspace_id,
                expected_revision: *expected_revision,
            },
            EnvironmentTarget::New { name } => EnvironmentAdmittedTarget::New {
                name: name.trim().to_owned(),
            },
        }
    }

    fn validate_selector_contract(&self) -> Result<(), EnvironmentConfigurationParseError> {
        if matches!(self.target, EnvironmentTarget::New { .. })
            && (self
                .workspace
                .listeners
                .iter()
                .any(|listener| listener.id.is_some())
                || self
                    .workspace
                    .rules
                    .iter()
                    .any(|rule| rule.existing_rule_id().is_some()))
        {
            return Err(EnvironmentConfigurationParseError::PersistedIdentityForNewTarget);
        }
        if self.schema_version != 1 {
            return Err(EnvironmentConfigurationParseError::UnsupportedSchemaVersion);
        }
        if self
            .workspace
            .android_network_profiles
            .iter()
            .any(|profile| !profile.validate_weak_network())
        {
            return Err(EnvironmentConfigurationParseError::WeakNetworkValueInvalid);
        }

        ensure_unique(
            self.workspace
                .rules
                .iter()
                .filter_map(RuleTemplate::existing_rule_id),
            EnvironmentConfigurationParseError::DuplicateRuleSelector,
        )
    }
}

impl StagedProtectedMaterialHandle {
    /// Parses the sealed candidate into its typed Application contract and only lends
    /// zeroizing serialized material records to the platform protector.
    pub fn prepare_with(
        self,
        mut protect: impl FnMut(
            &[u8],
            MaterialAlias,
            [u8; 32],
        ) -> AppResult<Box<dyn EnvironmentPreparedMaterialCapability>>,
    ) -> AppResult<EnvironmentPreparedMaterials> {
        let (candidate_json, workspace) = self.into_candidate_json();
        let candidate =
            parse_environment_configuration_candidate_v1(&candidate_json).map_err(|_| {
                AppError::new("PROTECTED_MATERIAL_PREPARE_FAILED", "受保护材料准备失败。")
            })?;
        let target = match candidate.target {
            EnvironmentTarget::Existing {
                workspace_id,
                expected_revision,
            } => EnvironmentCommitTarget::Existing {
                workspace_id,
                expected_revision,
            },
            EnvironmentTarget::New { name } => EnvironmentCommitTarget::New {
                workspace_id: workspace.id.as_uuid(),
                display_name: name.trim().to_owned(),
            },
        };
        let mut aliases = BTreeSet::new();
        let mut prepared_certificate_handles = BTreeMap::new();
        for material in candidate.materials.certificates {
            let alias = MaterialAlias::parse(material.alias.clone())?;
            if !aliases.insert(alias.clone()) {
                return Err(AppError::new(
                    "MATERIAL_ALIAS_DUPLICATE",
                    "受保护材料别名重复。",
                ));
            }
            let plaintext =
                zeroize::Zeroizing::new(serde_json::to_vec(&material).map_err(|_| {
                    AppError::new("PROTECTED_MATERIAL_PREPARE_FAILED", "受保护材料准备失败。")
                })?);
            let fingerprint = ring::digest::digest(&ring::digest::SHA256, &plaintext);
            let mut fingerprint_bytes = [0_u8; 32];
            fingerprint_bytes.copy_from_slice(fingerprint.as_ref());
            let handle = PreparedMaterialCapabilityHandle::from_capability(protect(
                &plaintext,
                alias.clone(),
                fingerprint_bytes,
            )?);
            prepared_certificate_handles.insert(alias, handle);
        }
        let mut prepared_secret_handles = BTreeMap::new();
        for material in candidate.materials.secrets {
            let alias = MaterialAlias::parse(material.alias.clone())?;
            if !aliases.insert(alias.clone()) {
                return Err(AppError::new(
                    "MATERIAL_ALIAS_DUPLICATE",
                    "受保护材料别名重复。",
                ));
            }
            let plaintext =
                zeroize::Zeroizing::new(serde_json::to_vec(&material).map_err(|_| {
                    AppError::new("PROTECTED_MATERIAL_PREPARE_FAILED", "受保护材料准备失败。")
                })?);
            let fingerprint = ring::digest::digest(&ring::digest::SHA256, &plaintext);
            let mut fingerprint_bytes = [0_u8; 32];
            fingerprint_bytes.copy_from_slice(fingerprint.as_ref());
            let handle = PreparedMaterialCapabilityHandle::from_capability(protect(
                &plaintext,
                alias.clone(),
                fingerprint_bytes,
            )?);
            prepared_secret_handles.insert(alias, handle);
        }
        Ok(EnvironmentPreparedMaterials::new(
            target,
            workspace,
            prepared_certificate_handles,
            prepared_secret_handles,
        ))
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) enum EnvironmentAdmittedTarget {
    Existing {
        workspace_id: Uuid,
        expected_revision: u64,
    },
    New {
        name: String,
    },
}

impl EnvironmentAdmittedTarget {
    pub(super) fn capacity_identity(&self) -> String {
        match self {
            Self::Existing { workspace_id, .. } => format!("existing:{workspace_id}"),
            Self::New { name } => {
                let mut encoded = String::with_capacity(name.len() * 2 + 4);
                encoded.push_str("new:");
                for byte in name.as_bytes() {
                    use std::fmt::Write as _;
                    write!(encoded, "{byte:02x}")
                        .expect("writing hexadecimal bytes to String cannot fail");
                }
                encoded
            }
        }
    }
}

fn ensure_unique(
    values: impl IntoIterator<Item = Uuid>,
    duplicate_error: EnvironmentConfigurationParseError,
) -> Result<(), EnvironmentConfigurationParseError> {
    let mut seen = BTreeSet::new();
    for value in values {
        if !seen.insert(value) {
            return Err(duplicate_error);
        }
    }
    Ok(())
}
