use intercept_proxy_application::{
    APPLICATION_BACKUP_FORMAT_VERSION, PortableSettings, ProxyWorkspace, SettingsDraft,
    parse_application_backup_document, serialize_application_backup_document,
};
use serde_json::{Value, json};

#[test]
fn valid_application_json_round_trips_deterministically() {
    let bytes = serde_json::to_vec(&valid_document()).unwrap();
    let parsed = parse_application_backup_document(&bytes).expect("valid backup document");

    let first = serialize_application_backup_document(&parsed).unwrap();
    let second = serialize_application_backup_document(
        &parse_application_backup_document(&first).expect("serialized document parses"),
    )
    .unwrap();

    assert_eq!(first, second);
    assert_eq!(parsed.referenced_paths().len(), 3);
}

#[test]
fn application_json_has_no_archive_hash_or_signature_fields() {
    let parsed = parse_value(&valid_document()).expect("valid backup document");
    let serialized = serialize_application_backup_document(&parsed).unwrap();
    let value: Value = serde_json::from_slice(&serialized).unwrap();

    assert_eq!(
        value
            .as_object()
            .unwrap()
            .keys()
            .map(String::as_str)
            .collect::<Vec<_>>(),
        [
            "application",
            "format_version",
            "portable_materials",
            "protocol_packages"
        ]
    );
    assert!(!String::from_utf8(serialized).unwrap().contains("signature"));
    assert!(!serde_json::to_string(&value).unwrap().contains("sha256"));
}

#[test]
fn application_document_debug_redacts_workspace_names_and_passwords() {
    let mut value = valid_document();
    value["application"]["workspaces"][0]["name"] = json!("workspace-secret-marker");
    value["portable_materials"][0]["password"] = json!("password-secret-marker");
    let document = parse_value(&value).expect("valid sensitive document");

    let debug = format!("{document:?}");

    assert!(!debug.contains("workspace-secret-marker"));
    assert!(!debug.contains("password-secret-marker"));
}

#[test]
fn application_json_rejects_unsupported_version() {
    let mut value = valid_document();
    value["format_version"] = json!(2);

    let error = parse_value(&value).expect_err("unsupported version rejected");

    assert_eq!(
        error.view_model.code,
        "APPLICATION_BACKUP_VERSION_UNSUPPORTED"
    );
}

#[test]
fn application_json_rejects_wrong_field_types() {
    let mut value = valid_document();
    value["protocol_packages"][0]["enabled"] = json!("true");

    let error = parse_value(&value).expect_err("wrong type rejected");

    assert_eq!(error.view_model.code, "APPLICATION_BACKUP_DOCUMENT_INVALID");
}

#[test]
fn application_json_requires_structured_application_object() {
    let mut value = valid_document();
    value["application"] = json!("legacy-json-is-not-a-structured-object");

    let error = parse_value(&value).expect_err("non-object application rejected");

    assert_eq!(error.view_model.code, "APPLICATION_BACKUP_DOCUMENT_INVALID");
}

#[test]
fn application_json_rejects_unknown_fields_at_every_typed_level() {
    for pointer in ["", "/protocol_packages/0", "/portable_materials/0"] {
        let mut value = valid_document();
        value
            .pointer_mut(pointer)
            .expect("fixture object")
            .as_object_mut()
            .expect("fixture object")
            .insert("unexpected".to_owned(), json!(true));

        let error = parse_value(&value).expect_err("unknown field rejected");
        assert_eq!(error.view_model.code, "APPLICATION_BACKUP_DOCUMENT_INVALID");
    }
}

#[test]
fn application_json_rejects_unknown_fields_in_application_configuration() {
    let mut value = valid_document();
    value["application"]["unexpected"] = json!(true);

    let error = parse_value(&value).expect_err("nested unknown field rejected");

    assert_eq!(error.view_model.code, "APPLICATION_BACKUP_DOCUMENT_INVALID");
}

#[test]
fn application_json_rejects_removed_metadata_extractors_recursively() {
    let mut value = valid_document();
    value["application"] = json!({
        "workspaces": [{ "rules": { "metadata_extractors": [] } }]
    });

    let error = parse_value(&value).expect_err("removed field rejected");

    assert_eq!(error.view_model.code, "APPLICATION_BACKUP_DOCUMENT_INVALID");
    assert!(!error.view_model.message.contains("workspaces"));
}

#[test]
fn application_json_rejects_noncanonical_relative_references() {
    for path in [
        "/protocol-packages/sample/1.0.0/manifest.json",
        "protocol-packages/sample/1.0.0/../manifest.json",
        "protocol-packages\\sample\\1.0.0\\manifest.json",
        "C:/protocol-packages/sample/1.0.0/manifest.json",
    ] {
        let mut value = valid_document();
        value["protocol_packages"][0]["files"][0] = json!(path);

        let error = parse_value(&value).expect_err("unsafe reference rejected");
        assert_eq!(error.view_model.code, "APPLICATION_BACKUP_DOCUMENT_INVALID");
    }
}

#[test]
fn application_json_requires_exact_protocol_identity_directory() {
    let mut value = valid_document();
    value["protocol_packages"][0]["files"][0] =
        json!("protocol-packages/other/1.0.0/manifest.json");

    let error = parse_value(&value).expect_err("mismatched package reference rejected");

    assert_eq!(
        error.view_model.code,
        "APPLICATION_BACKUP_REFERENCE_INVALID"
    );
}

#[test]
fn application_json_rejects_duplicate_and_unsorted_references() {
    let mut value = valid_document();
    value["protocol_packages"][0]["files"] = json!([
        "protocol-packages/sample/1.0.0/protocol.js",
        "protocol-packages/sample/1.0.0/manifest.json"
    ]);

    let error = parse_value(&value).expect_err("unsorted references rejected");

    assert_eq!(
        error.view_model.code,
        "APPLICATION_BACKUP_REFERENCE_INVALID"
    );
}

fn parse_value(
    value: &Value,
) -> Result<
    intercept_proxy_application::ApplicationBackupDocument,
    intercept_proxy_application::AppError,
> {
    parse_application_backup_document(&serde_json::to_vec(value).unwrap())
}

fn valid_document() -> Value {
    let workspace = ProxyWorkspace::default();
    let selected_workspace_id = workspace.id;
    let settings = PortableSettings::from(&SettingsDraft::default());
    json!({
        "format_version": APPLICATION_BACKUP_FORMAT_VERSION,
        "application": {
            "selected_workspace_id": selected_workspace_id,
            "workspaces": [workspace],
            "settings": settings
        },
        "protocol_packages": [{
            "package": { "id": "sample", "version": "1.0.0" },
            "enabled": true,
            "files": [
                "protocol-packages/sample/1.0.0/manifest.json",
                "protocol-packages/sample/1.0.0/protocol.js"
            ]
        }],
        "portable_materials": [{
            "reference_id": "00000000-0000-0000-0000-000000000002",
            "label": "server identity",
            "kind": "reverse_server_identity",
            "path": "portable-materials/server-identity.pem",
            "password": null
        }]
    })
}
