use std::{
    io::{Cursor, Write},
    process::Stdio,
};

use futures_util::{SinkExt, StreamExt};
use serde_json::{Value, json};
use tempfile::TempDir;
use tokio::{net::TcpListener, process::Command};
use tokio_tungstenite::{accept_async, tungstenite::Message};
use zip::{ZipWriter, write::SimpleFileOptions};

const MANIFEST: &str = include_str!(
    "../../../../test-support/fixtures/task-20260829-002/phase-4/package-contract/http-manifest.json"
);

fn archive() -> (TempDir, std::path::PathBuf) {
    let directory = TempDir::new().unwrap();
    let path = directory.path().join("package.zip");
    let mut bytes = Cursor::new(Vec::new());
    let mut writer = ZipWriter::new(&mut bytes);
    for (name, source) in [
        ("manifest.json", MANIFEST),
        (
            "protocol.js",
            "export function upstreamDecode({input}) { return {input}; }\n\
             export function downstreamDecode({input}) { return {input}; }\n\
             export function upstreamEncode({originalInput}) { return originalInput; }\n\
             export function downstreamEncode({originalInput}) { return originalInput; }",
        ),
        (
            "display.js",
            "export function upstreamDisplay() { return '<p>up</p>'; }\n\
             export function downstreamDisplay() { return '<p>down</p>'; }",
        ),
    ] {
        writer
            .start_file(name, SimpleFileOptions::default())
            .unwrap();
        writer.write_all(source.as_bytes()).unwrap();
    }
    writer.finish().unwrap();
    std::fs::write(&path, bytes.into_inner()).unwrap();
    (directory, path)
}

#[tokio::test]
async fn process_initiates_registration_and_serves_fixed_rpc_until_killed() {
    let (_directory, archive) = archive();
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let mut child = Command::new(env!("CARGO_BIN_EXE_intercept-proxy-package-sidecar"))
        .arg("--archive")
        .arg(&archive)
        .arg("--packages-url")
        .arg(format!("ws://{address}/packages"))
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();

    let (stream, _) = listener.accept().await.unwrap();
    let mut websocket = accept_async(stream).await.unwrap();

    let registration = websocket
        .next()
        .await
        .unwrap()
        .unwrap()
        .into_text()
        .unwrap();
    let registration: Value = serde_json::from_str(&registration).unwrap();
    assert_eq!(registration["method"], "package.register");
    assert!(registration.get("id").is_none());

    websocket
        .send(Message::Text(
            json!({
                "jsonrpc":"2.0",
                "id":"1",
                "method":"hooks.upstream.decode",
                "params":{"input":"hello"}
            })
            .to_string()
            .into(),
        ))
        .await
        .unwrap();
    let response = websocket
        .next()
        .await
        .unwrap()
        .unwrap()
        .into_text()
        .unwrap();
    let response: Value = serde_json::from_str(&response).unwrap();
    assert_eq!(
        response,
        json!({"jsonrpc":"2.0","id":"1","result":{"input":"hello"}})
    );

    child.kill().await.unwrap();
    assert!(!child.wait().await.unwrap().success());
}

#[tokio::test]
async fn missing_launch_arguments_fail_closed() {
    let status = Command::new(env!("CARGO_BIN_EXE_intercept-proxy-package-sidecar"))
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .await
        .unwrap();
    assert!(!status.success());
}
