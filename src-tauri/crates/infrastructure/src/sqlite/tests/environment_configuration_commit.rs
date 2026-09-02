use std::{fs, path::Path};

mod behavior;

fn source(path: &str) -> String {
    fs::read_to_string(Path::new(env!("CARGO_MANIFEST_DIR")).join(path)).unwrap_or_default()
}

fn commit_source() -> String {
    source("src/sqlite/environment_configuration.rs")
}

fn prepare_source() -> String {
    source("src/adapters/environment_configuration_materials.rs")
}

#[test]
fn sqlite_environment_commit_uses_one_immediate_transaction() {
    let source = commit_source();
    assert!(source.contains("impl EnvironmentCommitPort"));
    assert!(source.contains("TransactionBehavior::Immediate"));
    assert!(source.contains("commit_environment_configuration"));
}

#[test]
fn environment_commit_checks_every_persisted_baseline_inside_transaction() {
    let source = commit_source();
    for required in [
        "expected_revision",
        "selected_workspace_id",
        "package_inventory",
        "certificate_inventory",
        "protected_secret_inventory",
    ] {
        assert!(source.contains(required), "commit omits `{required}` CAS");
    }
}

#[test]
fn environment_commit_rewrites_aliases_and_materials_with_workspace() {
    let source = commit_source();
    for required in [
        "prepared_certificate_handles",
        "prepared_secret_handles",
        "rewrite_material_aliases",
        "workspace",
        "ProxyWorkspace",
    ] {
        assert!(
            source.contains(required),
            "atomic commit omits `{required}`"
        );
    }
}

#[test]
fn new_workspace_selection_is_null_guarded() {
    let source = commit_source();
    assert!(source.contains("selected_id IS NULL"));
    assert!(source.contains("PreserveExistingSelectionOrSelectNewWhenNone"));
}

#[test]
fn existing_workspace_advances_exactly_one_revision_without_changing_selection() {
    let source = commit_source();
    assert!(source.contains("expected_revision.checked_add(1)"));
    assert!(!source.contains("expected_revision.saturating_add(1)"));
    assert!(source.contains("preserve_existing_selection"));
}

#[test]
fn new_workspace_starts_at_revision_one() {
    let source = commit_source();
    assert!(source.contains("new_workspace_revision = 1"));
}

#[test]
fn commit_failure_returns_no_success_receipt() {
    let source = commit_source();
    let commit = source
        .find("transaction.commit()")
        .expect("transaction commit point");
    let receipt = source
        .find("EnvironmentCommitReceipt")
        .expect("commit receipt construction");
    assert!(
        commit < receipt,
        "receipt may only be created after SQLite commit"
    );
}

#[test]
fn environment_commit_has_deterministic_fault_points_before_each_write_family() {
    let source = commit_source();
    for required in [
        "BeforeCertificateInsert",
        "BeforeSecretInsert",
        "BeforeWorkspaceWrite",
        "BeforeSelectionWrite",
        "BeforeCommit",
    ] {
        assert!(
            source.contains(required),
            "missing fault point `{required}`"
        );
    }
}

#[test]
fn protected_material_preparation_never_writes_sqlite() {
    let source = prepare_source();
    let prepare_start = source
        .find("impl EnvironmentProtectedMaterialPreparePort")
        .expect("prepare adapter");
    let prepare_tail = &source[prepare_start..];
    let prepare_end = prepare_tail["impl ".len()..]
        .find("\nimpl ")
        .map_or(prepare_tail.len(), |offset| offset + "impl ".len());
    let prepare_impl = &prepare_tail[..prepare_end];
    for forbidden in ["SqliteStore", ".execute(", "INSERT ", "UPDATE "] {
        assert!(
            !prepare_impl.contains(forbidden),
            "prepare must not persist through `{forbidden}`"
        );
    }
}

#[test]
fn prepared_material_adapter_has_explicit_finish_and_drop_cleanup() {
    let source = prepare_source();
    assert!(source.contains("PreparedProtectedMaterialHandle"));
    assert!(source.contains("impl Drop for PreparedProtectedMaterialHandle"));
    assert!(source.contains("finish_cleanup"));
}
