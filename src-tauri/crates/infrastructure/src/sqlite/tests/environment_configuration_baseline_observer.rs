use chrono::Utc;
use intercept_proxy_domain::ProxyWorkspace;
use serde_json::to_value;
use uuid::Uuid;

use crate::sqlite::{SqliteStore, WorkspaceRecord};

fn seed_workspace(store: &SqliteStore) -> Uuid {
    let workspace = ProxyWorkspace {
        name: "baseline-a".into(),
        ..ProxyWorkspace::default()
    };
    let id = workspace.id.as_uuid();
    let revision = workspace.revision.get();
    store
        .insert_workspace(&WorkspaceRecord {
            id,
            revision,
            value: to_value(workspace).unwrap(),
            updated_at: Utc::now(),
        })
        .unwrap();
    id
}

#[test]
fn baseline_observer_reads_selected_workspace_and_structural_hash_from_sqlite() {
    let store = SqliteStore::in_memory().unwrap();
    let workspace_id = seed_workspace(&store);

    let before = store
        .observe_environment_apply_generations(workspace_id)
        .expect("observe baseline");

    assert_eq!(before.selected_workspace_id, Some(workspace_id));
    assert_ne!(before.application_mutation, 0);
    store
        .execute_test_batch("UPDATE workspaces SET json = json_set(json, '$.name', 'baseline-b')")
        .unwrap();
    let after = store
        .observe_environment_apply_generations(workspace_id)
        .expect("observe changed baseline");
    assert_ne!(after.application_mutation, before.application_mutation);
}

#[test]
fn baseline_observer_hashes_exact_package_and_material_content_not_row_counts() {
    let store = SqliteStore::in_memory().unwrap();
    let workspace_id = seed_workspace(&store);
    store
        .execute_test_batch(
            "INSERT INTO external_protocol_packages(
                package_id,version,registration_json,registration_fingerprint,local_archive,
                enabled,first_connected_at,last_connected_at
             ) VALUES(
                'pkg','1.0.0','{}',zeroblob(32),X'01',1,
                '2026-08-26T00:00:00Z','2026-08-26T00:00:00Z'
             );
             INSERT INTO certificate_material(kind,protected_blob,metadata_json,updated_at)
             VALUES('listener',X'01','{}','2026-08-26T00:00:00Z');
             INSERT INTO protected_secrets(provider,secret_key,protected_blob,updated_at)
             VALUES('basic_auth','alias',X'02','2026-08-26T00:00:00Z');",
        )
        .unwrap();
    let before = store
        .observe_environment_apply_generations(workspace_id)
        .expect("observe baseline");

    assert_eq!(before.package, 0);
    assert_ne!(before.package_inventory, 0);
    assert_ne!(before.certificate_inventory, 0);
    assert_ne!(before.protected_secret_inventory, 0);
    store
        .execute_test_batch(
            "UPDATE external_protocol_packages SET local_archive=X'03', enabled=0;
             UPDATE certificate_material SET protected_blob=X'03';
             UPDATE protected_secrets SET protected_blob=X'04';",
        )
        .unwrap();
    let after = store
        .observe_environment_apply_generations(workspace_id)
        .expect("observe changed baseline");

    assert_eq!(after.package, 0);
    assert_ne!(after.package_inventory, before.package_inventory);
    assert_ne!(after.certificate_inventory, before.certificate_inventory);
    assert_ne!(
        after.protected_secret_inventory,
        before.protected_secret_inventory
    );
}
