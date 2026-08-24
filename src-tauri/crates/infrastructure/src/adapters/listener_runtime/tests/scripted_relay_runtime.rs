//! 实际协议包驱动的 Scripted Relay TCP 运行链测试。

use std::io::{Cursor, Write};
use std::sync::Arc;

use intercept_proxy_domain::{
    DocumentAction, DocumentCondition, DocumentFieldName, DocumentValue, ListenerDataPlane,
    ProtocolDirection, ProtocolDocumentRuleDefinition, ProtocolDocumentRuleId, ProtocolPackageId,
    ProtocolPackageRef, ProtocolPackageVersion, ProxyListener, ProxyWorkspace,
    ScriptedSocketProcessing, SocketEndpoint, SocketPayloadProcessing, SocketRelaySecurity,
    SocketRelaySettings, SocketRelayTopology, SocketTopology,
};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::{TcpListener, TcpStream},
};
use zip::{ZipWriter, write::SimpleFileOptions};

use super::*;

const MANIFEST: &str = r#"
api = 1

[package]
id = "runtime-matrix"
name = "Runtime Matrix"
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

const SCHEMA: &str = r#"
id = "runtime-message"
version = 1
title = "Runtime Message"

[[fields]]
name = "amount"
label = "Amount"
type = "int"
"#;

const SCRIPT: &str = r#"
fn frame(reader, context) {
    if reader.available() < 2 { framing::need_more(2) } else { framing::complete(2) }
}

fn decode(origin, context) {
    let result = document::create();
    result.set("amount", origin[1]);
    result
}

fn encode(origin, document, context) {
    let result = origin;
    result[0] = if context.direction() == "upstream" { 161 } else { 209 };
    result[1] = if document.has("amount") { document.get("amount") } else { 0 };
    result
}
"#;

const DISPLAY: &str = r#"
fn display(document, context) { "<p>runtime</p>" }
"#;

#[tokio::test]
async fn both_directions_use_the_full_real_tcp_protocol_chain() {
    let upstream = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let upstream_address = upstream.local_addr().unwrap();
    let listener_port = reserve_port().await;
    let listener = listener(listener_port, upstream_address.port());
    let workspace = workspace(&listener);
    let upstream_task = tokio::spawn(async move {
        let (mut stream, _) = upstream.accept().await.unwrap();
        let mut request = [0_u8; 2];
        stream.read_exact(&mut request).await.unwrap();
        assert_eq!(request, [161, 42]);
        stream.write_all(&[2, 22]).await.unwrap();
        stream.shutdown().await.unwrap();
    });

    let store = Arc::new(SqliteStore::in_memory().unwrap());
    let repository = Arc::new(ProtocolPackageRepositoryAdapter::with_default_limits(
        Arc::clone(&store),
    ));
    repository.install_zip(&package_zip()).unwrap();
    repository.set_enabled(&package(), true).unwrap();
    let runtime = test_listener_runtime_with_packages(store, repository);
    runtime.start(workspace, listener.clone()).await.unwrap();

    let mut client = TcpStream::connect(("127.0.0.1", listener_port))
        .await
        .unwrap();
    client.write_all(&[2, 11]).await.unwrap();
    client.shutdown().await.unwrap();
    let mut response = Vec::new();
    client.read_to_end(&mut response).await.unwrap();
    assert_eq!(response, [209, 42]);

    upstream_task.await.unwrap();
    runtime.stop(listener.id).await.unwrap();
}

fn listener(port: u16, upstream_port: u16) -> ProxyListener {
    ProxyListener {
        name: "runtime matrix".into(),
        bind_address: "127.0.0.1".into(),
        port,
        data_plane: ListenerDataPlane::Socket(SocketRelaySettings {
            topology: SocketTopology::Relay(SocketRelayTopology {
                upstream: SocketEndpoint {
                    host: "127.0.0.1".into(),
                    port: upstream_port,
                },
                security: SocketRelaySecurity::Transparent,
            }),
            maximum_connections: 4,
            runtime_limits: intercept_proxy_domain::SocketRuntimeLimits::default(),
            processing: SocketPayloadProcessing::Scripted(ScriptedSocketProcessing {
                package: package(),
            }),
        }),
        ..ProxyListener::default()
    }
}

fn workspace(listener: &ProxyListener) -> ProxyWorkspace {
    let mut rules = Vec::new();
    add_rule(&mut rules, listener, ProtocolDirection::Upstream, 1);
    add_rule(&mut rules, listener, ProtocolDirection::Downstream, 2);
    let created_order_high_water = rules
        .iter()
        .map(ProtocolDocumentRuleDefinition::created_order)
        .max()
        .unwrap_or(0);
    ProxyWorkspace {
        listeners: vec![listener.clone()],
        protocol_rule_created_order_high_water: created_order_high_water,
        protocol_rules: rules,
        ..ProxyWorkspace::default()
    }
}

fn add_rule(
    rules: &mut Vec<ProtocolDocumentRuleDefinition>,
    listener: &ProxyListener,
    direction: ProtocolDirection,
    created_order: u64,
) {
    let expected_decoded_amount = match direction {
        ProtocolDirection::Upstream => 11,
        ProtocolDirection::Downstream => 22,
    };
    rules.push(
        ProtocolDocumentRuleDefinition::new(
            ProtocolDocumentRuleId::new(),
            true,
            10,
            created_order,
            listener.id,
            package(),
            1,
            direction,
            vec![DocumentCondition::Equals {
                field: DocumentFieldName::new("amount").unwrap(),
                value: DocumentValue::Int(expected_decoded_amount),
            }],
            vec![DocumentAction::SetField {
                field: DocumentFieldName::new("amount").unwrap(),
                value: DocumentValue::Int(42),
            }],
        )
        .unwrap(),
    );
}

async fn reserve_port() -> u16 {
    TcpListener::bind("127.0.0.1:0")
        .await
        .unwrap()
        .local_addr()
        .unwrap()
        .port()
}

fn package() -> ProtocolPackageRef {
    ProtocolPackageRef {
        id: ProtocolPackageId::new("runtime-matrix").unwrap(),
        version: ProtocolPackageVersion::new("1.0.0").unwrap(),
    }
}

fn package_zip() -> Vec<u8> {
    let mut writer = ZipWriter::new(Cursor::new(Vec::new()));
    for (path, contents) in [
        ("manifest.toml", MANIFEST.as_bytes()),
        ("document.toml", SCHEMA.as_bytes()),
        ("protocol.rhai", SCRIPT.as_bytes()),
        ("display.rhai", DISPLAY.as_bytes()),
    ] {
        writer
            .start_file(path, SimpleFileOptions::default())
            .unwrap();
        writer.write_all(contents).unwrap();
    }
    writer.finish().unwrap().into_inner()
}

#[path = "scripted_relay_runtime/flow_control.rs"]
mod flow_control;
#[path = "scripted_relay_runtime/isolation.rs"]
mod isolation;
#[path = "scripted_relay_runtime/support.rs"]
mod support;
