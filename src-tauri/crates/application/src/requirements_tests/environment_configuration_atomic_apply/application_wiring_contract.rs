use std::{fs, path::Path};

fn source(path: &str) -> String {
    fs::read_to_string(Path::new(env!("CARGO_MANIFEST_DIR")).join(path)).expect("source")
}

#[test]
fn application_dependencies_own_all_three_apply_ports() {
    let facade = source("src/facade.rs");
    let dependencies = facade
        .split("pub struct ApplicationDependencies")
        .nth(1)
        .and_then(|tail| tail.split("\n}").next())
        .expect("ApplicationDependencies body");
    for field in [
        "environment_apply_lease",
        "environment_material_preparer",
        "environment_commit",
        "environment_baseline_capture",
    ] {
        assert!(
            dependencies.contains(field),
            "missing owned `{field}` dependency"
        );
    }
}

#[test]
fn application_starts_owned_apply_without_caller_supplied_ports() {
    let facade = source("src/facade/environment_candidates.rs");
    assert!(facade.contains("fn start_next_environment_apply(&self)"));
    assert!(
        !facade
            .contains("environment_candidate_spawn_owned_apply(\n        &self,\n        prepare:")
    );
}

#[test]
fn preview_completion_captures_baseline_internally_for_a_valid_workspace_aggregate() {
    let registry = source("src/environment_configuration/lifecycle/registry.rs");
    let signature = registry
        .split("fn complete_preview_ready(")
        .nth(1)
        .and_then(|tail| tail.split(") ->").next())
        .expect("complete_preview_ready signature");
    assert!(signature.contains("ProxyWorkspace"));
    assert!(!signature.contains("EnvironmentValidatedApplyBaseline"));
    let facade = source("src/facade/environment_candidates.rs");
    assert!(facade.contains("environment_baseline_capture.capture("));
}

#[test]
fn candidate_material_never_derives_a_provisional_commit_baseline() {
    let candidate = source("src/environment_configuration/mod.rs");
    let state = source("src/environment_configuration/lifecycle/state.rs");
    assert!(!candidate.contains("provisional_apply_baseline"));
    assert!(!state.contains("candidate.provisional_apply_baseline()"));
}

#[test]
fn validated_baseline_constructor_is_not_public_api() {
    let baseline = source("src/environment_configuration/apply/baseline.rs");
    assert!(baseline.contains("fn validated("));
    assert!(!baseline.contains("pub fn validated("));
}

#[test]
fn worker_holds_application_mutation_gate_for_the_entire_terminal_transition() {
    let worker = source("src/environment_configuration/lifecycle/worker.rs");
    let acquire_gate = worker
        .find("mutation_gate.lock().await")
        .expect("gate acquisition");
    let lease = worker.find(".acquire(").expect("lease");
    let prepare = worker.find(".prepare(").expect("prepare");
    let commit = worker.find(".commit(").expect("commit");
    let terminal = worker.rfind("finish_committed").expect("terminalization");
    let release = worker.rfind("drop(mutation_guard)").expect("gate release");
    assert!(acquire_gate < lease && lease < prepare && prepare < commit);
    assert!(commit < terminal && terminal < release);
}

#[test]
fn commit_and_preparer_exchange_a_valid_proxy_workspace_not_raw_json() {
    let capability = source("src/environment_configuration/apply/capability.rs");
    assert!(capability.contains("pub workspace: ProxyWorkspace"));
    for forbidden in [
        "workspace_template: serde_json::Value",
        "WorkspaceCommitTemplate",
    ] {
        assert!(
            !capability.contains(forbidden),
            "raw commit DTO leaked: `{forbidden}`"
        );
    }
}

#[test]
fn lease_failures_have_exhaustive_stable_outcomes() {
    let apply = source("src/environment_configuration/apply.rs");
    for required in [
        "RuntimeActive",
        "AndroidOwnerMismatch",
        "PackageStale",
        "GenerationMismatch",
    ] {
        assert!(
            apply.contains(required),
            "missing stable lease outcome `{required}`"
        );
    }
    let worker = source("src/environment_configuration/lifecycle/worker.rs");
    assert!(!worker.contains("let Ok(lease) ="));
}

#[test]
fn worker_maps_lease_prepare_and_commit_through_typed_phase_outcomes_only() {
    let worker = source("src/environment_configuration/lifecycle/worker.rs");

    for forbidden in [
        "view_model.code.as_str()",
        "\"APPLY_RUNTIME_ACTIVE\"",
        "\"APPLY_ANDROID_OWNER_ACTIVE\"",
        "\"COMMIT_BASELINE_MISMATCH\"",
        "let Ok(prepared) =",
    ] {
        assert!(
            !worker.contains(forbidden),
            "worker branches on untyped phase data through `{forbidden}`"
        );
    }
}

#[test]
fn queue_and_start_is_one_application_owned_production_use_case() {
    let facade = source("src/facade/environment_candidates.rs");
    let start = facade
        .find("pub fn environment_candidate_queue_and_start_apply(")
        .expect("production queue-and-start use case");
    let body = &facade[start..];
    let queue = body.find(".queue_apply(").expect("queue transition");
    let spawn = body
        .find("self.start_next_environment_apply()")
        .expect("owned worker start");

    assert!(queue < spawn);
    assert!(
        !facade.contains("fn environment_candidate_queue_apply("),
        "queue-only Application entry point must not remain callable"
    );
}

#[test]
fn application_shutdown_wiring_begins_shutdown_and_waits_for_owned_apply_drain() {
    let facade = source("src/facade/environment_candidates.rs");
    let shutdown = facade
        .find("fn environment_candidate_shutdown_and_drain(")
        .expect("production shutdown-and-drain use case");
    let body = &facade[shutdown..];

    assert!(body.contains("environment_candidates.begin_shutdown()"));
    assert!(body.contains(".await"));
}

#[test]
fn baseline_contract_owns_typed_diff_classification_and_stable_resource_keys() {
    let apply = source("src/environment_configuration/apply.rs");

    for required in [
        "EnvironmentAffectedResourceDiff",
        "EnvironmentAffectedResourceKey",
        "EnvironmentResourceChangeKind",
        "Added",
        "Removed",
        "Changed",
        "Unchanged",
    ] {
        assert!(
            apply.contains(required),
            "missing typed diff token `{required}`"
        );
    }
}

#[test]
fn preview_capture_uses_validated_candidate_schema_and_engine_versions() {
    let facade = source("src/facade/environment_candidates.rs");

    assert!(
        !facade.contains("schema_version: 1,"),
        "preview capture hard-codes candidate schema instead of carrying validated authority"
    );
    assert!(facade.contains("validation_engine_version:"));
}

#[test]
fn commit_phase_classification_is_typed_and_never_uses_error_code_strings() {
    let apply = source("src/environment_configuration/apply.rs");
    let worker = source("src/environment_configuration/lifecycle/worker.rs");

    for required in ["BaselineMismatch", "Failed"] {
        assert!(
            apply.contains(required),
            "missing typed commit outcome `{required}`"
        );
    }
    assert!(!worker.contains("COMMIT_BASELINE_MISMATCH"));
    assert!(!worker.contains("error.view_model.code"));
    assert!(!apply.contains("\"COMMIT_BASELINE_MISMATCH\""));
    assert!(!apply.contains("error.view_model.code"));
}
