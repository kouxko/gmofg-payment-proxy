use intercept_proxy_domain::{CertificateReference, CertificateReferenceKind};
use serde_json::json;

use super::*;

fn document() -> ApplicationConfigurationDocument {
    let workspace = ProxyWorkspace::default();
    ApplicationConfigurationDocument {
        format_version: APPLICATION_CONFIGURATION_FORMAT_VERSION,
        selected_workspace_id: workspace.id,
        workspaces: vec![workspace],
        settings: PortableSettings::from(&SettingsDraft::default()),
        certificate_materials: Vec::new(),
        protocol_packages: Vec::new(),
    }
}

#[test]
fn full_configuration_round_trips() {
    let expected = document();
    let bytes = serialize_application_configuration(&expected).expect("serialize");
    let wire: Value = serde_json::from_slice(&bytes).unwrap();
    assert_eq!(wire["format_version"], 5);
    assert_eq!(wire["workspaces"][0]["protocol_rules"], json!([]));
    assert_eq!(
        wire["workspaces"][0]["protocol_rule_created_order_high_water"],
        0
    );
    assert_eq!(wire["protocol_packages"], json!([]));
    assert_eq!(
        parse_application_configuration(&bytes).expect("parse"),
        expected
    );
}

#[test]
fn non_current_configuration_version_is_rejected() {
    let mut value = serde_json::to_value(document()).unwrap();
    value["format_version"] = json!(4);
    let error = parse_application_configuration(&serde_json::to_vec(&value).unwrap())
        .expect_err("old configuration must be rejected");
    assert_eq!(
        error.view_model.code,
        "APPLICATION_CONFIGURATION_VERSION_UNSUPPORTED"
    );
}

#[test]
fn sensitive_and_runtime_fields_are_rejected_before_deserialization() {
    for forbidden in [
        "private_key_pem",
        "privateKey",
        "password",
        "basic_auth_password",
        "basicAuthPassword",
        "pkcs12_password",
        "protected_blob",
        "selected_serial",
        "resolved_routes",
        "payload",
    ] {
        let mut value = serde_json::to_value(document()).expect("value");
        value
            .as_object_mut()
            .expect("object")
            .insert(forbidden.into(), json!("forbidden"));
        let error =
            parse_application_configuration(&serde_json::to_vec(&value).expect("document bytes"))
                .expect_err("forbidden field must fail");
        assert_eq!(error.view_model.code, "IMPORT_CONTAINS_SENSITIVE_DATA");
    }
}

#[test]
fn missing_selected_workspace_is_rejected() {
    let mut value = document();
    value.selected_workspace_id = WorkspaceId::new();
    assert!(value.validate().is_err());
}

#[test]
fn unmanaged_certificate_reference_is_rejected() {
    let mut value = document();
    value.workspaces[0]
        .certificate_references
        .push(CertificateReference {
            id: intercept_proxy_domain::CertificateReferenceId::new(),
            label: "外部文件".into(),
            kind: CertificateReferenceKind::UpstreamServerTrust,
            reference: "file:/tmp/server-ca.pem".into(),
        });

    let error = value.validate().expect_err("unmanaged reference must fail");
    assert_eq!(
        error.view_model.code,
        "LISTENER_CERTIFICATE_REFERENCE_UNTRUSTED"
    );
}

#[test]
fn installation_root_reference_is_allowed_without_exporting_local_material() {
    let mut value = document();
    value.workspaces[0]
        .certificate_references
        .push(CertificateReference {
            id: intercept_proxy_domain::CertificateReferenceId::new(),
            label: "本机 MITM Root CA".into(),
            kind: CertificateReferenceKind::MitmRootCa,
            reference: INSTALLATION_ROOT_CERTIFICATE_REFERENCE.into(),
        });

    let bytes = serialize_application_configuration(&value).expect("serialize");
    assert_eq!(parse_application_configuration(&bytes).unwrap(), value);
}

#[test]
fn outer_size_limit_counts_embedded_registry_and_rule_bytes_before_json_parsing() {
    let mut document = br#"{"format_version":4,"protocol_packages":[{"files":["#.to_vec();
    document.resize(MAX_APPLICATION_CONFIGURATION_BYTES + 1, b'x');

    let error = parse_application_configuration(&document).expect_err("oversized config rejected");
    assert_eq!(error.view_model.code, "IMPORT_FAILED");
    assert!(error.view_model.message.contains("128 MiB"));
}
