use super::*;

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
    let document = WorkspaceDocument {
        format_version: WORKSPACE_DOCUMENT_FORMAT_VERSION,
        workspace,
        certificate_materials: Vec::new(),
    };

    let bytes = serialize_workspace_document(&document).unwrap();
    let mut wire: Value = serde_json::from_slice(&bytes).unwrap();
    assert!(wire["workspace"].get("socket_rules").is_none());
    assert!(
        wire["workspace"]
            .get("socket_rule_created_order_high_water")
            .is_none()
    );
    let parsed = parse_workspace_document(&bytes).unwrap();
    assert_eq!(parsed, document);
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
    assert_eq!(error.view_model.code, "SOCKET_RULE_PORTABILITY_REQUIRES_V4");
    wire["workspace"]
        .as_object_mut()
        .unwrap()
        .remove("socket_rules");
    wire["workspace"]["socket_rule_created_order_high_water"] = json!(0);
    let error = parse_workspace_document(&serde_json::to_vec(&wire).unwrap()).unwrap_err();
    assert_eq!(error.view_model.code, "SOCKET_RULE_PORTABILITY_REQUIRES_V4");
}
