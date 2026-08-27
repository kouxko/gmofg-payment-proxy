use crate::{
    EnvironmentApplyGenerations, EnvironmentConfigurationCandidateV1, ProxyWorkspace,
    parse_environment_configuration_candidate_v1,
};
pub(crate) use crate::{
    EnvironmentApplyWork, EnvironmentCancelStatus, EnvironmentCandidateEpoch,
    EnvironmentCandidateId, EnvironmentCandidateLifecycleError, EnvironmentCandidatePolicy,
    EnvironmentCandidatePublicSnapshot, EnvironmentCandidateRegistry, EnvironmentCandidateStatus,
    EnvironmentConfirmationToken, EnvironmentDiagnostic, EnvironmentStatusCode,
    EnvironmentValidatedApplyBaseline, EnvironmentValidationLayerResult,
};
use serde::Serialize;
use serde_json::Value;

const FULL_SHAPE: &[u8] = include_bytes!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../src/mcp/tests/fixtures/environment_configuration_candidate_v1/full-shape.json"
));
const EXPECTED_PREVIEW: &[u8] = include_bytes!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../src/mcp/tests/fixtures/environment_configuration_candidate_v1/expected-preview.json"
));

pub(crate) fn candidate() -> EnvironmentConfigurationCandidateV1 {
    candidate_named("Store Lab")
}

pub(crate) fn candidate_named(name: &str) -> EnvironmentConfigurationCandidateV1 {
    let mut value: Value = serde_json::from_slice(FULL_SHAPE).expect("fixture is valid JSON");
    value["target"]["name"] = Value::String(name.to_owned());
    parse_environment_configuration_candidate_v1(&serde_json::to_vec(&value).unwrap())
        .expect("the named G033 candidate remains valid")
}

pub(crate) fn expected_preview_value() -> Value {
    serde_json::from_slice(EXPECTED_PREVIEW).expect("expected preview is valid JSON")
}

pub(crate) fn public_snapshot() -> EnvironmentCandidatePublicSnapshot {
    EnvironmentCandidatePublicSnapshot::from_validated_json(EXPECTED_PREVIEW)
        .expect("G033 expected preview is a typed snapshot")
}

pub(crate) fn public_snapshot_named(name: &str) -> EnvironmentCandidatePublicSnapshot {
    let mut value = expected_preview_value();
    value["target_key"] = Value::String(format!("public:{name}"));
    value["target"]["name"] = Value::String(name.to_owned());
    EnvironmentCandidatePublicSnapshot::from_validated_json(&serde_json::to_vec(&value).unwrap())
        .expect("named preview remains a typed public snapshot")
}

pub(crate) fn public_snapshot_with_padding(
    name: &str,
    public_label_bytes: usize,
) -> EnvironmentCandidatePublicSnapshot {
    let mut value = expected_preview_value();
    value["target"]["name"] = Value::String(name.to_owned());
    value["materials_public"]["certificates"][0]["label"] =
        Value::String("p".repeat(public_label_bytes));
    EnvironmentCandidatePublicSnapshot::from_validated_json(&serde_json::to_vec(&value).unwrap())
        .expect("public padding remains a typed validated snapshot")
}

pub(crate) fn registry() -> EnvironmentCandidateRegistry {
    EnvironmentCandidateRegistry::new(EnvironmentCandidatePolicy::default())
}

pub(crate) fn sealed_baseline() -> EnvironmentValidatedApplyBaseline {
    EnvironmentValidatedApplyBaseline::validated(
        EnvironmentApplyGenerations {
            application_mutation: 1,
            ..EnvironmentApplyGenerations::default()
        },
        [1; 32],
        Vec::new(),
        Vec::new(),
        Vec::new(),
        Vec::new(),
    )
}

pub(crate) fn validated_workspace() -> ProxyWorkspace {
    let workspace = ProxyWorkspace::default();
    workspace.validate().expect("fixture Workspace validates");
    workspace
}

pub(crate) fn complete_preview_ready(
    registry: &EnvironmentCandidateRegistry,
    candidate_id: &EnvironmentCandidateId,
    snapshot: EnvironmentCandidatePublicSnapshot,
) -> Result<crate::EnvironmentCandidateCreateResult, EnvironmentCandidateLifecycleError> {
    registry.attach_validated_apply_baseline(candidate_id, sealed_baseline())?;
    registry.complete_preview_ready(candidate_id, snapshot, validated_workspace())
}

pub(crate) fn json(value: &impl Serialize) -> Value {
    serde_json::to_value(value).expect("public lifecycle output serializes")
}

pub(crate) fn token_from_create(
    create: &crate::EnvironmentCandidateCreateResult,
) -> EnvironmentConfirmationToken {
    let raw = json(create)["confirmation_token"]
        .as_str()
        .expect("preview-ready create exposes a token")
        .to_owned();
    EnvironmentConfirmationToken::new(raw).expect("serialized token parses for apply input")
}

pub(crate) fn insert_validating(
    registry: &EnvironmentCandidateRegistry,
    name: &str,
    epoch: u64,
) -> crate::EnvironmentCandidateCreateResult {
    registry
        .insert_validating(candidate_named(name), EnvironmentCandidateEpoch::new(epoch))
        .expect("candidate is admitted")
}

pub(crate) fn admit_preview_ready(
    registry: &EnvironmentCandidateRegistry,
    name: &str,
    epoch: u64,
) -> crate::EnvironmentCandidateCreateResult {
    let admitted = insert_validating(registry, name, epoch);
    complete_preview_ready(
        registry,
        admitted.candidate_id(),
        public_snapshot_named(name),
    )
    .expect("prevalidated candidate becomes preview-ready")
}

pub(crate) fn claim_apply(
    registry: &EnvironmentCandidateRegistry,
    name: &str,
) -> (
    crate::EnvironmentCandidateCreateResult,
    EnvironmentApplyWork,
) {
    let ready = admit_preview_ready(registry, name, 1);
    registry
        .queue_apply(
            ready.candidate_id(),
            ready.confirmation_token().expect("token exists"),
        )
        .expect("apply queues");
    let work = registry
        .claim_next_apply()
        .expect("worker observes the FIFO")
        .expect("worker takes cleanup ownership");
    (ready, work)
}

pub(crate) const fn failed_layer() -> EnvironmentValidationLayerResult {
    EnvironmentValidationLayerResult::failed(0)
}

pub(crate) const fn validation_diagnostic() -> EnvironmentDiagnostic {
    EnvironmentDiagnostic::error(EnvironmentStatusCode::ValidationLayerFailed)
}

pub(crate) fn fail_validation(
    registry: &EnvironmentCandidateRegistry,
    candidate_id: &EnvironmentCandidateId,
) -> Result<(), EnvironmentCandidateLifecycleError> {
    registry.complete_validation_failed(
        candidate_id,
        vec![failed_layer()],
        vec![validation_diagnostic()],
    )
}
