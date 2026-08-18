use super::*;
use crate::PortableProtocolPackageFile;

#[test]
fn v4_round_trip_preserves_socket_rule_high_water_field() {
    let workspace = ProxyWorkspace {
        socket_rule_created_order_high_water: 42,
        ..ProxyWorkspace::default()
    };
    let document = WorkspaceDocument {
        format_version: WORKSPACE_DOCUMENT_FORMAT_VERSION,
        workspace,
        certificate_materials: Vec::new(),
        protocol_packages: Vec::new(),
    };

    let bytes = serialize_workspace_document(&document).unwrap();
    let wire: Value = serde_json::from_slice(&bytes).unwrap();
    assert_eq!(
        wire["workspace"]["socket_rule_created_order_high_water"],
        42
    );
    assert_eq!(parse_workspace_document(&bytes).unwrap(), document);
}

#[test]
fn v4_document_round_trip_preserves_all_sixteen_direction_switch_combinations() {
    use intercept_proxy_domain::{
        DirectionProcessingOptions, ListenerDataPlane, ProtocolPackageId, ProtocolPackageRef,
        ProtocolPackageVersion, ProxyListener, ScriptedSocketProcessing, SocketEndpoint,
        SocketPayloadProcessing, SocketRelaySecurity, SocketRelaySettings,
    };

    let package = ProtocolPackageRef {
        id: ProtocolPackageId::new("switch-matrix").unwrap(),
        version: ProtocolPackageVersion::new("1.0.0").unwrap(),
    };
    for bits in 0_u8..16 {
        let upstream = DirectionProcessingOptions {
            decode_enabled: bits & 0b0001 != 0,
            encode_enabled: bits & 0b0010 != 0,
        };
        let downstream = DirectionProcessingOptions {
            decode_enabled: bits & 0b0100 != 0,
            encode_enabled: bits & 0b1000 != 0,
        };
        let listener = ProxyListener {
            data_plane: ListenerDataPlane::Socket(SocketRelaySettings::relay(
                SocketEndpoint {
                    host: "socket.example.test".into(),
                    port: 16_127,
                },
                SocketRelaySecurity::Transparent,
                777,
                SocketPayloadProcessing::Scripted(ScriptedSocketProcessing {
                    package: package.clone(),
                    upstream,
                    downstream,
                }),
            )),
            ..ProxyListener::default()
        };
        let document = WorkspaceDocument {
            format_version: WORKSPACE_DOCUMENT_FORMAT_VERSION,
            workspace: ProxyWorkspace {
                listeners: vec![listener],
                ..ProxyWorkspace::default()
            },
            certificate_materials: Vec::new(),
            protocol_packages: vec![PortableProtocolPackage {
                package: package.clone(),
                files: vec![PortableProtocolPackageFile {
                    path: "manifest.toml".into(),
                    contents_base64: "bWFuaWZlc3Q=".into(),
                }],
            }],
        };

        let parsed =
            parse_workspace_document(&serialize_workspace_document(&document).unwrap()).unwrap();
        let SocketPayloadProcessing::Scripted(actual) =
            &parsed.workspace.listeners[0].socket().unwrap().processing
        else {
            panic!("case {bits}: scripted processing must survive v4 round-trip")
        };
        assert_eq!(actual.upstream, upstream, "case {bits}");
        assert_eq!(actual.downstream, downstream, "case {bits}");
    }
}

#[test]
fn v3_socket_workspace_round_trip_preserves_the_tagged_variant() {
    let listener = intercept_proxy_domain::ProxyListener {
        data_plane: intercept_proxy_domain::ListenerDataPlane::Socket(
            intercept_proxy_domain::SocketRelaySettings::relay(
                intercept_proxy_domain::SocketEndpoint {
                    host: "socket.example.test".into(),
                    port: 16_127,
                },
                intercept_proxy_domain::SocketRelaySecurity::Transparent,
                777,
                intercept_proxy_domain::SocketPayloadProcessing::Scripted(
                    intercept_proxy_domain::ScriptedSocketProcessing {
                        package: intercept_proxy_domain::ProtocolPackageRef {
                            id: intercept_proxy_domain::ProtocolPackageId::new("iso8583-standard")
                                .unwrap(),
                            version: intercept_proxy_domain::ProtocolPackageVersion::new("1.2.3")
                                .unwrap(),
                        },
                        upstream: intercept_proxy_domain::DirectionProcessingOptions {
                            decode_enabled: true,
                            encode_enabled: false,
                        },
                        downstream: intercept_proxy_domain::DirectionProcessingOptions {
                            decode_enabled: false,
                            encode_enabled: true,
                        },
                    },
                ),
            ),
        ),
        ..intercept_proxy_domain::ProxyListener::default()
    };
    let workspace = ProxyWorkspace {
        listeners: vec![listener],
        ..ProxyWorkspace::default()
    };
    let expected_workspace = workspace.clone();
    let mut wire = json!({
        "format_version": WORKSPACE_DOCUMENT_V3_FORMAT_VERSION,
        "workspace": workspace,
        "certificate_materials": []
    });
    let workspace_wire = wire["workspace"].as_object_mut().unwrap();
    workspace_wire.insert("metadata_extractors".into(), json!([]));
    workspace_wire.remove("socket_rules");
    workspace_wire.remove("socket_rule_created_order_high_water");
    let bytes = serde_json::to_vec_pretty(&wire).unwrap();
    let parsed = parse_workspace_document(&bytes).unwrap();
    assert_eq!(parsed.workspace, expected_workspace);
    assert!(parsed.protocol_packages.is_empty());
    let socket = parsed.workspace.listeners[0].socket().unwrap();
    let intercept_proxy_domain::SocketPayloadProcessing::Scripted(processing) = &socket.processing
    else {
        panic!("scripted processing must survive workspace export/import")
    };
    assert!(processing.upstream.decode_enabled);
    assert!(!processing.upstream.encode_enabled);
    assert!(!processing.downstream.decode_enabled);
    assert!(processing.downstream.encode_enabled);

    wire["workspace"]["socket_rules"] = json!([]);
    let error = parse_workspace_document(&serde_json::to_vec(&wire).unwrap()).unwrap_err();
    assert_eq!(error.view_model.code, "IMPORT_FAILED");
    wire["workspace"]
        .as_object_mut()
        .unwrap()
        .remove("socket_rules");
    wire["workspace"]["socket_rule_created_order_high_water"] = json!(0);
    let error = parse_workspace_document(&serde_json::to_vec(&wire).unwrap()).unwrap_err();
    assert_eq!(error.view_model.code, "IMPORT_FAILED");
}
