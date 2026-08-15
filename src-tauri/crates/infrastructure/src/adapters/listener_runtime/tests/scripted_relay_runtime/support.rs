//! T24 Scripted Relay 真实 TCP 测试的最小协议包与运行时装配。

use super::*;

const TEST_SCHEMA: &str = r#"
id = "runtime-message"
version = 1
title = "Runtime Message"

[[fields]]
name = "amount"
label = "Amount"
type = "int"
"#;

pub(super) fn package_ref(id: &str) -> ProtocolPackageRef {
    ProtocolPackageRef {
        id: ProtocolPackageId::new(id).unwrap(),
        version: ProtocolPackageVersion::new("1.0.0").unwrap(),
    }
}

fn package_zip(id: &str, script: &str, include_display: bool) -> Vec<u8> {
    let display = if include_display {
        r#"
[document.display]
script = "protocol.rhai"
function = "display"
"#
    } else {
        ""
    };
    let manifest = format!(
        r#"
api = 1

[package]
id = "{id}"
name = "T24 Runtime Test"
version = "1.0.0"

[document]
schema = "document.toml"

{display}

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
"#
    );
    let mut writer = ZipWriter::new(Cursor::new(Vec::new()));
    for (path, contents) in [
        ("manifest.toml", manifest.as_bytes()),
        ("document.toml", TEST_SCHEMA.as_bytes()),
        ("protocol.rhai", script.as_bytes()),
    ] {
        writer
            .start_file(path, SimpleFileOptions::default())
            .unwrap();
        writer.write_all(contents).unwrap();
    }
    writer.finish().unwrap().into_inner()
}

pub(super) async fn start_scripted_runtime(
    id: &str,
    script: &str,
    listener_port: u16,
    upstream_port: u16,
) -> (ListenerRuntimeAdapter, ProxyListener) {
    let (runtime, listener, _) =
        start_scripted_runtime_with_capture(id, script, listener_port, upstream_port, true).await;
    (runtime, listener)
}

pub(super) async fn start_scripted_runtime_with_capture(
    id: &str,
    script: &str,
    listener_port: u16,
    upstream_port: u16,
    include_display: bool,
) -> (
    ListenerRuntimeAdapter,
    ProxyListener,
    Arc<crate::adapters::SocketCaptureRepositoryAdapter>,
) {
    let package = package_ref(id);
    let listener = ProxyListener {
        name: format!("T24 {id}"),
        bind_address: "127.0.0.1".into(),
        port: listener_port,
        data_plane: ListenerDataPlane::Socket(SocketRelaySettings {
            topology: SocketTopology::Relay(SocketRelayTopology {
                upstream: SocketEndpoint {
                    host: "127.0.0.1".into(),
                    port: upstream_port,
                },
                security: SocketRelaySecurity::Transparent,
            }),
            maximum_connections: 8,
            processing: SocketPayloadProcessing::Scripted(ScriptedSocketProcessing {
                package: package.clone(),
                upstream: DirectionProcessingOptions {
                    decode_enabled: true,
                    encode_enabled: true,
                },
                downstream: DirectionProcessingOptions {
                    decode_enabled: true,
                    encode_enabled: true,
                },
            }),
        }),
        ..ProxyListener::default()
    };
    start_scripted_runtime_from_listener(id, script, listener, include_display, None, true).await
}

pub(super) async fn start_scripted_runtime_from_listener(
    id: &str,
    script: &str,
    listener: ProxyListener,
    include_display: bool,
    events: Option<Arc<intercept_proxy_application::EventHub>>,
    publish_captures: bool,
) -> (
    ListenerRuntimeAdapter,
    ProxyListener,
    Arc<crate::adapters::SocketCaptureRepositoryAdapter>,
) {
    let package = package_ref(id);
    let workspace = ProxyWorkspace {
        listeners: vec![listener.clone()],
        ..ProxyWorkspace::default()
    };
    let store = Arc::new(SqliteStore::in_memory().unwrap());
    let repository = Arc::new(ProtocolPackageRepositoryAdapter::with_default_limits(
        Arc::clone(&store),
    ));
    repository
        .install_zip(&package_zip(id, script, include_display))
        .unwrap();
    repository.set_enabled(&package, true).unwrap();
    let captures = Arc::new(crate::adapters::SocketCaptureRepositoryAdapter::new(
        Arc::clone(&store),
    ));
    let runtime = ListenerRuntimeAdapter::new(store).with_protocol_packages(repository);
    if publish_captures {
        runtime.set_socket_capture_repository(Arc::clone(&captures));
    }
    if let Some(events) = events {
        runtime.set_socket_diagnostic_events(events);
    }
    runtime.start(workspace, listener.clone()).await.unwrap();
    (runtime, listener, captures)
}

pub(super) async fn read_to_end_bounded(stream: &mut TcpStream) -> Vec<u8> {
    let mut bytes = Vec::new();
    tokio::time::timeout(
        std::time::Duration::from_secs(2),
        stream.read_to_end(&mut bytes),
    )
    .await
    .expect("connection must reach a terminal state")
    .unwrap();
    bytes
}
