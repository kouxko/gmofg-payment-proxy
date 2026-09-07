const LIFECYCLE_MOD: &str = include_str!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/src/environment_configuration/lifecycle/mod.rs"
));
const REGISTRY_SOURCE: &str = include_str!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/src/environment_configuration/lifecycle/registry.rs"
));
const TYPES_SOURCE: &str = include_str!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/src/environment_configuration/lifecycle/types.rs"
));
const SNAPSHOT_SOURCE: &str = include_str!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/src/environment_configuration/lifecycle/snapshot.rs"
));
const TERMINAL_SOURCE: &str = include_str!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/src/environment_configuration/terminal.rs"
));

#[test]
fn public_lifecycle_exports_no_forgeable_target_key() {
    assert!(!LIFECYCLE_MOD.contains("EnvironmentTargetKey"));
}

#[test]
fn public_lifecycle_exports_no_forgeable_committed_completion() {
    assert!(!LIFECYCLE_MOD.contains("EnvironmentApplyCompletion"));
    assert!(!TYPES_SOURCE.contains("pub enum EnvironmentApplyCompletion"));
}

#[test]
fn apply_worker_has_no_arbitrary_task_id_claim_surface() {
    assert!(!REGISTRY_SOURCE.contains("claim_queued_apply"));
    assert!(REGISTRY_SOURCE.contains("claim_next_apply"));
}

#[test]
fn terminal_size_serialization_failure_is_not_masked_as_usize_max() {
    assert!(!REGISTRY_SOURCE.contains("usize::MAX"));
    assert!(!REGISTRY_SOURCE.contains("unwrap_or(usize::MAX)"));
    assert!(!REGISTRY_SOURCE.contains("map_or(usize::MAX"));
}

#[test]
fn public_snapshot_is_serialize_only_and_state_advancement_is_crate_sealed() {
    assert!(!SNAPSHOT_SOURCE.contains(
        "Debug, Deserialize, PartialEq, Serialize)]\npub struct EnvironmentCandidatePublicSnapshot"
    ));
    assert!(REGISTRY_SOURCE.contains("pub(crate) fn complete_preview_ready"));
    assert!(REGISTRY_SOURCE.contains("pub(crate) fn complete_validation_failed"));
    assert!(REGISTRY_SOURCE.contains("pub(crate) fn claim_next_apply"));
}

#[test]
fn public_diagnostics_have_no_deserialize_or_arbitrary_message_constructor() {
    assert!(!TERMINAL_SOURCE.contains("Debug, Deserialize, PartialEq, Serialize)]\n#[serde(deny_unknown_fields)]\npub struct EnvironmentDiagnostic"));
    assert!(!TERMINAL_SOURCE.contains("message: impl Into<String>"));
    assert!(!TERMINAL_SOURCE.contains("pub message: String"));
    assert!(!TERMINAL_SOURCE.contains("pub field: Option<String>"));
}
