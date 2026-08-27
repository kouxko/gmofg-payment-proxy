use std::{fs, path::Path};

fn rust_sources(root: &Path) -> String {
    let mut paths = fs::read_dir(root)
        .expect("source directory")
        .map(|entry| entry.expect("source entry").path())
        .collect::<Vec<_>>();
    paths.sort();
    paths
        .into_iter()
        .map(|path| {
            if path.is_dir()
                && matches!(
                    path.file_name().and_then(|name| name.to_str()),
                    Some("requirements_tests" | "tests")
                )
            {
                String::new()
            } else if path.is_dir() {
                rust_sources(&path)
            } else if path.extension().is_some_and(|extension| extension == "rs") {
                fs::read_to_string(path).expect("Rust source")
            } else {
                String::new()
            }
        })
        .collect::<Vec<_>>()
        .join("\n")
}

fn application_sources() -> String {
    rust_sources(Path::new(env!("CARGO_MANIFEST_DIR")).join("src").as_path())
}

fn candidate_apply_sources() -> String {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
    [
        root.join("environment_configuration"),
        root.join("facade/environment_candidates.rs"),
    ]
    .into_iter()
    .map(|path| {
        if path.is_dir() {
            rust_sources(&path)
        } else {
            fs::read_to_string(path).expect("candidate apply source")
        }
    })
    .collect::<Vec<_>>()
    .join("\n")
}

fn declaration_prefix<'a>(source: &'a str, name: &str) -> &'a str {
    let declaration = source
        .find(&format!("struct {name}"))
        .unwrap_or_else(|| panic!("missing `{name}`"));
    let prefix_start = source[..declaration]
        .rfind("\n\n")
        .map_or(0, |offset| offset + 2);
    &source[prefix_start..declaration]
}

#[test]
fn application_defines_the_three_g038_ports() {
    let source = application_sources();
    for required in [
        "trait EnvironmentApplyLeasePort",
        "trait EnvironmentProtectedMaterialPreparePort",
        "trait EnvironmentCommitPort",
    ] {
        assert!(source.contains(required), "missing `{required}`");
    }
}

#[test]
fn protected_material_handles_are_opaque_application_capabilities() {
    let source = application_sources();
    let required = "StagedProtectedMaterialHandle";
    assert!(source.contains(required), "missing `{required}`");
    let prefix = declaration_prefix(&source, required);
    assert!(
        !prefix.contains("Debug"),
        "{required} must not derive Debug"
    );
    assert!(
        !prefix.contains("Serialize") && !prefix.contains("Deserialize"),
        "{required} must not derive serde traits"
    );
    assert!(
        !source.contains(&format!("impl serde::Serialize for {required}")),
        "{required} must not serialize"
    );
    assert!(
        !source.contains(&format!("impl std::fmt::Debug for {required}")),
        "{required} must not expose Debug"
    );
    assert!(source.contains("trait EnvironmentPreparedMaterialCapability"));
    assert!(!source.contains("PreparedProtectedMaterialHandle"));
    assert!(!source.contains("pub fn into_candidate_json"));
    assert!(!source.contains("pub fn into_record"));
    assert!(!source.contains("pub trait PreparedProtectedMaterialCommitSink"));
    assert!(!source.contains("pub fn commit_into"));
}

#[test]
fn commit_receipt_has_no_public_constructor_or_public_fields() {
    let source = application_sources();
    assert!(source.contains("struct EnvironmentCommitReceipt"));
    assert!(!source.contains("pub fn new_environment_commit_receipt"));
    assert!(!source.contains("pub struct EnvironmentCommitReceipt {\n    pub "));
}

#[test]
fn candidate_apply_does_not_use_legacy_partial_persistence_paths() {
    let source = candidate_apply_sources();
    for forbidden in [
        ".store_basic_auth(",
        ".restore_portable(",
        ".workspace_save(",
        ".workspace_create(",
        ".workspace_import(",
        "SqliteStore",
        "rusqlite",
    ] {
        assert!(
            !source.contains(forbidden),
            "candidate apply must not use `{forbidden}`"
        );
    }
}

#[test]
fn apply_worker_orders_lease_then_prepare_then_commit() {
    let source = candidate_apply_sources();
    let lease = source.find(".acquire(").expect("lease acquisition");
    let prepare = source.find(".prepare(").expect("material preparation");
    let commit = source.find(".commit(").expect("atomic commit");
    assert!(lease < prepare && prepare < commit);
}

#[test]
fn lease_contract_names_all_guarded_generation_families() {
    let source = candidate_apply_sources();
    for required in [
        "listener",
        "android",
        "package",
        "certificate_inventory",
        "protected_secret_inventory",
        "application_mutation",
    ] {
        assert!(source.contains(required), "lease omits `{required}`");
    }
}

#[test]
fn apply_worker_preserves_package_stale_precedence_over_generic_lease_mismatch() {
    let source = candidate_apply_sources();
    let stale = source
        .find("CandidateStale")
        .expect("package stale outcome");
    let mismatch = source
        .find("ApplyLeaseMismatch")
        .expect("generic lease mismatch outcome");
    assert!(stale < mismatch, "package-specific stale check must win");
}

#[test]
fn apply_worker_has_owned_cancel_and_shutdown_terminal_paths() {
    let source = candidate_apply_sources();
    for required in [
        "CandidateCancelled",
        "CandidateCancelledByShutdown",
        "ProtectedMaterialPrepareFailed",
        "CommitRolledBack",
    ] {
        assert!(
            source.contains(required),
            "missing `{required}` terminal path"
        );
    }
}
