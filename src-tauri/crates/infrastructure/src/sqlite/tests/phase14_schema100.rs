use chrono::Utc;

use super::*;
use crate::sqlite::external_packages::canonical_external_registration_fingerprint;

#[test]
fn final_schema100_has_only_the_unified_external_package_registry() {
    let store = SqliteStore::in_memory().expect("final Schema100 store");
    let tables = store.table_names().expect("Schema100 tables");

    assert!(tables.contains(&"external_protocol_packages".to_owned()));
    assert!(!tables.contains(&"protocol_packages".to_owned()));
    assert!(!tables.contains(&"protocol_package_files".to_owned()));
}

#[test]
fn environment_package_inventory_tracks_the_external_registry_lifecycle() {
    let store = SqliteStore::in_memory().expect("final Schema100 store");
    let workspace_id = Uuid::new_v4();
    store
        .insert_workspace(&WorkspaceRecord {
            id: workspace_id,
            revision: 1,
            value: json!({"id": workspace_id, "name": "phase14", "revision": 1}),
            updated_at: Utc::now(),
        })
        .expect("workspace");
    let manifest = serde_json::from_value(serde_json::json!({
        "api": 1,
        "kind": "http",
        "package": {
            "id": "phase14-package",
            "version": "1.0.0",
            "name": "Phase 14",
            "description": ""
        },
        "document": {"upstream": {}, "downstream": {}}
    }))
    .expect("manifest");
    store
        .accept_external_package_registration(
            &manifest,
            canonical_external_registration_fingerprint(&manifest).expect("fingerprint"),
            Utc::now(),
        )
        .expect("register package");

    let inventory = || {
        store
            .observe_environment_apply_generations(workspace_id)
            .expect("observe package inventory")
            .package_inventory
    };
    let mut before = inventory();
    assert_ne!(before, 0);

    for mutation in [
        "UPDATE external_protocol_packages SET registration_json = '{\"phase14\":true}'",
        "UPDATE external_protocol_packages SET registration_fingerprint = zeroblob(32)",
        "UPDATE external_protocol_packages SET local_archive = X'504B0304'",
        "UPDATE external_protocol_packages SET first_connected_at = '2026-08-31T00:00:01+00:00'",
        "UPDATE external_protocol_packages SET last_connected_at = '2026-08-31T00:00:02+00:00'",
        "UPDATE external_protocol_packages SET last_remote_address = '127.0.0.1:45100'",
        "UPDATE external_protocol_packages SET recent_error_code = 'EXTERNAL_PACKAGE_DISCONNECTED', recent_error_message = 'connection closed', recent_error_occurred_at = '2026-08-31T00:00:03+00:00'",
    ] {
        store
            .execute_test_batch(mutation)
            .expect("mutate lifecycle field");
        let after = inventory();
        assert_ne!(after, before, "inventory must include mutation: {mutation}");
        before = after;
    }

    store
        .set_external_package_enabled(&manifest.package().identity(), false)
        .expect("disable package");
    assert_ne!(inventory(), before, "inventory must include enabled");
}
