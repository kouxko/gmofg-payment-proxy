//! `LocalResponder` 真实运行链 fixture：协议包 ZIP、Listener、Workspace 与 TCP 客户端。

use std::{
    io::{Cursor, Write},
    sync::Arc,
};

use intercept_proxy_domain::{
    ListenerDataPlane, ProtocolDocumentRuleDefinition, ProtocolPackageId, ProtocolPackageRef,
    ProtocolPackageVersion, ProxyListener, ProxyWorkspace, ScriptedSocketProcessing,
    SocketDownstreamSecurity, SocketLocalResponderTopology, SocketPayloadProcessing,
    SocketRelaySettings, SocketTopology,
};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::{TcpListener, TcpStream},
};
use zip::{ZipWriter, write::SimpleFileOptions};

use super::*;

pub(super) const BASIC_SCHEMA: &str = r#"
id = "local-basic"
version = 1
title = "Local Basic"

[[fields]]
name = "amount"
label = "Amount"
type = "int"
"#;

pub(super) const BASIC_SCRIPT: &str = r#"
fn frame(reader, context) {
    if reader.available() < 1 {
        framing::need_more(1)
    } else {
        let total = reader.peek_u8(0);
        if reader.available() < total { framing::need_more(total) }
        else { framing::complete(total) }
    }
}

fn decode(origin, context) {
    let result = document::create();
    result.set("amount", origin[1].to_int());
    result
}

fn encode(origin, document, context) {
    let result = blob(2, 0);
    result[0] = 209;
    result[1] = if document.has("amount") { document.get("amount") } else { 0 };
    result
}

fn display(document, context) { "<p>local response</p>" }
"#;

pub(super) const ISO_SCHEMA: &str = r#"
id = "local-iso8583"
version = 1
title = "Local ISO8583"

[[fields]]
name = "message"
label = "Message"
type = "blob"
"#;

pub(super) const ISO_SCRIPT: &str = r#"
fn frame(reader, context) {
    if reader.available() < 18 { framing::need_more(18) }
    else { framing::complete(18) }
}

fn decode(origin, context) {
    let result = document::create();
    result.set("message", origin);
    result
}

fn encode(origin, document, context) {
    document.get("message")
}

fn display(document, context) { "<p>iso8583 response</p>" }
"#;

pub(super) fn package_ref(id: &str) -> ProtocolPackageRef {
    ProtocolPackageRef {
        id: ProtocolPackageId::new(id).unwrap(),
        version: ProtocolPackageVersion::new("1.0.0").unwrap(),
    }
}

pub(super) fn local_listener(id: &str, listener_port: u16) -> ProxyListener {
    ProxyListener {
        name: format!("LocalResponder {id}"),
        bind_address: "127.0.0.1".into(),
        port: listener_port,
        data_plane: ListenerDataPlane::Socket(SocketRelaySettings {
            topology: SocketTopology::LocalResponder(SocketLocalResponderTopology {
                downstream_security: SocketDownstreamSecurity::Tcp,
            }),
            maximum_connections: 8,
            processing: SocketPayloadProcessing::Scripted(ScriptedSocketProcessing {
                package: package_ref(id),
            }),
        }),
        ..ProxyListener::default()
    }
}

pub(super) fn listener_package_id(listener: &ProxyListener) -> &str {
    let ListenerDataPlane::Socket(settings) = &listener.data_plane else {
        unreachable!("fixture is always Socket")
    };
    let SocketPayloadProcessing::Scripted(scripted) = &settings.processing else {
        unreachable!("fixture is always Scripted")
    };
    scripted.package.id.as_str()
}

pub(super) fn workspace(
    listener: ProxyListener,
    rules: Vec<ProtocolDocumentRuleDefinition>,
) -> ProxyWorkspace {
    let created_order_high_water = rules
        .iter()
        .map(ProtocolDocumentRuleDefinition::created_order)
        .max()
        .unwrap_or(0);
    ProxyWorkspace {
        listeners: vec![listener],
        protocol_rule_created_order_high_water: created_order_high_water,
        protocol_rules: rules,
        ..ProxyWorkspace::default()
    }
}

pub(super) async fn start_local_runtime(
    id: &str,
    schema: &str,
    script: &str,
    workspace: ProxyWorkspace,
    listener: &ProxyListener,
) -> ListenerRuntimeAdapter {
    start_local_runtime_inner(id, schema, schema, script, workspace, listener, None)
        .await
        .0
}

pub(super) async fn start_local_runtime_with_events(
    id: &str,
    schema: &str,
    script: &str,
    workspace: ProxyWorkspace,
    listener: &ProxyListener,
    events: Arc<intercept_proxy_application::EventHub>,
) -> ListenerRuntimeAdapter {
    start_local_runtime_inner(
        id,
        schema,
        schema,
        script,
        workspace,
        listener,
        Some(events),
    )
    .await
    .0
}

pub(super) async fn start_local_runtime_with_capture(
    id: &str,
    schema: &str,
    script: &str,
    workspace: ProxyWorkspace,
    listener: &ProxyListener,
    events: Arc<intercept_proxy_application::EventHub>,
) -> (
    ListenerRuntimeAdapter,
    Arc<crate::adapters::SocketCaptureRepositoryAdapter>,
) {
    start_local_runtime_inner(
        id,
        schema,
        schema,
        script,
        workspace,
        listener,
        Some(events),
    )
    .await
}

pub(super) async fn start_local_runtime_with_directional_schemas_and_capture(
    id: &str,
    upstream_schema: &str,
    downstream_schema: &str,
    script: &str,
    workspace: ProxyWorkspace,
    listener: &ProxyListener,
    events: Arc<intercept_proxy_application::EventHub>,
) -> (
    ListenerRuntimeAdapter,
    Arc<crate::adapters::SocketCaptureRepositoryAdapter>,
) {
    start_local_runtime_inner(
        id,
        upstream_schema,
        downstream_schema,
        script,
        workspace,
        listener,
        Some(events),
    )
    .await
}

async fn start_local_runtime_inner(
    id: &str,
    upstream_schema: &str,
    downstream_schema: &str,
    script: &str,
    workspace: ProxyWorkspace,
    listener: &ProxyListener,
    events: Option<Arc<intercept_proxy_application::EventHub>>,
) -> (
    ListenerRuntimeAdapter,
    Arc<crate::adapters::SocketCaptureRepositoryAdapter>,
) {
    let store = Arc::new(SqliteStore::in_memory().unwrap());
    let repository = Arc::new(ProtocolPackageRepositoryAdapter::with_default_limits(
        Arc::clone(&store),
    ));
    repository
        .install_zip(&package_zip(id, upstream_schema, downstream_schema, script))
        .unwrap();
    repository.set_enabled(&package_ref(id), true).unwrap();
    let capture = Arc::new(crate::adapters::SocketCaptureRepositoryAdapter::new(
        Arc::clone(&store),
    ));
    let runtime = test_listener_runtime_with_packages(store, repository);
    runtime.set_socket_capture_repository(Arc::clone(&capture));
    if let Some(events) = events {
        runtime.set_socket_diagnostic_events(events);
    }
    runtime.start(workspace, listener.clone()).await.unwrap();
    (runtime, capture)
}

pub(super) async fn request_once(port: u16, request: &[u8]) -> Vec<u8> {
    let mut client = TcpStream::connect(("127.0.0.1", port)).await.unwrap();
    client.write_all(request).await.unwrap();
    client.shutdown().await.unwrap();
    let mut response = Vec::new();
    client.read_to_end(&mut response).await.unwrap();
    response
}

pub(super) async fn reserve_port() -> u16 {
    TcpListener::bind("127.0.0.1:0")
        .await
        .unwrap()
        .local_addr()
        .unwrap()
        .port()
}

fn package_zip(id: &str, upstream_schema: &str, downstream_schema: &str, script: &str) -> Vec<u8> {
    let manifest = format!(
        r#"
api = 1

[package]
id = "{id}"
name = "T25 LocalResponder Test"
version = "1.0.0"

[document.upstream]
schema = "upstream.toml"
display = "display"

[document.downstream]
schema = "downstream.toml"
display = "display"

[hooks.upstream]
frame = "frame"
decode = "decode"
encode = "encode"

[hooks.downstream]
frame = "frame"
decode = "decode"
encode = "encode"
"#,
    );
    let mut writer = ZipWriter::new(Cursor::new(Vec::new()));
    for (path, contents) in [
        ("manifest.toml", manifest.as_bytes()),
        ("upstream.toml", upstream_schema.as_bytes()),
        ("downstream.toml", downstream_schema.as_bytes()),
        ("protocol.rhai", script.as_bytes()),
        ("display.rhai", script.as_bytes()),
    ] {
        writer
            .start_file(path, SimpleFileOptions::default())
            .unwrap();
        writer.write_all(contents).unwrap();
    }
    writer.finish().unwrap().into_inner()
}
