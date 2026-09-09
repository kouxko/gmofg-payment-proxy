use intercept_proxy_domain::{ProtocolDirection, ProxyWorkspace, Revision};
use intercept_proxy_package_runtime::{WasmPackageRuntime, read_package_component};

use super::*;

#[tokio::test]
async fn finalize_host_reopens_schema100_and_keeps_the_existing_http_package_and_workspace() {
    let repository = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../..");
    let archive = std::fs::read(
        repository.join("examples/protocol-packages/json_pretty/dist/json-pretty-1.0.0.wasm"),
    )
    .expect("existing pre-Finalize HTTP Component");
    let component = read_package_component(&archive).expect("existing package manifest");
    let identity = component.manifest().package().identity();
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("schema100.sqlite");
    let now = Utc::now();
    let workspace_value = ProxyWorkspace {
        name: "preserved workspace".into(),
        revision: Revision::new(7),
        ..ProxyWorkspace::default()
    };
    workspace_value.validate().unwrap();
    let workspace = crate::adapters::WorkspaceRepositoryAdapter::record(&workspace_value).unwrap();

    let before = {
        let store = SqliteStore::open(&path).unwrap();
        store.insert_workspace(&workspace).unwrap();
        store
            .install_local_external_package(component.manifest(), &archive, now)
            .unwrap();
        store
            .set_external_package_enabled(&identity, false)
            .unwrap();
        store.get_external_package(&identity).unwrap().unwrap()
    };

    let store = SqliteStore::open(&path).expect("reopen the compatible database");
    assert_eq!(
        store.load_workspace(workspace.id).unwrap(),
        Some(workspace.clone())
    );
    assert_eq!(
        store.load_workspaces().unwrap().selected_id,
        Some(workspace.id)
    );
    let after = store.get_external_package(&identity).unwrap().unwrap();
    assert_eq!(
        after, before,
        "Finalize must not migrate or rewrite stored package data"
    );
    let version: i64 = store
        .connection
        .lock()
        .query_row(
            "SELECT version FROM application_schema WHERE singleton_id = 1",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(version, 100);

    let persisted = read_package_component(after.local_archive.as_ref().unwrap()).unwrap();
    let mut runtime = WasmPackageRuntime::load(&persisted)
        .await
        .expect("load the unchanged old Component");
    for direction in [ProtocolDirection::Upstream, ProtocolDirection::Downstream] {
        assert!(!runtime.has_http_finalize(direction));
        assert!(
            runtime
                .finalize_http(direction, "not needed by an old package")
                .await
                .unwrap()
                .is_none()
        );
    }
    let document = runtime
        .decode_http(ProtocolDirection::Upstream, r#"{"old":true}"#)
        .await
        .unwrap();
    assert_eq!(
        runtime
            .encode_http(ProtocolDirection::Upstream, r#"{"old":true}"#, &document)
            .await
            .unwrap(),
        r#"{"old":true}"#
    );
}
