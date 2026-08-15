//! 实际协议包驱动的 Scripted Relay TCP 运行链测试。

use std::io::{Cursor, Write};
use std::sync::Arc;

use intercept_proxy_domain::{
    DirectionProcessingOptions, DocumentAction, DocumentCondition, DocumentFieldName,
    DocumentValue, ListenerDataPlane, ProtocolPackageId, ProtocolPackageRef,
    ProtocolPackageVersion, ProxyListener, ProxyWorkspace, ScriptedSocketProcessing,
    SocketDirection, SocketDocumentRuleDefinition, SocketDocumentRuleId, SocketEndpoint,
    SocketPayloadProcessing, SocketRelaySecurity, SocketRelaySettings, SocketRelayTopology,
    SocketTopology,
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

[document]
schema = "document.toml"

[document.display]
script = "protocol.rhai"
function = "display"

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

fn display(document, context) { "<p>runtime</p>" }
"#;

#[tokio::test]
async fn all_sixteen_direction_state_pairs_use_exact_real_tcp_wire_semantics() {
    for upstream_state in states() {
        for downstream_state in states() {
            run_case(upstream_state, downstream_state).await;
        }
    }
}

async fn run_case(upstream_state: State, downstream_state: State) {
    let upstream = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let upstream_address = upstream.local_addr().unwrap();
    let listener_port = reserve_port().await;
    let listener = listener(
        listener_port,
        upstream_address.port(),
        upstream_state,
        downstream_state,
    );
    let workspace = workspace(&listener, upstream_state, downstream_state);
    let expected_upstream = expected([2, 11], 161, upstream_state);
    let expected_downstream = expected([2, 22], 209, downstream_state);
    let upstream_task = tokio::spawn(async move {
        let (mut stream, _) = upstream.accept().await.unwrap();
        let mut request = [0_u8; 2];
        stream.read_exact(&mut request).await.unwrap();
        assert_eq!(request, expected_upstream);
        stream.write_all(&[2, 22]).await.unwrap();
        stream.shutdown().await.unwrap();
    });

    let store = Arc::new(SqliteStore::in_memory().unwrap());
    let repository = Arc::new(ProtocolPackageRepositoryAdapter::with_default_limits(
        Arc::clone(&store),
    ));
    repository.install_zip(&package_zip()).unwrap();
    repository.set_enabled(&package(), true).unwrap();
    let captures = Arc::new(crate::adapters::SocketCaptureRepositoryAdapter::new(
        Arc::clone(&store),
    ));
    let runtime = ListenerRuntimeAdapter::new(store).with_protocol_packages(repository);
    runtime.set_socket_capture_repository(Arc::clone(&captures));
    runtime.start(workspace, listener.clone()).await.unwrap();

    let mut client = TcpStream::connect(("127.0.0.1", listener_port))
        .await
        .unwrap();
    client.write_all(&[2, 11]).await.unwrap();
    client.shutdown().await.unwrap();
    let mut response = Vec::new();
    client.read_to_end(&mut response).await.unwrap();
    assert_eq!(response, expected_downstream);

    upstream_task.await.unwrap();
    let page = tokio::time::timeout(std::time::Duration::from_secs(2), async {
        loop {
            let page = captures.query(&captures::query()).unwrap();
            if page.rows.len() == 2 {
                break page;
            }
            tokio::time::sleep(std::time::Duration::from_millis(5)).await;
        }
    })
    .await
    .expect("two committed relay captures should persist within the bounded wait");
    for row in page.rows {
        let detail = captures.get_detail(row.capture_id).unwrap().record;
        let intercept_proxy_application::SocketCapturePayload::RelayFrame(frame) = detail.payload
        else {
            panic!("expected relay frame")
        };
        let (state, origin, written) = match frame.direction {
            SocketDirection::Upstream => (upstream_state, vec![2, 11], expected_upstream.to_vec()),
            SocketDirection::Downstream => {
                (downstream_state, vec![2, 22], expected_downstream.to_vec())
            }
        };
        assert_eq!(frame.origin, origin);
        assert_eq!(frame.written, written);
        assert_eq!(frame.document.is_some(), state.decode);
        assert_eq!(
            frame.write_kind,
            if state.encode {
                intercept_proxy_application::SocketWriteKind::Encoded
            } else {
                intercept_proxy_application::SocketWriteKind::Original
            }
        );
        assert!(if state.encode {
            matches!(
                frame.display,
                intercept_proxy_application::SocketDisplayResult::UntrustedHtml { .. }
            )
        } else {
            matches!(
                frame.display,
                intercept_proxy_application::SocketDisplayResult::HexFallback {
                    reason:
                        intercept_proxy_application::SocketDisplayFallbackReason::EncodeDisabled,
                    ..
                }
            )
        });
    }
    runtime.stop(listener.id).await.unwrap();
}

#[derive(Clone, Copy, Debug)]
struct State {
    decode: bool,
    encode: bool,
}

fn states() -> [State; 4] {
    [
        State {
            decode: false,
            encode: false,
        },
        State {
            decode: true,
            encode: false,
        },
        State {
            decode: false,
            encode: true,
        },
        State {
            decode: true,
            encode: true,
        },
    ]
}

fn expected(origin: [u8; 2], marker: u8, state: State) -> [u8; 2] {
    if !state.encode {
        origin
    } else if state.decode {
        [marker, 42]
    } else {
        [marker, 0]
    }
}

fn listener(port: u16, upstream_port: u16, upstream: State, downstream: State) -> ProxyListener {
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
            processing: SocketPayloadProcessing::Scripted(ScriptedSocketProcessing {
                package: package(),
                upstream: options(upstream),
                downstream: options(downstream),
            }),
        }),
        ..ProxyListener::default()
    }
}

fn workspace(listener: &ProxyListener, upstream: State, downstream: State) -> ProxyWorkspace {
    let mut rules = Vec::new();
    add_rule(&mut rules, listener, SocketDirection::Upstream, upstream, 1);
    add_rule(
        &mut rules,
        listener,
        SocketDirection::Downstream,
        downstream,
        2,
    );
    let created_order_high_water = rules
        .iter()
        .map(SocketDocumentRuleDefinition::created_order)
        .max()
        .unwrap_or(0);
    ProxyWorkspace {
        listeners: vec![listener.clone()],
        socket_rule_created_order_high_water: created_order_high_water,
        socket_rules: rules,
        ..ProxyWorkspace::default()
    }
}

fn add_rule(
    rules: &mut Vec<SocketDocumentRuleDefinition>,
    listener: &ProxyListener,
    direction: SocketDirection,
    state: State,
    created_order: u64,
) {
    if !state.decode {
        return;
    }
    let actions = if state.encode {
        vec![DocumentAction::SetField {
            field: DocumentFieldName::new("amount").unwrap(),
            value: DocumentValue::Int(42),
        }]
    } else {
        vec![DocumentAction::RecordMatch]
    };
    let expected_decoded_amount = match direction {
        SocketDirection::Upstream => 11,
        SocketDirection::Downstream => 22,
    };
    rules.push(
        SocketDocumentRuleDefinition::new(
            SocketDocumentRuleId::new(),
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
            actions,
        )
        .unwrap(),
    );
}

fn options(state: State) -> DirectionProcessingOptions {
    DirectionProcessingOptions {
        decode_enabled: state.decode,
        encode_enabled: state.encode,
    }
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
    ] {
        writer
            .start_file(path, SimpleFileOptions::default())
            .unwrap();
        writer.write_all(contents).unwrap();
    }
    writer.finish().unwrap().into_inner()
}

#[path = "scripted_relay_runtime/captures.rs"]
mod captures;
#[path = "scripted_relay_runtime/failures.rs"]
mod failures;
#[path = "scripted_relay_runtime/isolation.rs"]
mod isolation;
#[path = "scripted_relay_runtime/support.rs"]
mod support;
