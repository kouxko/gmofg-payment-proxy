use chrono::Utc;
use intercept_proxy_application::{
    EnvironmentApplyGenerations, EnvironmentCommitRequest, EnvironmentCommitTarget,
    EnvironmentSelectionPolicy,
};
use intercept_proxy_domain::ProxyWorkspace;
use serde_json::{Value, json};
use uuid::Uuid;

use crate::adapters::common::{decode_workspace_record, encode_workspace_record};
use crate::sqlite::{EnvironmentCommitFaultPoint, SqliteStore, WorkspaceRecord};

fn baseline(selected_workspace_id: Option<Uuid>) -> EnvironmentApplyGenerations {
    EnvironmentApplyGenerations {
        selected_workspace_id,
        ..Default::default()
    }
}

fn workspace_value(name: &str) -> Value {
    encode_workspace_record(&workspace(name)).expect("workspace serializes")
}

fn workspace(name: &str) -> ProxyWorkspace {
    ProxyWorkspace {
        name: name.into(),
        ..ProxyWorkspace::default()
    }
}

fn request(
    target: EnvironmentCommitTarget,
    baseline: EnvironmentApplyGenerations,
) -> EnvironmentCommitRequest {
    EnvironmentCommitRequest::without_prepared_materials(
        baseline,
        target,
        workspace("candidate"),
        EnvironmentSelectionPolicy::PreserveExistingSelectionOrSelectNewWhenNone,
    )
}

fn assert_valid_workspace(record: WorkspaceRecord) -> ProxyWorkspace {
    decode_workspace_record(record).expect("stored Workspace uses the current persistence envelope")
}

fn seed_workspace(store: &SqliteStore, id: Uuid, name: &str) {
    let mut value = workspace_value(name);
    value["id"] = Value::String(id.to_string());
    value["revision"] = json!(1);
    store
        .insert_workspace(&WorkspaceRecord {
            id,
            revision: 1,
            value,
            updated_at: Utc::now(),
        })
        .expect("seed workspace");
}

#[test]
fn new_commit_persists_an_application_generated_identity_and_valid_workspace() {
    let store = SqliteStore::in_memory().expect("store");
    let generated_id = Uuid::new_v4();

    let result = store
        .commit_environment_configuration(
            request(
                EnvironmentCommitTarget::New {
                    workspace_id: generated_id,
                    display_name: "Created by Application".into(),
                },
                baseline(None),
            ),
            None,
        )
        .expect("new commit");

    assert_eq!(result.workspace_id, generated_id);
    assert_eq!(result.revision, 1);
    let snapshot = store.load_workspaces().expect("snapshot");
    assert_eq!(snapshot.selected_id, Some(generated_id));
    let workspace = assert_valid_workspace(snapshot.records[0].clone());
    assert_eq!(workspace.name, "Created by Application");
}

#[test]
fn existing_commit_advances_one_revision_and_preserves_other_selection() {
    let store = SqliteStore::in_memory().expect("store");
    let selected_id = Uuid::new_v4();
    let target_id = Uuid::new_v4();
    seed_workspace(&store, selected_id, "selected");
    seed_workspace(&store, target_id, "target");

    let result = store
        .commit_environment_configuration(
            request(
                EnvironmentCommitTarget::Existing {
                    workspace_id: target_id,
                    expected_revision: 1,
                },
                baseline(Some(selected_id)),
            ),
            None,
        )
        .expect("existing commit");

    assert_eq!(result.revision, 2);
    let snapshot = store.load_workspaces().expect("snapshot");
    assert_eq!(snapshot.selected_id, Some(selected_id));
    let record = snapshot
        .records
        .iter()
        .find(|record| record.id == target_id)
        .expect("target");
    assert_eq!(record.revision, 2);
    assert_valid_workspace(record.clone());
}

#[test]
fn workspace_structural_aba_is_rejected_even_when_revision_is_unchanged() {
    let store = SqliteStore::in_memory().expect("store");
    let id = Uuid::new_v4();
    seed_workspace(&store, id, "baseline");
    let frozen = baseline(Some(id));
    store
        .execute_test_batch(&format!(
            "UPDATE workspaces SET json = '{{\"id\":\"{id}\",\"revision\":1,\"name\":\"ABA\"}}' WHERE id = '{id}'"
        ))
        .expect("ABA mutation");

    let error = store
        .commit_environment_configuration(
            request(
                EnvironmentCommitTarget::Existing {
                    workspace_id: id,
                    expected_revision: 1,
                },
                frozen,
            ),
            None,
        )
        .expect_err("structural ABA must reject");

    assert_eq!(error.error().view_model.code, "COMMIT_BASELINE_MISMATCH");
}

#[test]
fn package_generation_aba_is_rejected_when_row_count_is_unchanged() {
    let store = SqliteStore::in_memory().expect("store");
    store.execute_test_batch(
        "INSERT INTO protocol_packages(package_id,version,name,host_api,kind,enabled,validation_state,validation_error_code,installed_at,generation)
         VALUES ('pkg','1.0.0','before',1,'socket',1,'valid',NULL,'2026-08-26T00:00:00Z','generation-a')",
    ).expect("seed package");
    let frozen = EnvironmentApplyGenerations {
        package_inventory: 1,
        ..baseline(None)
    };
    store.execute_test_batch(
        "UPDATE protocol_packages SET name='after', enabled=0, generation='generation-b' WHERE package_id='pkg'",
    ).expect("package ABA");

    let error = store
        .commit_environment_configuration(
            request(
                EnvironmentCommitTarget::New {
                    workspace_id: Uuid::new_v4(),
                    display_name: "new".into(),
                },
                frozen,
            ),
            None,
        )
        .expect_err("package ABA must reject");

    assert_eq!(error.error().view_model.code, "COMMIT_BASELINE_MISMATCH");
}

#[test]
fn secret_inventory_aba_is_rejected_when_row_count_is_unchanged() {
    let store = SqliteStore::in_memory().expect("store");
    store
        .execute_test_batch(
            "INSERT INTO protected_secrets(provider,secret_key,protected_blob,updated_at)
         VALUES ('system','same',X'01','2026-08-26T00:00:00Z')",
        )
        .expect("seed secret");
    let frozen = EnvironmentApplyGenerations {
        protected_secret_inventory: 1,
        ..baseline(None)
    };
    store.execute_test_batch(
        "UPDATE protected_secrets SET protected_blob=X'02' WHERE provider='system' AND secret_key='same'",
    ).expect("secret ABA");

    let error = store
        .commit_environment_configuration(
            request(
                EnvironmentCommitTarget::New {
                    workspace_id: Uuid::new_v4(),
                    display_name: "new".into(),
                },
                frozen,
            ),
            None,
        )
        .expect_err("secret ABA must reject");

    assert_eq!(error.error().view_model.code, "COMMIT_BASELINE_MISMATCH");
}

#[test]
fn certificate_inventory_aba_is_rejected_when_revision_is_unchanged() {
    let store = SqliteStore::in_memory().expect("store");
    store
        .execute_test_batch(
            "INSERT INTO certificate_material(kind,protected_blob,metadata_json,updated_at)
             VALUES ('same',X'01','{\"fingerprint\":\"before\"}','2026-08-26T00:00:00Z')",
        )
        .expect("seed certificate");
    let frozen = baseline(None);
    store
        .execute_test_batch(
            "UPDATE certificate_material SET protected_blob=X'02', metadata_json='{\"fingerprint\":\"after\"}' WHERE kind='same'",
        )
        .expect("certificate ABA");

    let error = store
        .commit_environment_configuration(
            request(
                EnvironmentCommitTarget::New {
                    workspace_id: Uuid::new_v4(),
                    display_name: "new".into(),
                },
                frozen,
            ),
            None,
        )
        .expect_err("certificate ABA must reject");

    assert_eq!(error.error().view_model.code, "COMMIT_BASELINE_MISMATCH");
}

fn assert_fault_rolls_back_without_residue(fault: EnvironmentCommitFaultPoint) {
    let store = SqliteStore::in_memory().expect("store");
    let id = Uuid::new_v4();
    let error = store
        .commit_environment_configuration(
            request(
                EnvironmentCommitTarget::New {
                    workspace_id: id,
                    display_name: "fault".into(),
                },
                baseline(None),
            ),
            Some(fault),
        )
        .expect_err("fault must roll back");
    assert_eq!(error.error().view_model.code, "COMMIT_ROLLED_BACK");
    assert!(
        store
            .load_workspaces()
            .expect("workspaces")
            .records
            .is_empty()
    );
    assert!(
        store
            .load_certificate_materials_snapshot(&[])
            .expect("certificates")
            .records
            .is_empty()
    );
    assert!(
        store
            .load_protected_secret("system", "same")
            .expect("secrets")
            .is_none()
    );
}

macro_rules! fault_case {
    ($name:ident, $fault:ident) => {
        #[test]
        fn $name() {
            assert_fault_rolls_back_without_residue(EnvironmentCommitFaultPoint::$fault);
        }
    };
}

fault_case!(
    certificate_insert_fault_leaves_zero_residue,
    BeforeCertificateInsert
);
fault_case!(secret_insert_fault_leaves_zero_residue, BeforeSecretInsert);
fault_case!(
    workspace_write_fault_leaves_zero_residue,
    BeforeWorkspaceWrite
);
fault_case!(
    selection_write_fault_leaves_zero_residue,
    BeforeSelectionWrite
);
fault_case!(commit_fault_leaves_zero_residue, BeforeCommit);
