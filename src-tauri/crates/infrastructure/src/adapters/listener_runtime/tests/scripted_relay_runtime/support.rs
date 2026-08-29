//! T24 Scripted Relay 真实 TCP 测试的最小协议包与运行时装配。

use super::*;

const TEST_SCHEMA: &str = r#"
type = "object"
title = "Runtime Message"

[properties.amount]
type = "number"
title = "Amount"
"#;

pub(super) fn package_ref(id: &str) -> ProtocolPackageRef {
    ProtocolPackageRef {
        id: ProtocolPackageId::new(id).unwrap(),
        version: ProtocolPackageVersion::new("1.0.0").unwrap(),
    }
}

fn package_zip(id: &str, script: &str) -> Vec<u8> {
    let display_script = if script.contains("fn display(") {
        script
    } else {
        r#"fn display(document, context) { "<p>runtime</p>" }"#
    };
    let manifest = format!(
        r#"
api = 1

[package]
id = "{id}"
name = "T24 Runtime Test"
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
"#
    );
    let mut writer = ZipWriter::new(Cursor::new(Vec::new()));
    for (path, contents) in [
        ("manifest.toml", manifest.as_bytes()),
        ("document.toml", TEST_SCHEMA.as_bytes()),
        ("protocol.rhai", script.as_bytes()),
        ("display.rhai", display_script.as_bytes()),
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
            runtime_limits: intercept_proxy_domain::SocketRuntimeLimits::default(),
            processing: SocketPayloadProcessing::Scripted(ScriptedSocketProcessing {
                package: package.clone(),
            }),
        }),
        ..ProxyListener::default()
    };
    start_scripted_runtime_from_listener(id, script, listener, None).await
}

pub(super) async fn start_scripted_runtime_from_listener(
    id: &str,
    script: &str,
    listener: ProxyListener,
    events: Option<Arc<intercept_proxy_application::EventHub>>,
) -> (ListenerRuntimeAdapter, ProxyListener) {
    let package = package_ref(id);
    let workspace = ProxyWorkspace {
        listeners: vec![listener.clone()],
        ..ProxyWorkspace::default()
    };
    let store = Arc::new(SqliteStore::in_memory().unwrap());
    let repository = Arc::new(ProtocolPackageRepositoryAdapter::with_default_limits(
        Arc::clone(&store),
    ));
    repository.install_zip(&package_zip(id, script)).unwrap();
    repository.set_enabled(&package, true).unwrap();
    let runtime = test_listener_runtime_with_packages(store, repository);
    if let Some(events) = events {
        runtime.set_socket_diagnostic_events(events);
    }
    runtime.start(workspace, listener.clone()).await.unwrap();
    (runtime, listener)
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
