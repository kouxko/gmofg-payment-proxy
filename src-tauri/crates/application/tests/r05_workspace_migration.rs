use intercept_proxy_application::{
    APPLICATION_CONFIGURATION_FORMAT_VERSION, MigrationSourceKind, PortableSettings, SettingsDraft,
    WORKSPACE_PERSISTENCE_VERSION, migrate_workspace_value,
    parse_application_configuration_with_source,
};
use intercept_proxy_domain::ProxyWorkspace;
use serde_json::{Map, Value, json};
use uuid::Uuid;

const LEGACY_VERSIONS: [u16; 3] = [2, 3, 4];

#[test]
fn shared_workspace_migration_reports_exact_removed_extractor_counts() {
    for version in LEGACY_VERSIONS {
        for count in [0, 1, 4, 64] {
            let source = legacy_workspace_value(version, count);
            let (workspace, report) =
                migrate_workspace_value(source, MigrationSourceKind::WorkspaceDocument, version)
                    .unwrap_or_else(|error| panic!("v{version} count {count}: {error:?}"));

            assert_eq!(report.removed_metadata_extractors, count);
            assert_eq!(report.source_kind, MigrationSourceKind::WorkspaceDocument);
            assert_eq!(report.source_version, version);
            assert_eq!(workspace.name, "Legacy workspace");
            assert!(
                serde_json::to_value(workspace)
                    .expect("serialize migrated workspace")
                    .get("metadata_extractors")
                    .is_none(),
                "v{version} output must use the v5 shape"
            );
        }
    }
}

#[test]
fn shared_workspace_migration_accepts_all_four_legacy_extractor_variants() {
    for version in LEGACY_VERSIONS {
        let (_, report) = migrate_workspace_value(
            legacy_workspace_value(version, 4),
            MigrationSourceKind::WorkspaceDocument,
            version,
        )
        .unwrap_or_else(|error| panic!("v{version}: {error:?}"));

        assert_eq!(report.removed_metadata_extractors, 4);
    }
}

#[test]
fn configuration_migration_aggregates_removed_extractors_across_workspaces() {
    for version in LEGACY_VERSIONS {
        let first = legacy_workspace_value(version, 1);
        let second = legacy_workspace_value(version, 4);
        let selected_workspace_id = first["id"].clone();
        let mut document = json!({
            "format_version": version,
            "selected_workspace_id": selected_workspace_id,
            "workspaces": [first, second],
            "settings": PortableSettings::from(&SettingsDraft::default()),
            "certificate_materials": []
        });
        if version == 4 {
            document["protocol_packages"] = json!([]);
        }

        let parsed = parse_application_configuration_with_source(
            &serde_json::to_vec(&document).expect("serialize legacy configuration"),
        )
        .unwrap_or_else(|error| panic!("configuration v{version}: {error:?}"));

        assert_eq!(parsed.source_version, version);
        assert_eq!(
            parsed.migration_report.source_kind,
            MigrationSourceKind::ApplicationConfigurationDocument
        );
        assert_eq!(parsed.migration_report.source_version, version);
        assert_eq!(parsed.migration_report.removed_metadata_extractors, 5);
        assert_eq!(
            parsed.document.format_version,
            APPLICATION_CONFIGURATION_FORMAT_VERSION
        );
    }
}

#[test]
fn current_v5_workspace_rejects_legacy_metadata_extractors_as_unknown() {
    let mut value = serde_json::to_value(ProxyWorkspace::default()).expect("workspace value");
    value["metadata_extractors"] = Value::Array(legacy_extractors(1));

    let error = migrate_workspace_value(
        value,
        MigrationSourceKind::WorkspaceDocument,
        WORKSPACE_PERSISTENCE_VERSION,
    )
    .expect_err("v5 must reject removed fields instead of silently ignoring them");

    assert_eq!(error.view_model.code, "IMPORT_FAILED");
    assert!(error.view_model.message.contains("metadata_extractors"));
}

fn legacy_workspace_value(version: u16, extractor_count: usize) -> Value {
    let mut workspace = ProxyWorkspace {
        id: intercept_proxy_domain::WorkspaceId::new(),
        name: "Legacy workspace".into(),
        ..ProxyWorkspace::default()
    };
    workspace.listeners[0].name = format!("v{version} listener");
    let mut value = serde_json::to_value(workspace).expect("workspace fixture");
    let object = value.as_object_mut().expect("workspace object");
    object.insert(
        "metadata_extractors".into(),
        Value::Array(legacy_extractors(extractor_count)),
    );

    if version < 4 {
        object.remove("socket_rules");
        object.remove("socket_rule_created_order_high_water");
    }
    if version == 2 {
        let listeners = object["listeners"].as_array_mut().expect("listeners array");
        for listener in listeners {
            flatten_v2_http_listener(listener);
        }
    }
    value
}

fn flatten_v2_http_listener(listener: &mut Value) {
    let object = listener.as_object_mut().expect("listener object");
    let data_plane = object
        .remove("data_plane")
        .expect("current listener data plane");
    assert_eq!(data_plane["kind"], "http");
    let settings = data_plane["settings"]
        .as_object()
        .expect("HTTP settings")
        .clone();
    object.extend(settings);
}

fn legacy_extractors(count: usize) -> Vec<Value> {
    (0..count)
        .map(|index| {
            json!({
                "id": Uuid::new_v4(),
                "name": format!("extractor-{index}"),
                "listener_ids": [],
                "source": extractor_source(index)
            })
        })
        .collect()
}

fn extractor_source(index: usize) -> Value {
    let source: Map<String, Value> = match index % 4 {
        0 => Map::from_iter([
            ("kind".into(), json!("header")),
            ("name".into(), json!("x-request-id")),
        ]),
        1 => Map::from_iter([
            ("kind".into(), json!("json_path")),
            ("path".into(), json!("$.transaction.id")),
        ]),
        2 => Map::from_iter([("kind".into(), json!("body_text"))]),
        _ => Map::from_iter([
            ("kind".into(), json!("fixed_value")),
            ("value".into(), json!("fixed")),
        ]),
    };
    Value::Object(source)
}
