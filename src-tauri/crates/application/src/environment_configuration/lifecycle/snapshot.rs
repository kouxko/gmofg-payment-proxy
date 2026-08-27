use serde::{Deserialize, Serialize};
use std::fmt::Write as _;
use uuid::Uuid;

use crate::environment_configuration::EnvironmentValidationResult;
use crate::environment_configuration::{
    EnvironmentAdmittedTarget, EnvironmentStatusCode, EnvironmentValidationLayer,
    EnvironmentValidationStatus,
};

#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct EnvironmentCandidatePublicSnapshot {
    #[serde(skip)]
    schema_version: u8,
    #[serde(skip)]
    validation_engine_version: u32,
    target_key: String,
    target: EnvironmentPreviewTarget,
    baseline_public: EnvironmentBaselinePublic,
    validation_layers: Vec<EnvironmentValidationLayerResult>,
    resources: EnvironmentPreviewResources,
    alias_graph: EnvironmentAliasGraph,
    materials_public: EnvironmentMaterialsPublic,
    protocol_document_values: Vec<EnvironmentDocumentValue>,
    terminal_action_fields: EnvironmentTerminalActionFields,
}

impl EnvironmentCandidatePublicSnapshot {
    pub(crate) fn from_validated_json(bytes: &[u8]) -> Result<Self, serde_json::Error> {
        let wire: EnvironmentCandidatePublicSnapshotWire = serde_json::from_slice(bytes)?;
        let target_key = wire.target.public_key();
        Ok(Self {
            schema_version: 1,
            validation_engine_version: crate::ENVIRONMENT_VALIDATION_ENGINE_VERSION,
            target_key,
            target: wire.target,
            baseline_public: wire.baseline_public,
            validation_layers: wire
                .validation_layers
                .into_iter()
                .map(EnvironmentValidationLayerResult::from)
                .collect(),
            resources: wire.resources,
            alias_graph: wire.alias_graph,
            materials_public: wire.materials_public,
            protocol_document_values: wire.protocol_document_values,
            terminal_action_fields: wire.terminal_action_fields,
        })
    }

    pub(crate) fn admitted_target(&self) -> EnvironmentAdmittedTarget {
        self.target.admitted_target()
    }

    pub(crate) const fn schema_version(&self) -> u8 {
        self.schema_version
    }

    pub(crate) const fn validation_engine_version(&self) -> u32 {
        self.validation_engine_version
    }

    pub(super) fn into_parts(
        self,
    ) -> (
        String,
        EnvironmentBaselinePublic,
        Vec<EnvironmentValidationLayerResult>,
        EnvironmentCandidatePreview,
    ) {
        debug_assert_eq!(self.schema_version(), 1);
        debug_assert_eq!(
            self.validation_engine_version(),
            crate::ENVIRONMENT_VALIDATION_ENGINE_VERSION
        );
        (
            self.target_key,
            self.baseline_public,
            self.validation_layers,
            EnvironmentCandidatePreview {
                target: self.target,
                resources: self.resources,
                alias_graph: self.alias_graph,
                materials_public: self.materials_public,
                protocol_document_values: self.protocol_document_values,
                terminal_action_fields: self.terminal_action_fields,
            },
        )
    }
}

#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct EnvironmentCandidatePreview {
    target: EnvironmentPreviewTarget,
    resources: EnvironmentPreviewResources,
    alias_graph: EnvironmentAliasGraph,
    materials_public: EnvironmentMaterialsPublic,
    protocol_document_values: Vec<EnvironmentDocumentValue>,
    terminal_action_fields: EnvironmentTerminalActionFields,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(tag = "mode", rename_all = "snake_case", deny_unknown_fields)]
enum EnvironmentPreviewTarget {
    Existing {
        workspace_id: Uuid,
        expected_revision: u64,
    },
    New {
        name: String,
    },
}

impl EnvironmentPreviewTarget {
    fn admitted_target(&self) -> EnvironmentAdmittedTarget {
        match self {
            Self::Existing {
                workspace_id,
                expected_revision,
            } => EnvironmentAdmittedTarget::Existing {
                workspace_id: *workspace_id,
                expected_revision: *expected_revision,
            },
            Self::New { name } => EnvironmentAdmittedTarget::New {
                name: name.trim().to_owned(),
            },
        }
    }

    fn public_key(&self) -> String {
        exact_public_target_key(&self.admitted_target())
    }
}

pub(crate) fn exact_public_target_key(target: &EnvironmentAdmittedTarget) -> String {
    match target {
        EnvironmentAdmittedTarget::Existing { workspace_id, .. } => {
            format!("existing:{workspace_id}")
        }
        EnvironmentAdmittedTarget::New { name } => {
            let trimmed = name.trim();
            let mut key = String::with_capacity(4 + trimmed.len() * 2);
            key.push_str("new:");
            for byte in trimmed.as_bytes() {
                write!(&mut key, "{byte:02x}").expect("writing to a String cannot fail");
            }
            key
        }
    }
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct EnvironmentBaselinePublic {
    workspace_id: Option<Uuid>,
    revision: Option<u64>,
    selected: bool,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct EnvironmentValidationLayerResult {
    layer: EnvironmentValidationLayer,
    status: EnvironmentValidationStatus,
    code: Option<EnvironmentStatusCode>,
    reason: Option<&'static str>,
    duration_ms: u64,
}

impl EnvironmentValidationLayerResult {
    #[cfg(test)]
    pub(crate) const fn failed(duration_ms: u64) -> Self {
        Self {
            layer: EnvironmentValidationLayer::Domain,
            status: EnvironmentValidationStatus::Failed,
            code: Some(EnvironmentStatusCode::ValidationLayerFailed),
            reason: Some("environment validation layer failed"),
            duration_ms,
        }
    }

    pub(crate) fn from_orchestrated(result: &EnvironmentValidationResult) -> Self {
        Self {
            layer: result.layer(),
            status: result.status(),
            code: result.code(),
            reason: result.reason(),
            duration_ms: result.duration_ms(),
        }
    }
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct EnvironmentValidationLayerResultWire {
    layer: EnvironmentValidationLayer,
    status: EnvironmentValidationStatus,
    code: Option<EnvironmentStatusCode>,
    reason: Option<String>,
    duration_ms: u64,
}

impl From<EnvironmentValidationLayerResultWire> for EnvironmentValidationLayerResult {
    fn from(wire: EnvironmentValidationLayerResultWire) -> Self {
        let reason = wire
            .reason
            .map(|_| "environment validation detail is available through its status code");
        Self {
            layer: wire.layer,
            status: wire.status,
            code: wire.code,
            reason,
            duration_ms: wire.duration_ms,
        }
    }
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct EnvironmentCandidatePublicSnapshotWire {
    #[serde(rename = "target_key")]
    _target_key: String,
    target: EnvironmentPreviewTarget,
    baseline_public: EnvironmentBaselinePublic,
    validation_layers: Vec<EnvironmentValidationLayerResultWire>,
    resources: EnvironmentPreviewResources,
    alias_graph: EnvironmentAliasGraph,
    materials_public: EnvironmentMaterialsPublic,
    protocol_document_values: Vec<EnvironmentDocumentValue>,
    terminal_action_fields: EnvironmentTerminalActionFields,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
struct EnvironmentPreviewResources {
    listeners: Vec<EnvironmentPreviewListener>,
    http_rules: Vec<EnvironmentPreviewHttpRule>,
    protocol_rules: Vec<EnvironmentPreviewProtocolRule>,
    android_profile_ids: Vec<String>,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
struct EnvironmentPreviewListener {
    alias: String,
    candidate_local_id: Uuid,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
struct EnvironmentPreviewHttpRule {
    candidate_index: usize,
    candidate_local_id: Uuid,
    created_order: u64,
    listener_alias: String,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
struct EnvironmentPreviewProtocolRule {
    candidate_index: usize,
    candidate_local_id: Uuid,
    created_order: u64,
    listener_alias: String,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
struct EnvironmentAliasGraph {
    certificate_aliases: Vec<String>,
    secret_aliases: Vec<String>,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
struct EnvironmentMaterialsPublic {
    certificates: Vec<EnvironmentCertificatePublic>,
    secrets: Vec<EnvironmentSecretPublic>,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
struct EnvironmentCertificatePublic {
    alias: String,
    role: EnvironmentCertificateRole,
    encoding: EnvironmentCertificateEncoding,
    label: String,
}

#[derive(Clone, Copy, Debug, Deserialize, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
enum EnvironmentCertificateRole {
    DownstreamServerIdentity,
    DownstreamClientTrust,
    UpstreamClientIdentity,
    UpstreamServerTrust,
}

#[derive(Clone, Copy, Debug, Deserialize, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
enum EnvironmentCertificateEncoding {
    Pem,
    Base64Der,
    Pkcs12Base64,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
struct EnvironmentSecretPublic {
    alias: String,
    role: EnvironmentSecretRole,
    label: String,
}

#[derive(Clone, Copy, Debug, Deserialize, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
enum EnvironmentSecretRole {
    ProxyBasicAuth,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(tag = "type", content = "value", rename_all = "snake_case")]
enum EnvironmentDocumentValue {
    String(String),
    Int(i64),
    Bool(bool),
    Blob(Vec<u8>),
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
struct EnvironmentTerminalActionFields {
    #[serde(rename = "TruncateResponse")]
    truncate_response: Vec<String>,
    #[serde(rename = "DisconnectDuringUpstreamWrite")]
    disconnect_during_upstream_write: Vec<String>,
    #[serde(rename = "DisconnectDuringDownstreamWrite")]
    disconnect_during_downstream_write: Vec<String>,
}
