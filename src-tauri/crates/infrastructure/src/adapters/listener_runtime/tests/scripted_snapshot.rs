use std::io::{Cursor, Write};

use intercept_proxy_domain::{
    CertificateReference, CertificateReferenceId, CertificateReferenceKind,
    DirectionProcessingOptions, DocumentAction, ProtocolPackageId, ProtocolPackageRef,
    ProtocolPackageVersion, ScriptedSocketProcessing, SocketDirection,
    SocketDocumentRuleDefinition, SocketDocumentRuleId, SocketEndpoint,
    SocketLocalResponderTopology, SocketPayloadProcessing, SocketRelaySecurity,
    SocketRelaySettings, SocketRelayTopology, SocketTopology,
};
use intercept_proxy_protocol_scripting::{ProtocolDirection, ProtocolRuntimeLimits};
use intercept_proxy_runtime::{SocketDownstreamTlsConfig, SocketTlsIdentity};
use zeroize::Zeroizing;
use zip::{ZipWriter, write::SimpleFileOptions};

use super::super::scripted_snapshot::ScriptedSocketSecuritySnapshot;
use super::*;

#[path = "scripted_snapshot/isolation.rs"]
mod isolation;
#[path = "scripted_snapshot/rule_matrix.rs"]
mod rule_matrix;

const SNAPSHOT_MANIFEST: &str = r#"
api = 1

[package]
id = "snapshot-protocol"
name = "Snapshot Protocol"
version = "1.0.0"

[document]
schema = "document.toml"

[hooks.upstream.receive]
script = "protocol.rhai"
frame = "frame"
decode = "decode"

[hooks.upstream.send]
script = "protocol.rhai"
encode = "encode"

[hooks.downstream.receive]
script = "protocol.rhai"
frame = "frame"
decode = "decode"

[hooks.downstream.send]
script = "protocol.rhai"
encode = "encode"
"#;

const SNAPSHOT_SCHEMA: &str = r#"
id = "snapshot-message"
version = 7
title = "Snapshot Message"

[[fields]]
name = "amount"
label = "Amount"
type = "int"
"#;

const SNAPSHOT_SCRIPT: &str = r"
fn frame(reader, context) { framing::complete(1) }
fn decode(origin, context) { document::create() }
fn encode(origin, document, context) { origin }
";

#[tokio::test]
async fn scripted_relay_freezes_exact_package_plans_rules_and_limits_then_starts() {
    let store = Arc::new(SqliteStore::in_memory().unwrap());
    let limits = ProtocolRuntimeLimits::new(77_777, 24, 32_768, 131_072, 125).unwrap();
    let repository = Arc::new(ProtocolPackageRepositoryAdapter::new(
        Arc::clone(&store),
        intercept_proxy_protocol_scripting::ProtocolArchiveLimits::default(),
        limits,
    ));
    install_enabled(&repository);
    let reservation = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let bind_port = reservation.local_addr().unwrap().port();
    drop(reservation);
    let mut listener = scripted_listener(SocketTopology::Relay(SocketRelayTopology {
        upstream: SocketEndpoint {
            host: "127.0.0.1".into(),
            port: 9_999,
        },
        security: SocketRelaySecurity::Transparent,
    }));
    listener.port = bind_port;
    let mut workspace = ProxyWorkspace {
        listeners: vec![listener.clone()],
        certificate_references: vec![CertificateReference {
            id: CertificateReferenceId::new(),
            label: "unrelated trust".into(),
            kind: CertificateReferenceKind::UpstreamServerTrust,
            reference: "managed-listener:unrelated".into(),
        }],
        ..ProxyWorkspace::default()
    };
    workspace.socket_rules = vec![
        rule(&listener, 20, 2),
        rule(&listener, 10, 3),
        rule(&listener, 10, 1),
    ];
    workspace.socket_rule_created_order_high_water = 3;
    workspace.validate().unwrap();
    let runtime = ListenerRuntimeAdapter::new(store).with_protocol_packages(repository);
    let plan = ListenerRuntimePlanBuilder::new(&runtime)
        .build(&workspace, &listener, Uuid::new_v4())
        .await
        .unwrap();
    let snapshot = plan
        .scripted_snapshot()
        .expect("Scripted plan contains its frozen snapshot");

    assert_eq!(snapshot.package().compiled().package(), &snapshot_package());
    assert_ne!(snapshot.package().generation(), Uuid::nil());
    assert_eq!(snapshot.runtime_limits(), limits);
    assert_eq!(snapshot.upstream().direction(), ProtocolDirection::Upstream);
    assert_eq!(
        snapshot.downstream().direction(),
        ProtocolDirection::Downstream
    );
    assert_eq!(
        snapshot
            .rules()
            .iter()
            .map(|item| (item.priority(), item.created_order()))
            .collect::<Vec<_>>(),
        vec![(10, 1), (10, 3), (20, 2)]
    );
    assert_eq!(
        snapshot
            .rule_program(SocketDirection::Upstream)
            .rules()
            .len(),
        3
    );
    assert!(
        snapshot
            .rule_program(SocketDirection::Downstream)
            .rules()
            .is_empty()
    );
    assert!(snapshot.certificate_references().is_empty());
    assert!(matches!(
        snapshot.security(),
        ScriptedSocketSecuritySnapshot::Relay(_)
    ));

    let running = runtime.start(workspace, listener.clone()).await.unwrap();
    assert_eq!(running.state, ListenerRuntimeState::Running);
    assert!(
        TcpListener::bind(("127.0.0.1", listener.port))
            .await
            .is_err()
    );
    runtime.stop(listener.id).await.unwrap();
    TcpListener::bind(("127.0.0.1", listener.port))
        .await
        .expect("stopped Scripted Relay must release its listener port");
}

#[tokio::test]
async fn local_responder_plan_has_no_upstream_security_or_probe_and_starts_locally() {
    let store = Arc::new(SqliteStore::in_memory().unwrap());
    let repository = Arc::new(ProtocolPackageRepositoryAdapter::with_default_limits(
        Arc::clone(&store),
    ));
    install_enabled(&repository);
    let reservation = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let listener_port = reservation.local_addr().unwrap().port();
    drop(reservation);
    let mut listener = scripted_listener(SocketTopology::LocalResponder(
        SocketLocalResponderTopology::default(),
    ));
    listener.port = listener_port;
    let workspace = ProxyWorkspace {
        listeners: vec![listener.clone()],
        ..ProxyWorkspace::default()
    };
    workspace.validate().unwrap();
    let runtime = ListenerRuntimeAdapter::new(store).with_protocol_packages(repository);
    let plan = ListenerRuntimePlanBuilder::new(&runtime)
        .build(&workspace, &listener, Uuid::new_v4())
        .await
        .unwrap();
    let snapshot = plan
        .scripted_snapshot()
        .expect("LocalResponder staged plan");
    assert!(matches!(
        snapshot.security(),
        ScriptedSocketSecuritySnapshot::LocalResponder {
            downstream_tls: None
        }
    ));
    let probe = ListenerRuntimePlanBuilder::new(&runtime)
        .build_probe(&workspace, &listener, Uuid::new_v4())
        .await
        .err()
        .expect("LocalResponder has no upstream probe");
    assert_eq!(probe.view_model.code, "LOCAL_RESPONDER_NOT_AVAILABLE");
    let start = runtime.start(workspace, listener.clone()).await.unwrap();
    assert_eq!(start.state, ListenerRuntimeState::Running);

    let mut client = tokio::net::TcpStream::connect(("127.0.0.1", listener_port))
        .await
        .unwrap();
    tokio::io::AsyncWriteExt::write_all(&mut client, b"x")
        .await
        .unwrap();
    tokio::io::AsyncWriteExt::shutdown(&mut client)
        .await
        .unwrap();
    let mut response = Vec::new();
    tokio::io::AsyncReadExt::read_to_end(&mut client, &mut response)
        .await
        .unwrap();
    assert_eq!(response, b"x");

    runtime.stop(listener.id).await.unwrap();
}

#[tokio::test]
async fn runtime_plan_rejects_an_enabled_encode_switch_without_manifest_entry() {
    let store = Arc::new(SqliteStore::in_memory().unwrap());
    let repository = Arc::new(ProtocolPackageRepositoryAdapter::with_default_limits(
        Arc::clone(&store),
    ));
    let manifest = SNAPSHOT_MANIFEST.replace(
        "\n[hooks.downstream.send]\nscript = \"protocol.rhai\"\nencode = \"encode\"\n",
        "\n",
    );
    repository
        .install_zip(&snapshot_zip_with_manifest(&manifest, SNAPSHOT_SCRIPT))
        .unwrap();
    repository.set_enabled(&snapshot_package(), true).unwrap();
    let listener = scripted_listener(SocketTopology::Relay(SocketRelayTopology {
        upstream: SocketEndpoint {
            host: "127.0.0.1".into(),
            port: 9_999,
        },
        security: SocketRelaySecurity::Transparent,
    }));
    let workspace = ProxyWorkspace {
        listeners: vec![listener.clone()],
        ..ProxyWorkspace::default()
    };
    let runtime = ListenerRuntimeAdapter::new(store).with_protocol_packages(repository);
    let error = ListenerRuntimePlanBuilder::new(&runtime)
        .build(&workspace, &listener, Uuid::new_v4())
        .await
        .err()
        .expect("missing configured Encode must fail at the runtime boundary");
    assert_eq!(error.view_model.code, "ENTRY_POINT_UNAVAILABLE");
}

#[tokio::test]
async fn runtime_plan_rejects_rule_schema_drift_even_when_called_below_application() {
    let store = Arc::new(SqliteStore::in_memory().unwrap());
    let repository = Arc::new(ProtocolPackageRepositoryAdapter::with_default_limits(
        Arc::clone(&store),
    ));
    install_enabled(&repository);
    let listener = scripted_listener(SocketTopology::Relay(SocketRelayTopology {
        upstream: SocketEndpoint {
            host: "127.0.0.1".into(),
            port: 9_999,
        },
        security: SocketRelaySecurity::Transparent,
    }));
    let mut invalid_rule = rule(&listener, 10, 1);
    let wire = serde_json::to_value(&invalid_rule).unwrap();
    let mut object = wire.as_object().unwrap().clone();
    object.insert("schema_version".into(), serde_json::json!(8));
    invalid_rule = serde_json::from_value(serde_json::Value::Object(object)).unwrap();
    let workspace = ProxyWorkspace {
        listeners: vec![listener.clone()],
        socket_rules: vec![invalid_rule],
        socket_rule_created_order_high_water: 1,
        ..ProxyWorkspace::default()
    };
    workspace.validate().unwrap();
    let runtime = ListenerRuntimeAdapter::new(store).with_protocol_packages(repository);
    let error = ListenerRuntimePlanBuilder::new(&runtime)
        .build(&workspace, &listener, Uuid::new_v4())
        .await
        .err()
        .expect("fresh compiled Schema must reject persisted rule drift");
    assert_eq!(
        error.view_model.code,
        "SOCKET_RULE_RUNTIME_BINDING_MISMATCH"
    );
}

#[test]
fn listener_start_snapshot_recompiles_persisted_files_and_ignores_warm_cache() {
    let store = Arc::new(SqliteStore::in_memory().unwrap());
    let repository = ProtocolPackageRepositoryAdapter::with_default_limits(Arc::clone(&store));
    install_enabled(&repository);
    let cached = repository.compiled(&snapshot_package()).unwrap();
    store.replace_protocol_package_file_for_test(
        &snapshot_package(),
        "protocol.rhai",
        b"fn frame( {",
    );

    let error = repository
        .freeze_for_listener_start(&snapshot_package())
        .unwrap_err();
    assert_eq!(error.view_model.code, "SCRIPT_SYNTAX_INVALID");
    assert_eq!(cached.package(), &snapshot_package());
}

#[test]
fn disabled_or_missing_exact_package_cannot_create_a_runtime_snapshot() {
    let repository = ProtocolPackageRepositoryAdapter::with_default_limits(Arc::new(
        SqliteStore::in_memory().unwrap(),
    ));
    repository
        .install_zip(&snapshot_zip(SNAPSHOT_SCRIPT))
        .unwrap();
    let disabled = repository
        .freeze_for_listener_start(&snapshot_package())
        .unwrap_err();
    assert_eq!(disabled.view_model.code, "PROTOCOL_PACKAGE_DISABLED");
    let missing = repository
        .freeze_for_listener_start(&ProtocolPackageRef {
            id: ProtocolPackageId::new("snapshot-protocol").unwrap(),
            version: ProtocolPackageVersion::new("2.0.0").unwrap(),
        })
        .unwrap_err();
    assert_eq!(missing.view_model.code, "PROTOCOL_PACKAGE_NOT_FOUND");
}

#[tokio::test]
async fn historical_compile_failure_is_revalidated_but_persistence_corruption_stays_closed() {
    let store = Arc::new(SqliteStore::in_memory().unwrap());
    let repository = Arc::new(ProtocolPackageRepositoryAdapter::with_default_limits(
        Arc::clone(&store),
    ));
    install_enabled(&repository);
    assert!(
        store
            .set_protocol_package_validation(&snapshot_package(), Some("SCRIPT_SYNTAX_INVALID"))
            .unwrap()
    );
    let listener = scripted_listener(SocketTopology::Relay(SocketRelayTopology {
        upstream: SocketEndpoint {
            host: "127.0.0.1".into(),
            port: 9_999,
        },
        security: SocketRelaySecurity::Transparent,
    }));
    let workspace = ProxyWorkspace {
        listeners: vec![listener.clone()],
        ..ProxyWorkspace::default()
    };
    let runtime = ListenerRuntimeAdapter::new(Arc::clone(&store))
        .with_protocol_packages(Arc::clone(&repository));
    let listener_id = listener.id;
    let running = runtime.start(workspace, listener).await.unwrap();
    assert_eq!(running.state, ListenerRuntimeState::Running);
    runtime.stop(listener_id).await.unwrap();

    assert!(
        store
            .set_protocol_package_validation(&snapshot_package(), Some("PERSISTENCE_CORRUPT"))
            .unwrap()
    );
    let error = repository
        .freeze_for_listener_start(&snapshot_package())
        .expect_err("corrupt persisted metadata must remain fail-closed");
    assert_eq!(error.view_model.code, "PERSISTENCE_CORRUPT");
}

#[test]
fn scripted_security_debug_never_contains_certificate_or_private_key_bytes() {
    let security = ScriptedSocketSecuritySnapshot::LocalResponder {
        downstream_tls: Some(SocketDownstreamTlsConfig {
            server_identity: SocketTlsIdentity {
                certificate_chain_der: vec![vec![222, 173, 190, 239]],
                private_key_pkcs8_der: Zeroizing::new(vec![17, 34, 51, 68]),
            },
            client_trust_der: vec![vec![171, 205, 239, 1]],
            client_authentication_required: true,
        }),
    };

    let debug = format!("{security:?}");
    assert!(debug.contains("server_certificate_count: 1"));
    assert!(debug.contains("client_trust_count: 1"));
    assert!(!debug.contains("222, 173, 190, 239"));
    assert!(!debug.contains("17, 34, 51, 68"));
    assert!(!debug.contains("171, 205, 239, 1"));
}

fn scripted_listener(topology: SocketTopology) -> ProxyListener {
    let processing = match topology {
        SocketTopology::Relay(_) => ScriptedSocketProcessing {
            package: snapshot_package(),
            upstream: DirectionProcessingOptions {
                decode_enabled: true,
                encode_enabled: true,
            },
            downstream: DirectionProcessingOptions {
                decode_enabled: true,
                encode_enabled: true,
            },
        },
        SocketTopology::LocalResponder(_) => ScriptedSocketProcessing {
            package: snapshot_package(),
            upstream: DirectionProcessingOptions {
                decode_enabled: true,
                encode_enabled: false,
            },
            downstream: DirectionProcessingOptions {
                decode_enabled: false,
                encode_enabled: true,
            },
        },
    };
    ProxyListener {
        name: "scripted snapshot".into(),
        bind_address: "127.0.0.1".into(),
        port: 19_090,
        data_plane: intercept_proxy_domain::ListenerDataPlane::Socket(SocketRelaySettings {
            topology,
            maximum_connections: 8,
            processing: SocketPayloadProcessing::Scripted(processing),
        }),
        ..ProxyListener::default()
    }
}

fn rule(
    listener: &ProxyListener,
    priority: i32,
    created_order: u64,
) -> SocketDocumentRuleDefinition {
    SocketDocumentRuleDefinition::new(
        SocketDocumentRuleId::new(),
        true,
        priority,
        created_order,
        listener.id,
        snapshot_package(),
        7,
        SocketDirection::Upstream,
        Vec::new(),
        vec![DocumentAction::RecordMatch],
    )
    .unwrap()
}

fn install_enabled(repository: &ProtocolPackageRepositoryAdapter) {
    repository
        .install_zip(&snapshot_zip(SNAPSHOT_SCRIPT))
        .unwrap();
    repository.set_enabled(&snapshot_package(), true).unwrap();
}

fn snapshot_package() -> ProtocolPackageRef {
    ProtocolPackageRef {
        id: ProtocolPackageId::new("snapshot-protocol").unwrap(),
        version: ProtocolPackageVersion::new("1.0.0").unwrap(),
    }
}

fn snapshot_zip(script: &str) -> Vec<u8> {
    snapshot_zip_with_manifest(SNAPSHOT_MANIFEST, script)
}

fn snapshot_zip_with_manifest(manifest: &str, script: &str) -> Vec<u8> {
    let mut writer = ZipWriter::new(Cursor::new(Vec::new()));
    for (path, contents) in [
        ("manifest.toml", manifest.as_bytes()),
        ("document.toml", SNAPSHOT_SCHEMA.as_bytes()),
        ("protocol.rhai", script.as_bytes()),
    ] {
        writer
            .start_file(path, SimpleFileOptions::default())
            .unwrap();
        writer.write_all(contents).unwrap();
    }
    writer.finish().unwrap().into_inner()
}
