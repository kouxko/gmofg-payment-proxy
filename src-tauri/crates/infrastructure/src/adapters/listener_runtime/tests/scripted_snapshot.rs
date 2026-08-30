use std::io::{Cursor, Write};

use intercept_proxy_domain::{
    CertificateReference, CertificateReferenceId, CertificateReferenceKind, ProtocolDirection,
    ProtocolDocumentOperation, ProtocolDocumentRuleDefinition, ProtocolDocumentRuleId,
    ProtocolPackageId, ProtocolPackageRef, ProtocolPackageVersion, ProtocolRuleStage,
    ScriptedSocketProcessing, SocketEndpoint, SocketLocalResponderTopology,
    SocketPayloadProcessing, SocketRelaySecurity, SocketRelaySettings, SocketRelayTopology,
    SocketTopology,
};
use intercept_proxy_protocol_scripting::{
    ProtocolDirection as ScriptProtocolDirection, ProtocolRuntimeLimits,
};
use intercept_proxy_runtime::{SocketDownstreamTlsConfig, SocketTlsIdentity};
use zeroize::Zeroizing;
use zip::{ZipWriter, write::SimpleFileOptions};

use super::super::scripted_snapshot::ScriptedSocketSecuritySnapshot;
use super::*;

#[path = "scripted_snapshot/fixtures.rs"]
mod fixtures;
#[path = "scripted_snapshot/isolation.rs"]
mod isolation;
#[path = "scripted_snapshot/rule_matrix.rs"]
mod rule_matrix;

use fixtures::{snapshot_zip, snapshot_zip_with_manifest};

const SNAPSHOT_MANIFEST: &str = r#"
api = 1

[package]
id = "snapshot-protocol"
name = "Snapshot Protocol"
version = "1.0.0"

[document.upstream]
schema = "document.toml"
display = "display"

[document.downstream]
schema = "document.toml"
display = "display"

[hooks.upstream]
frame = "frame"
decode = "decode"
encode = "encode"

[hooks.downstream]
frame = "frame"
decode = "decode"
encode = "encode"
"#;

const HTTP_SNAPSHOT_MANIFEST: &str = r#"
api = 1

[package]
id = "snapshot-protocol"
name = "Snapshot Protocol"
version = "1.0.0"

[document.upstream]
schema = "document.toml"
display = "display"

[document.downstream]
schema = "document.toml"
display = "display"

[hooks.upstream]
decode = "decode"
encode = "encode"

[hooks.downstream]
decode = "decode"
encode = "encode"
"#;

const SNAPSHOT_SCHEMA: &str = r#"
type = "object"
title = "Snapshot Message"

[properties.amount]
type = "number"
title = "Amount"
"#;

const SNAPSHOT_SCRIPT: &str = r"
fn frame(reader, context) { framing::complete(1) }
fn decode(origin, context) { document::create() }
fn encode(origin, document, context) { origin }
";

const SNAPSHOT_DISPLAY: &str = r#"fn display(document, context) { "<p>snapshot</p>" }"#;

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
    workspace
        .replace_document_runtime_rules(vec![
            rule(&listener, 20, 2),
            rule(&listener, 10, 3),
            rule(&listener, 10, 1),
        ])
        .unwrap();
    workspace.rule_created_order_high_water = 3;
    workspace.validate().unwrap();
    let runtime = test_listener_runtime_with_packages(store, repository);
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
    assert_eq!(
        snapshot.upstream().direction(),
        ScriptProtocolDirection::Upstream
    );
    assert_eq!(
        snapshot.downstream().direction(),
        ScriptProtocolDirection::Downstream
    );
    assert_eq!(
        snapshot
            .rules()
            .iter()
            .map(|item| (item.priority(), item.created_order()))
            .collect::<Vec<_>>(),
        vec![(10, 3), (10, 1), (20, 2)]
    );
    assert_eq!(
        snapshot
            .rule_program(ProtocolRuleStage::ProxyToUpstream)
            .rules()
            .len(),
        3
    );
    assert!(
        snapshot
            .rule_program(ProtocolRuleStage::ProxyToApp)
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
    let runtime = test_listener_runtime_with_packages(store, repository);
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
    assert_eq!(probe.view_model.code, "LISTENER_UPSTREAM_NOT_APPLICABLE");
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
async fn runtime_plan_rejects_rule_value_schema_mismatch_even_when_called_below_application() {
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
    let invalid_rule = ProtocolDocumentRuleDefinition::new(
        ProtocolDocumentRuleId::new(),
        true,
        10,
        1,
        listener.id,
        snapshot_package(),
        ProtocolDirection::Upstream,
        vec![intercept_proxy_domain::ProtocolDocumentPredicate::Equals {
            field: intercept_proxy_domain::JsonPointer::property("amount"),
            value: intercept_proxy_domain::DocumentValue::integer(1).unwrap(),
        }],
        vec![ProtocolDocumentOperation::SetField {
            field: intercept_proxy_domain::JsonPointer::property("amount"),
            value: intercept_proxy_domain::DocumentValue::String("not-a-number".into()),
        }],
    )
    .unwrap();
    let mut workspace = ProxyWorkspace {
        listeners: vec![listener.clone()],
        rule_created_order_high_water: 1,
        ..ProxyWorkspace::default()
    };
    workspace
        .replace_document_runtime_rules(vec![invalid_rule])
        .unwrap();
    workspace.validate().unwrap();
    let runtime = test_listener_runtime_with_packages(store, repository);
    let error = ListenerRuntimePlanBuilder::new(&runtime)
        .build(&workspace, &listener, Uuid::new_v4())
        .await
        .err()
        .expect("fresh compiled Schema must reject persisted rule value mismatch");
    assert_eq!(error.view_model.code, "RULE_INVALID");
}

#[tokio::test]
async fn socket_runtime_snapshot_rejects_an_http_protocol_package() {
    let store = Arc::new(SqliteStore::in_memory().unwrap());
    let repository = Arc::new(ProtocolPackageRepositoryAdapter::with_default_limits(
        Arc::clone(&store),
    ));
    repository
        .install_zip(&snapshot_zip_with_manifest(
            HTTP_SNAPSHOT_MANIFEST,
            SNAPSHOT_SCRIPT,
        ))
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
    let runtime = test_listener_runtime_with_packages(store, repository);

    let error = ListenerRuntimePlanBuilder::new(&runtime)
        .build(&workspace, &listener, Uuid::new_v4())
        .await
        .err()
        .expect("Socket runtime must reject an HTTP protocol package");
    assert_eq!(error.view_model.code, "PROTOCOL_PACKAGE_KIND_MISMATCH");
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
    let runtime = test_listener_runtime_with_packages(Arc::clone(&store), Arc::clone(&repository));
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
        SocketTopology::Relay(_) | SocketTopology::LocalResponder(_) => ScriptedSocketProcessing {
            package: snapshot_package(),
        },
    };
    ProxyListener {
        name: "scripted snapshot".into(),
        bind_address: "127.0.0.1".into(),
        port: 19_090,
        data_plane: intercept_proxy_domain::ListenerDataPlane::Socket(SocketRelaySettings {
            topology,
            maximum_connections: 8,
            runtime_limits: intercept_proxy_domain::SocketRuntimeLimits::default(),
            processing: SocketPayloadProcessing::Scripted(processing),
        }),
        ..ProxyListener::default()
    }
}

fn rule(
    listener: &ProxyListener,
    priority: i32,
    created_order: u64,
) -> ProtocolDocumentRuleDefinition {
    ProtocolDocumentRuleDefinition::new(
        ProtocolDocumentRuleId::from_uuid(Uuid::from_u128(u128::from(10 - created_order))),
        true,
        priority,
        created_order,
        listener.id,
        snapshot_package(),
        ProtocolDirection::Upstream,
        vec![intercept_proxy_domain::ProtocolDocumentPredicate::Equals {
            field: intercept_proxy_domain::JsonPointer::property("amount"),
            value: intercept_proxy_domain::DocumentValue::integer(1).unwrap(),
        }],
        vec![ProtocolDocumentOperation::RecordMatch],
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
