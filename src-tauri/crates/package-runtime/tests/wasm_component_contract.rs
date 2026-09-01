use std::{borrow::Cow, path::PathBuf, process::Command};

use futures_util::{SinkExt, StreamExt};
use intercept_proxy_domain::{Document, ErrorCode, ProtocolDirection};
use intercept_proxy_package_contract::{FrameResult, PackageKind};
use intercept_proxy_package_runtime::{
    WasmPackageRuntime, embed_package_manifest, read_package_component,
};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use wasm_encoder::{Component, CustomSection, Module, ModuleSection};

const MANIFEST_SECTION: &str = "intercept-proxy:manifest";

fn manifest() -> &'static [u8] {
    br#"{
      "api": 1,
      "kind": "http",
      "package": {
        "id": "component-contract",
        "version": "1.0.0",
        "name": "Component Contract",
        "description": ""
      },
      "document": {
        "upstream": {},
        "downstream": {}
      }
    }"#
}

fn component_with_sections(sections: &[(&str, &[u8])]) -> Vec<u8> {
    let mut component = Component::new();
    for (name, data) in sections {
        component.section(&CustomSection {
            name: Cow::Borrowed(name),
            data: Cow::Borrowed(data),
        });
    }
    component.finish()
}

fn build_http_fixture() -> Vec<u8> {
    let manifest_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let fixture = manifest_dir.join("tests/fixtures/http-echo");
    let output = Command::new(env!("CARGO"))
        .args([
            "build",
            "--locked",
            "--manifest-path",
            fixture.join("Cargo.toml").to_str().expect("UTF-8 path"),
            "--target",
            "wasm32-wasip2",
        ])
        .output()
        .expect("build Rust HTTP Component fixture");
    assert!(
        output.status.success(),
        "fixture build failed:\n{}\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr),
    );
    let component = std::fs::read(
        fixture.join("target/wasm32-wasip2/debug/intercept_proxy_http_echo_component.wasm"),
    )
    .expect("read built Component fixture");
    let manifest = std::fs::read(fixture.join("manifest.json")).expect("read HTTP manifest");
    embed_package_manifest(&component, &manifest).expect("embed top-level HTTP manifest")
}

fn build_socket_fixture() -> Vec<u8> {
    let manifest_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let fixture = manifest_dir.join("../../../templates/socket-protocol/iso8583-standard");
    let output = Command::new(env!("CARGO"))
        .args([
            "build",
            "--locked",
            "--manifest-path",
            fixture.join("Cargo.toml").to_str().expect("UTF-8 path"),
            "--target",
            "wasm32-wasip2",
        ])
        .output()
        .expect("build Rust Socket Component fixture");
    assert!(
        output.status.success(),
        "fixture build failed:\n{}\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr),
    );
    let component =
        std::fs::read(fixture.join(
            "target/wasm32-wasip2/debug/intercept_proxy_iso8583_ascii_standard_component.wasm",
        ))
        .expect("read built Socket Component fixture");
    let manifest = std::fs::read(fixture.join("manifest.json")).expect("read Socket manifest");
    embed_package_manifest(&component, &manifest).expect("embed top-level Socket manifest")
}

#[test]
fn accepts_one_component_manifest_before_instantiation() {
    let bytes = component_with_sections(&[(MANIFEST_SECTION, manifest())]);

    let package = read_package_component(&bytes).expect("valid component payload");

    assert_eq!(package.manifest().kind(), PackageKind::Http);
    assert_eq!(
        package.manifest().package().identity().id.as_str(),
        "component-contract"
    );
    assert_eq!(package.bytes(), bytes.as_slice());
}

#[test]
fn rejects_core_wasm_even_when_it_has_a_manifest_section() {
    let mut module = Module::new();
    module.section(&CustomSection {
        name: Cow::Borrowed(MANIFEST_SECTION),
        data: Cow::Borrowed(manifest()),
    });

    assert!(read_package_component(&module.finish()).is_err());
}

#[test]
fn rejects_a_manifest_hidden_inside_a_nested_core_module() {
    let mut module = Module::new();
    module.section(&CustomSection {
        name: Cow::Borrowed(MANIFEST_SECTION),
        data: Cow::Borrowed(manifest()),
    });
    let mut component = Component::new();
    component.section(&ModuleSection(&module));

    assert!(read_package_component(&component.finish()).is_err());
}

#[test]
fn rejects_missing_duplicate_and_invalid_manifest_sections() {
    assert!(read_package_component(&component_with_sections(&[])).is_err());
    assert!(
        read_package_component(&component_with_sections(&[
            (MANIFEST_SECTION, manifest()),
            (MANIFEST_SECTION, manifest()),
        ]))
        .is_err()
    );
    assert!(
        read_package_component(&component_with_sections(&[(MANIFEST_SECTION, b"not-json")]))
            .is_err()
    );
}

#[tokio::test]
async fn rejects_component_missing_the_manifest_selected_world_exports() {
    let bytes = component_with_sections(&[(MANIFEST_SECTION, manifest())]);
    let package = read_package_component(&bytes).expect("static component contract");

    assert!(WasmPackageRuntime::load(&package).await.is_err());
}

#[tokio::test]
async fn loads_and_calls_a_rust_http_component_inside_the_current_process() {
    let bytes = build_http_fixture();
    let package = read_package_component(&bytes).expect("valid Rust HTTP Component");
    let mut runtime = WasmPackageRuntime::load(&package)
        .await
        .expect("instantiate HTTP world");
    let document_json = r#"{"message":"hello"}"#;
    let expected = serde_json::from_str::<Document>(document_json).expect("test Document");

    let decoded = runtime
        .decode_http(ProtocolDirection::Upstream, document_json)
        .await
        .expect("call decode");
    assert_eq!(decoded, expected);

    let encoded = runtime
        .encode_http(ProtocolDirection::Downstream, "ignored", &expected)
        .await
        .expect("call encode");
    assert_eq!(encoded, document_json);

    let displayed = runtime
        .display(ProtocolDirection::Upstream, &expected)
        .await
        .expect("call display");
    assert_eq!(displayed, document_json);
}

#[tokio::test]
async fn guest_inherits_environment_and_can_read_the_host_filesystem() {
    let temp = tempfile::TempDir::new().expect("temporary host directory");
    let host_path = temp.path().join("visible-to-guest.txt");
    std::fs::write(&host_path, "host-file").expect("write host fixture");
    let guest_path = guest_host_path(&host_path);
    let bytes = build_http_fixture();
    let package = read_package_component(&bytes).expect("valid Rust HTTP Component");
    let mut runtime = WasmPackageRuntime::load(&package)
        .await
        .expect("instantiate HTTP world with inherited WASI capabilities");

    let environment = runtime
        .decode_http(ProtocolDirection::Upstream, "wasi-env:PATH")
        .await
        .expect("guest reads inherited environment");
    assert_eq!(
        environment,
        serde_json::from_str::<Document>(r#"{"present":true}"#).unwrap()
    );

    let file = runtime
        .decode_http(
            ProtocolDirection::Upstream,
            &format!("wasi-read:{guest_path}"),
        )
        .await
        .expect("guest reads host file through the unrestricted preopen");
    assert_eq!(
        file,
        serde_json::from_str::<Document>(r#"{"contents":"host-file"}"#).unwrap()
    );
}

fn guest_host_path(host_path: &std::path::Path) -> String {
    #[cfg(unix)]
    {
        host_path.to_string_lossy().into_owned()
    }
    #[cfg(windows)]
    {
        let raw = host_path.to_string_lossy().replace('\\', "/");
        let (drive, relative) = raw.split_once(':').expect("Windows host path has drive");
        format!("/host/{}/{relative}", drive.to_ascii_lowercase())
    }
}

#[tokio::test]
async fn guest_can_open_and_close_a_host_websocket_connection() {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .expect("bind test WebSocket server");
    let address = listener.local_addr().expect("test server address");
    let server = tokio::spawn(async move {
        let (stream, _) = listener.accept().await.expect("accept TCP connection");
        let mut websocket = tokio_tungstenite::accept_async(stream)
            .await
            .expect("accept WebSocket handshake");
        assert!(matches!(
            websocket.next().await,
            Some(Ok(tokio_tungstenite::tungstenite::Message::Close(_)))
        ));
    });
    let bytes = build_http_fixture();
    let package = read_package_component(&bytes).expect("valid Rust HTTP Component");
    let mut runtime = WasmPackageRuntime::load(&package)
        .await
        .expect("instantiate HTTP world with Host WebSocket");

    let actual = runtime
        .decode_http(
            ProtocolDirection::Upstream,
            &format!("websocket:ws://{address}"),
        )
        .await
        .expect("guest opens and closes Host WebSocket");

    assert_eq!(actual, serde_json::from_str::<Document>("{}").unwrap());
    server.await.expect("WebSocket server task");
}

#[tokio::test]
async fn guest_websocket_supports_text_binary_receive_and_peer_close() {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .expect("bind test WebSocket server");
    let address = listener.local_addr().expect("test server address");
    let server = tokio::spawn(async move {
        let (stream, _) = listener.accept().await.expect("accept TCP connection");
        let mut websocket = tokio_tungstenite::accept_async(stream)
            .await
            .expect("accept WebSocket handshake");
        assert!(matches!(
            websocket.next().await,
            Some(Ok(tokio_tungstenite::tungstenite::Message::Text(value))) if value == "guest-text"
        ));
        assert!(matches!(
            websocket.next().await,
            Some(Ok(tokio_tungstenite::tungstenite::Message::Binary(value))) if value.as_ref() == [1, 2, 3]
        ));
        websocket
            .send(tokio_tungstenite::tungstenite::Message::Text(
                "host-text".into(),
            ))
            .await
            .expect("send text");
        websocket
            .send(tokio_tungstenite::tungstenite::Message::Binary(
                vec![4, 5, 6].into(),
            ))
            .await
            .expect("send binary");
        websocket.close(None).await.expect("send peer close");
    });
    let bytes = build_http_fixture();
    let package = read_package_component(&bytes).expect("valid Rust HTTP Component");
    let mut runtime = WasmPackageRuntime::load(&package)
        .await
        .expect("instantiate HTTP world with Host WebSocket");

    let actual = runtime
        .decode_http(
            ProtocolDirection::Upstream,
            &format!("websocket-roundtrip:ws://{address}"),
        )
        .await
        .expect("guest completes Host WebSocket roundtrip");

    assert_eq!(
        actual,
        serde_json::from_str::<Document>(r#"{"text":"host-text","binary":[4,5,6],"closed":""}"#)
            .unwrap()
    );
    server.await.expect("WebSocket server task");
}

#[tokio::test]
async fn guest_can_make_an_outbound_wasi_http_request() {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .expect("bind test HTTP server");
    let address = listener.local_addr().expect("test server address");
    let server = tokio::spawn(async move {
        let (mut stream, _) = listener.accept().await.expect("accept HTTP connection");
        let mut request = vec![0_u8; 4096];
        let request_bytes = stream.read(&mut request).await.expect("read HTTP request");
        assert!(
            String::from_utf8_lossy(&request[..request_bytes]).starts_with("GET /health HTTP/1.1")
        );
        stream
            .write_all(b"HTTP/1.1 200 OK\r\nContent-Length: 0\r\nConnection: close\r\n\r\n")
            .await
            .expect("write HTTP response");
    });
    let bytes = build_http_fixture();
    let package = read_package_component(&bytes).expect("valid Rust HTTP Component");
    let mut runtime = WasmPackageRuntime::load(&package)
        .await
        .expect("instantiate HTTP world with WASI HTTP");

    let actual = runtime
        .decode_http(ProtocolDirection::Upstream, &format!("wasi-http:{address}"))
        .await
        .expect("guest performs an outbound WASI HTTP request");

    assert_eq!(
        actual,
        serde_json::from_str::<Document>(r#"{"status":200}"#).unwrap()
    );
    server.await.expect("HTTP server task");
}

#[tokio::test]
async fn built_in_socket_component_preserves_frame_decode_encode_and_display_behavior() {
    let bytes = build_socket_fixture();
    let package = read_package_component(&bytes).expect("valid built-in Socket Component");
    let mut runtime = WasmPackageRuntime::load(&package)
        .await
        .expect("instantiate Socket world");
    let request = [0_u8, 4, b'0', b'2', b'0', b'0'];

    let frame = runtime
        .frame(ProtocolDirection::Upstream, &request)
        .await
        .expect("call frame");
    assert_eq!(frame, FrameResult::complete(request.len()).unwrap());

    let decoded = runtime
        .decode_socket(ProtocolDirection::Downstream, &request)
        .await
        .expect("call decode");
    assert_eq!(
        decoded,
        serde_json::from_str::<Document>(r#"{"message_type":"0200"}"#).unwrap()
    );

    let error = runtime
        .decode_socket(ProtocolDirection::Downstream, &[0_u8, 4, b'0'])
        .await
        .expect_err("guest decode error must cross WIT with its stable code");
    assert_eq!(error.code, ErrorCode::BodyDecodeFailed);

    let replacement = serde_json::from_str::<Document>(r#"{"message_type":"0210"}"#).unwrap();
    let encoded = runtime
        .encode_socket(ProtocolDirection::Upstream, &request, &replacement)
        .await
        .expect("call encode");
    assert_eq!(encoded, [0_u8, 4, b'0', b'2', b'1', b'0']);

    let displayed = runtime
        .display(ProtocolDirection::Downstream, &replacement)
        .await
        .expect("call display");
    assert!(displayed.contains("<td>0210</td>"));
}
