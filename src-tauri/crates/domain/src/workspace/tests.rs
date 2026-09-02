use std::collections::BTreeSet;

use super::*;
use crate::{
    AndroidProxyRoute, AndroidTargetApplication, Condition, HttpAction, HttpRuleContent,
    MatchField, MatchOperator, RuleContent, RuleDefinition, RuleDefinitionDraft, RuleStage,
    UnifiedAction, WeakNetworkProfile,
};

mod listener_topology;
mod listener_v3;

fn http(listener: &ProxyListener) -> &HttpListenerSettings {
    listener.http().expect("HTTP listener")
}

fn http_mut(listener: &mut ProxyListener) -> &mut HttpListenerSettings {
    match &mut listener.data_plane {
        ListenerDataPlane::Http(settings) => settings,
        ListenerDataPlane::Socket(_) => panic!("expected HTTP listener"),
    }
}

#[test]
fn default_workspace_is_empty_safe_and_serializable() {
    let workspace = ProxyWorkspace::default();
    assert_eq!(workspace.listeners.len(), 1);
    let listener = &workspace.listeners[0];
    assert!(!listener.enabled);
    assert_eq!(listener.bind_address, "127.0.0.1");
    assert_eq!(listener.port, 8080);
    assert!(http(listener).fixed_server.is_none());
    assert_eq!(http(listener).request_body_codec, BodyCodecKind::Auto);
    assert_eq!(http(listener).response_body_codec, BodyCodecKind::Auto);
    assert!(workspace.rule_definitions.is_empty());
    workspace.validate().expect("safe draft must validate");
    let json = serde_json::to_string(&workspace).unwrap();
    for forbidden in ["private_key", "password", "pkcs12"] {
        assert!(!json.to_ascii_lowercase().contains(forbidden));
    }
}

#[test]
fn standard_http_rules_require_one_existing_http_listener() {
    let standard_rule = |listener_id| {
        RuleDefinition::create(
            RuleDefinitionDraft {
                name: "HTTP rule".into(),
                enabled: true,
                priority: 10,
                listener_id,
                stage: RuleStage::ProxyToUpstream,
                content: RuleContent::Http(HttpRuleContent {
                    description: String::new(),
                    condition: Condition::Http {
                        field: MatchField::Method,
                        operator: MatchOperator::Equals("GET".into()),
                    },
                    action: UnifiedAction::Http(HttpAction::Delay { milliseconds: 10 }),
                }),
            },
            1,
        )
        .unwrap()
    };

    let mut workspace = ProxyWorkspace::default();
    workspace.rule_definitions = vec![standard_rule(workspace.listeners[0].id)];
    workspace.rule_created_order_high_water = 1;
    workspace.validate().expect("HTTP listener binding");

    let socket_id = ListenerId::new();
    workspace.listeners.push(ProxyListener {
        id: socket_id,
        name: "Socket".into(),
        port: 9000,
        data_plane: ListenerDataPlane::Socket(SocketRelaySettings::default()),
        ..ProxyListener::default()
    });
    workspace.rule_definitions = vec![standard_rule(socket_id)];
    assert_eq!(
        workspace.validate().unwrap_err().field_errors["rule_definitions.0.listener_id"],
        vec!["HTTP 规则只能绑定 HTTP Listener"]
    );
}

#[test]
fn listener_body_codec_values_are_deserializable() {
    for (serialized, expected) in [
        ("raw", BodyCodecKind::Raw),
        ("utf8", BodyCodecKind::Utf8),
        ("shift_jis", BodyCodecKind::ShiftJis),
    ] {
        let codec: BodyCodecKind = serde_json::from_value(serialized.into()).unwrap();
        assert_eq!(codec, expected);
    }
}

#[test]
fn non_loopback_forward_listener_requires_authentication() {
    let mut workspace = ProxyWorkspace::default();
    let listener = &mut workspace.listeners[0];
    listener.enabled = true;
    listener.bind_address = "0.0.0.0".into();
    let error = workspace.validate().unwrap_err();
    assert!(
        error
            .field_errors
            .contains_key("listeners.0.data_plane.settings.authentication")
    );
}

#[test]
fn non_loopback_fixed_server_allows_all_clients() {
    let mut workspace = ProxyWorkspace::default();
    let listener = &mut workspace.listeners[0];
    listener.enabled = true;
    listener.bind_address = "0.0.0.0".into();
    http_mut(listener).fixed_server = Some(FixedServerSettings {
        upstream_url: "https://server.example.test:443".into(),
        upstream_tls: UpstreamTlsSettings::default(),
    });

    workspace.validate().unwrap();
}

#[test]
fn fixed_server_accepts_generic_http_and_https_origins() {
    assert!(is_valid_upstream_origin("http://127.0.0.1:8081"));
    assert!(is_valid_upstream_origin("https://example.test:443/"));
    for invalid in [
        "ftp://example.test",
        "https://user@example.test",
        "https://example.test/path",
        "https://example.test?query=1",
    ] {
        assert!(!is_valid_upstream_origin(invalid), "{invalid}");
    }
}

#[test]
fn workspace_accepts_multiple_fixed_server_listener_mappings() {
    let fixed = |name: &str, port: u16, upstream_url: &str| ProxyListener {
        id: ListenerId::new(),
        name: name.into(),
        enabled: true,
        bind_address: "127.0.0.1".into(),
        port,
        data_plane: ListenerDataPlane::Http(HttpListenerSettings {
            request_body_codec: BodyCodecKind::Raw,
            response_body_codec: BodyCodecKind::Raw,
            fixed_server: Some(FixedServerSettings {
                upstream_url: upstream_url.into(),
                upstream_tls: UpstreamTlsSettings::default(),
            }),
            ..HttpListenerSettings::default()
        }),
        ..ProxyListener::default()
    };
    let workspace = ProxyWorkspace {
        listeners: vec![
            fixed("Orders API", 18_081, "https://orders.example.test:18081"),
            fixed("Webhook API", 18_082, "https://webhook.example.test:18082"),
        ],
        ..ProxyWorkspace::default()
    };

    workspace
        .validate()
        .expect("distinct local endpoints may map to distinct upstream origins");
}

#[test]
fn mitm_is_fail_closed_without_allowlist_but_can_use_installation_root() {
    let mut workspace = ProxyWorkspace::default();
    let listener = &mut workspace.listeners[0];
    http_mut(listener).mitm.enabled = true;
    let error = workspace.validate().unwrap_err();
    assert!(
        error
            .field_errors
            .contains_key("listeners.0.data_plane.settings.mitm.authority_allowlist")
    );
    assert!(
        !error
            .field_errors
            .contains_key("listeners.0.data_plane.settings.mitm.root_ca")
    );
}

#[test]
fn downstream_tls_can_use_installation_root_for_dynamic_sni() {
    let mut workspace = ProxyWorkspace::default();
    http_mut(&mut workspace.listeners[0]).downstream_tls.enabled = true;
    http_mut(&mut workspace.listeners[0])
        .downstream_tls
        .server_identity = None;
    http_mut(&mut workspace.listeners[0])
        .downstream_tls
        .dynamic_sni_allowlist = vec!["api.example.test".into(), "*.service.test".into()];

    workspace
        .validate()
        .expect("installation root supports validated dynamic SNI patterns");
}

#[test]
fn downstream_tls_rejects_invalid_dynamic_sni_pattern() {
    let mut workspace = ProxyWorkspace::default();
    http_mut(&mut workspace.listeners[0]).downstream_tls.enabled = true;
    http_mut(&mut workspace.listeners[0])
        .downstream_tls
        .dynamic_sni_allowlist = vec!["https://api.example.test/path".into()];

    let error = workspace.validate().expect_err("invalid SNI pattern");
    assert!(
        error
            .field_errors
            .contains_key("listeners.0.data_plane.settings.downstream_tls.dynamic_sni_allowlist.0")
    );
}

#[test]
fn downstream_tls_rejects_ip_literal_dynamic_sni() {
    let mut workspace = ProxyWorkspace::default();
    http_mut(&mut workspace.listeners[0]).downstream_tls.enabled = true;
    http_mut(&mut workspace.listeners[0])
        .downstream_tls
        .dynamic_sni_allowlist = vec!["10.0.0.1".into()];

    let error = workspace.validate().expect_err("IP is not a TLS SNI name");

    assert!(
        error
            .field_errors
            .contains_key("listeners.0.data_plane.settings.downstream_tls.dynamic_sni_allowlist.0")
    );
}

#[test]
fn fixed_http_server_rejects_tls_certificate_configuration() {
    let mut workspace = ProxyWorkspace::default();
    let trust_id = CertificateReferenceId::new();
    workspace.certificate_references.push(CertificateReference {
        id: trust_id,
        label: "测试 Server CA".into(),
        kind: CertificateReferenceKind::UpstreamServerTrust,
        reference: "managed:test-ca".into(),
    });
    http_mut(&mut workspace.listeners[0]).fixed_server = Some(FixedServerSettings {
        upstream_url: "http://server.example.test:8080".into(),
        upstream_tls: UpstreamTlsSettings {
            server_trust: Some(trust_id),
            ..UpstreamTlsSettings::default()
        },
    });

    let error = workspace.validate().unwrap_err();
    assert!(
        error
            .field_errors
            .contains_key("listeners.0.data_plane.settings.fixed_server.upstream_tls")
    );
}

#[test]
fn listener_tls_rejects_certificate_references_used_in_the_wrong_role() {
    let mut workspace = ProxyWorkspace::default();
    let trust_id = CertificateReferenceId::new();
    workspace.certificate_references.push(CertificateReference {
        id: trust_id,
        label: "客户端证书 CA".into(),
        kind: CertificateReferenceKind::DownstreamClientTrust,
        reference: "managed:listener-tls:test-client-ca".into(),
    });
    http_mut(&mut workspace.listeners[0]).fixed_server = Some(FixedServerSettings {
        upstream_url: "https://server.example.test:443".into(),
        upstream_tls: UpstreamTlsSettings {
            server_trust: Some(trust_id),
            ..UpstreamTlsSettings::default()
        },
    });

    let error = workspace.validate().unwrap_err();

    assert!(
        error
            .field_errors
            .contains_key("listeners.0.data_plane.settings.fixed_server.upstream_tls.server_trust")
    );
}

#[test]
fn fixed_server_stores_body_encoding_on_the_listener() {
    let mut workspace = ProxyWorkspace::default();
    http_mut(&mut workspace.listeners[0]).request_body_codec = BodyCodecKind::Utf8;
    http_mut(&mut workspace.listeners[0]).response_body_codec = BodyCodecKind::ShiftJis;
    http_mut(&mut workspace.listeners[0]).fixed_server = Some(FixedServerSettings {
        upstream_url: "https://example.test".into(),
        upstream_tls: UpstreamTlsSettings::default(),
    });

    workspace
        .validate()
        .expect("listener body codecs are self-contained");
    assert_eq!(
        http(&workspace.listeners[0]).request_body_codec,
        BodyCodecKind::Utf8
    );
    assert_eq!(
        http(&workspace.listeners[0]).response_body_codec,
        BodyCodecKind::ShiftJis
    );
}

#[test]
fn android_proxy_routes_must_reference_a_listener_in_the_same_workspace() {
    let mut workspace = ProxyWorkspace::default();
    let profile = AndroidNetworkProfile {
        id: "android-route".into(),
        name: "Android route".into(),
        target_applications: vec![AndroidTargetApplication {
            package_name: "com.example.client".into(),
            uid: 10_001,
            display_name: None,
        }],
        destination_targets: Vec::new(),
        proxy_routes: vec![AndroidProxyRoute {
            destination: "api.example.test".into(),
            ports: vec![443],
            listener_id: workspace.listeners[0].id,
        }],
        confirmed_shared_uids: BTreeSet::new(),
        auto_resume_after_reboot: false,
        stop_vpn_on_control_loss: true,
        weak_network: WeakNetworkProfile::default(),
    };
    workspace.android_network_profiles.push(profile);
    workspace.validate().expect("same-workspace listener route");

    workspace.android_network_profiles[0].proxy_routes[0].listener_id = ListenerId::new();
    let error = workspace.validate().expect_err("dangling listener route");
    assert!(
        error
            .field_errors
            .contains_key("android_network_profiles.0.proxy_routes.0.listener_id")
    );
}

#[test]
fn apply_is_atomic_and_preserves_workspace_identity() {
    let mut stored = ProxyWorkspace::default();
    let original_id = stored.id;
    let mut candidate = stored.clone();
    candidate.id = WorkspaceId::new();
    candidate.name = "Renamed".into();
    let revision = stored.apply(Revision::INITIAL, candidate).unwrap();
    assert_eq!(revision, Revision::new(2));
    assert_eq!(stored.id, original_id);
    assert_eq!(stored.name, "Renamed");
}
