use intercept_proxy_domain::{
    CertificateReference, CertificateReferenceKind, HttpListenerSettings, ProxyListenerV2,
};
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

fn v2_document() -> Value {
    let current = ProxyWorkspace::default();
    let listener = &current.listeners[0];
    let http = listener.http().unwrap();
    let listener = ProxyListenerV2 {
        id: listener.id,
        name: listener.name.clone(),
        enabled: listener.enabled,
        bind_address: listener.bind_address.clone(),
        port: listener.port,
        authentication: http.authentication.clone(),
        allowed_client_cidrs: listener.allowed_client_cidrs.clone(),
        mitm: http.mitm.clone(),
        connect_timeout_ms: listener.connect_timeout_ms,
        read_timeout_ms: listener.read_timeout_ms,
        write_timeout_ms: listener.write_timeout_ms,
        downstream_tls: Some(http.downstream_tls.clone()),
        request_body_codec: http.request_body_codec,
        response_body_codec: http.response_body_codec,
        fixed_server: http.fixed_server.clone(),
    };
    json!({
        "format_version": APPLICATION_CONFIGURATION_V2_FORMAT_VERSION,
        "selected_workspace_id": current.id,
        "workspaces": [{
            "id": current.id,
            "name": current.name,
            "revision": current.revision,
            "listeners": [listener],
            "metadata_extractors": [],
            "response_assertions": current.response_assertions,
            "rules": current.rules,
            "fault_presets": current.fault_presets,
            "certificate_references": current.certificate_references,
            "android_network_profiles": current.android_network_profiles,
        }],
        "settings": PortableSettings::from(&SettingsDraft::default()),
        "certificate_materials": [],
    })
}

#[test]
fn full_configuration_round_trips() {
    let expected = document();
    let bytes = serialize_application_configuration(&expected).expect("serialize");
    let wire: Value = serde_json::from_slice(&bytes).unwrap();
    assert_eq!(wire["format_version"], 5);
    assert_eq!(wire["workspaces"][0]["socket_rules"], json!([]));
    assert_eq!(
        wire["workspaces"][0]["socket_rule_created_order_high_water"],
        0
    );
    assert_eq!(wire["protocol_packages"], json!([]));
    assert_eq!(
        parse_application_configuration(&bytes).expect("parse"),
        expected
    );
}

#[test]
fn v2_full_configuration_migrates_to_v5_without_changing_settings() {
    let v2 = v2_document();
    let expected_settings = PortableSettings::from(&SettingsDraft::default());
    let parsed = parse_application_configuration(&serde_json::to_vec(&v2).unwrap()).unwrap();

    assert_eq!(
        parsed.format_version,
        APPLICATION_CONFIGURATION_FORMAT_VERSION
    );
    assert_eq!(parsed.settings, expected_settings);
    assert!(parsed.protocol_packages.is_empty());
    assert!(matches!(
        parsed.workspaces[0].listeners[0].data_plane,
        intercept_proxy_domain::ListenerDataPlane::Http(HttpListenerSettings { .. })
    ));
    let exported = serialize_application_configuration(&parsed).unwrap();
    let exported_value: Value = serde_json::from_slice(&exported).unwrap();
    assert_eq!(exported_value["format_version"], 5);
    assert_eq!(parse_application_configuration(&exported).unwrap(), parsed);
}

#[test]
fn v3_rejects_forged_socket_rules_and_reports_legacy_source() {
    let current = document();
    let mut legacy_wire = serde_json::to_value(current).unwrap();
    legacy_wire["format_version"] = json!(APPLICATION_CONFIGURATION_V3_FORMAT_VERSION);
    legacy_wire
        .as_object_mut()
        .unwrap()
        .remove("protocol_packages");
    for workspace in legacy_wire["workspaces"].as_array_mut().unwrap() {
        let workspace = workspace.as_object_mut().unwrap();
        workspace.insert("metadata_extractors".into(), json!([]));
        workspace.remove("socket_rules");
        workspace.remove("socket_rule_created_order_high_water");
    }
    let bytes = serde_json::to_vec(&legacy_wire).unwrap();
    let parsed = parse_application_configuration_with_source(&bytes).unwrap();
    assert_eq!(parsed.source_version, 3);
    assert!(parsed.document.protocol_packages.is_empty());

    let mut forged = legacy_wire;
    forged["workspaces"][0]["socket_rules"] = json!([]);
    let error = parse_application_configuration(&serde_json::to_vec(&forged).unwrap())
        .expect_err("v3 cannot smuggle v4 socket rules");
    assert_eq!(error.view_model.code, "IMPORT_FAILED");
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
