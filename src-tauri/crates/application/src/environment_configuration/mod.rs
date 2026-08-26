//! Strict Application-owned wire contract for MCP environment configuration.
//!
//! This module intentionally contains no candidate lifecycle or persistence behavior. It gives
//! later use cases one authoritative, fail-closed parser without exposing Domain persistence IDs
//! beyond the explicitly accepted existing-target selectors.

mod android;
mod lifecycle;
mod listener;
mod materials;
mod rules;
mod terminal;

use std::collections::BTreeSet;

use serde::{Deserialize, Serialize};
use uuid::Uuid;

use android::AndroidNetworkProfileTemplate;
#[cfg(test)]
pub(crate) use lifecycle::EnvironmentCandidatePolicy;
pub use lifecycle::{
    EnvironmentApplyQueuedResult, EnvironmentApplyTaskId, EnvironmentCancelResult,
    EnvironmentCancelStatus, EnvironmentCandidateCreateResult, EnvironmentCandidateEpoch,
    EnvironmentCandidateId, EnvironmentCandidateLifecycleError, EnvironmentCandidateMetrics,
    EnvironmentCandidatePublicSnapshot, EnvironmentCandidateStatus,
    EnvironmentCandidateStatusResult, EnvironmentConfirmationToken,
    EnvironmentValidationLayerResult,
};
pub(crate) use lifecycle::{EnvironmentApplyWork, EnvironmentCandidateRegistry};
use listener::ListenerTemplate;
use materials::EnvironmentMaterials;
use rules::{HttpRuleTemplate, ProtocolDocumentRuleTemplate};
pub use terminal::{
    DiagnosticSeverity, EnvironmentDiagnostic, EnvironmentDiagnosticScope, EnvironmentStatusCode,
    EnvironmentTerminalResult,
};

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
    http_rules: Vec<HttpRuleTemplate>,
    protocol_rules: Vec<ProtocolDocumentRuleTemplate>,
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
    #[error("an existing HTTP rule selector may appear only once")]
    DuplicateHttpRuleSelector,
    #[error("an existing protocol Document rule selector may appear only once")]
    DuplicateProtocolRuleSelector,
    #[error("weak-network numeric values violate the v1 contract")]
    WeakNetworkValueInvalid,
}

pub fn parse_environment_configuration_candidate_v1(
    bytes: &[u8],
) -> Result<EnvironmentConfigurationCandidateV1, EnvironmentConfigurationParseError> {
    let candidate: EnvironmentConfigurationCandidateV1 = serde_json::from_slice(bytes)?;
    candidate.validate_selector_contract()?;
    Ok(candidate)
}

impl EnvironmentConfigurationCandidateV1 {
    pub(super) fn lifecycle_target(&self) -> EnvironmentAdmittedTarget {
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
                    .http_rules
                    .iter()
                    .any(|rule| rule.existing_rule_id.is_some())
                || self
                    .workspace
                    .protocol_rules
                    .iter()
                    .any(|rule| rule.existing_rule_id.is_some()))
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
                .http_rules
                .iter()
                .filter_map(|rule| rule.existing_rule_id),
            EnvironmentConfigurationParseError::DuplicateHttpRuleSelector,
        )?;
        ensure_unique(
            self.workspace
                .protocol_rules
                .iter()
                .filter_map(|rule| rule.existing_rule_id),
            EnvironmentConfigurationParseError::DuplicateProtocolRuleSelector,
        )
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(super) enum EnvironmentAdmittedTarget {
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
