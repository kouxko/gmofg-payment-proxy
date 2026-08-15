use std::{
    collections::VecDeque,
    io::{Cursor, Write},
    path::{Path, PathBuf},
    sync::{Arc, Mutex},
};

use intercept_proxy_application::{AppErrorViewModel, AppResult};
use intercept_proxy_host::{ApplicationHostBuilder, HostPlatformServices};
use intercept_proxy_infrastructure::{
    InfrastructureError, NativeFileDialog, SecretProtector, adapters::FileSelection,
};
use intercept_proxy_product_api::InterceptProxyProfile;
use serde::de::DeserializeOwned;
use serde_json::Value;
use tauri::{
    Manager, WebviewUrl, WebviewWindowBuilder, http::HeaderMap, ipc::InvokeBody, test::MockRuntime,
};
use tempfile::TempDir;
use zip::{ZipWriter, write::SimpleFileOptions};

use super::super::*;
use crate::app_state::AppState;

const MANIFEST: &str = r#"
api = 1

[package]
id = "t30-iso-local"
name = "T30 ISO LocalResponder"
version = "1.0.0"

[document]
schema = "document.toml"
display = { script = "protocol.rhai", function = "display" }

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
id = "t30-iso8583"
version = 1
title = "T30 ISO8583"

[[fields]]
name = "mti"
label = "MTI"
type = "string"

[[fields]]
name = "trace"
label = "Trace"
type = "string"

[[fields]]
name = "amount"
label = "Amount"
type = "string"

[[fields]]
name = "response_code"
label = "Response Code"
type = "string"
"#;

const SCRIPT: &str = r#"
fn frame(reader, context) {
    if reader.available() < 18 { framing::need_more(18) }
    else { framing::complete(18) }
}

fn decode(origin, context) {
    let result = document::create();
    result.set("mti", origin.extract(0, 4).as_string());
    result.set("trace", origin.extract(4, 6).as_string());
    result.set("amount", origin.extract(10, 8).as_string());
    result
}

fn encode(origin, document, context) {
    document.get("mti").to_blob()
        + document.get("trace").to_blob()
        + document.get("amount").to_blob()
        + document.get("response_code").to_blob()
}

fn display(document, context) { "<p>T30 ISO response</p>" }
"#;

#[derive(Debug)]
struct TestDialog {
    open_paths: Mutex<VecDeque<PathBuf>>,
    save_path: PathBuf,
    calls: Mutex<Vec<(String, &'static str)>>,
}

impl NativeFileDialog for TestDialog {
    fn choose_open_file(&self, purpose: &str) -> AppResult<Option<PathBuf>> {
        self.calls.lock().unwrap().push((purpose.into(), "open"));
        Ok(self.open_paths.lock().unwrap().pop_front())
    }

    fn choose_save_file(&self, purpose: &str, _: &str) -> AppResult<Option<FileSelection>> {
        self.calls.lock().unwrap().push((purpose.into(), "save"));
        Ok(Some(FileSelection {
            path: self.save_path.clone(),
            overwrite_confirmed: false,
        }))
    }
}

#[derive(Debug)]
struct TestSecretProtector;

impl SecretProtector for TestSecretProtector {
    fn protect(&self, plaintext: &[u8]) -> Result<Vec<u8>, InfrastructureError> {
        Ok(plaintext.to_vec())
    }

    fn unprotect(&self, ciphertext: &[u8]) -> Result<Vec<u8>, InfrastructureError> {
        Ok(ciphertext.to_vec())
    }
}

pub(super) struct CrossLayerFixture {
    _directory: TempDir,
    app: tauri::App<MockRuntime>,
    dialog: Arc<TestDialog>,
}

impl CrossLayerFixture {
    pub(super) fn new() -> Self {
        let directory = TempDir::new().unwrap();
        let zip_path = directory.path().join("t30-iso.zip");
        let export_path = directory.path().join("round-trip.intercept-workspace");
        std::fs::write(&zip_path, package_zip()).unwrap();
        let dialog = Arc::new(TestDialog {
            open_paths: Mutex::new(VecDeque::from([zip_path, export_path.clone()])),
            save_path: export_path,
            calls: Mutex::new(Vec::new()),
        });
        let host = tauri::async_runtime::block_on(
            ApplicationHostBuilder::new(
                directory.path(),
                HostPlatformServices::new(Arc::new(TestSecretProtector), dialog.clone()),
                Arc::new(InterceptProxyProfile),
            )
            .build(),
        )
        .unwrap();
        let app = tauri::test::mock_builder()
            .manage(AppState::new(host))
            .invoke_handler(tauri::generate_handler![
                workspace_list,
                workspace_get,
                workspace_import,
                workspace_export,
                listener_new,
                listener_save,
                listener_start,
                listener_stop,
                protocol_package_import,
                protocol_package_import_commit,
                protocol_package_detail,
                protocol_package_enable,
                socket_rule_save,
                socket_capture_query,
                socket_capture_get_detail,
            ])
            .build(tauri::test::mock_context(tauri::test::noop_assets()))
            .unwrap();
        Self {
            _directory: directory,
            app,
            dialog,
        }
    }

    pub(super) fn webview(&self) -> tauri::WebviewWindow<MockRuntime> {
        WebviewWindowBuilder::new(&self.app, "main", WebviewUrl::default())
            .build()
            .unwrap()
    }

    pub(super) fn invoke_ok<T: DeserializeOwned>(
        &self,
        webview: &tauri::WebviewWindow<MockRuntime>,
        command: &str,
        body: Value,
    ) -> T {
        assert!(self.app.get_webview_window(webview.label()).is_some());
        tauri::test::get_ipc_response(webview, request(command, body))
            .unwrap_or_else(|error| {
                let error: AppErrorViewModel = serde_json::from_value(error).unwrap();
                panic!("{command} failed: {} {}", error.code, error.message)
            })
            .deserialize()
            .unwrap()
    }

    pub(super) fn export_path(&self) -> &Path {
        &self.dialog.save_path
    }

    pub(super) fn assert_dialog_boundaries(&self) {
        assert_eq!(
            self.dialog.calls.lock().unwrap().as_slice(),
            &[
                ("protocol_package_zip".into(), "open"),
                ("intercept_workspace".into(), "save"),
                ("intercept_workspace".into(), "open"),
            ]
        );
    }
}

pub(super) fn unused_local_port() -> u16 {
    std::net::TcpListener::bind("127.0.0.1:0")
        .unwrap()
        .local_addr()
        .unwrap()
        .port()
}

fn request(command: &str, body: Value) -> tauri::webview::InvokeRequest {
    tauri::webview::InvokeRequest {
        cmd: command.into(),
        callback: tauri::ipc::CallbackFn(0),
        error: tauri::ipc::CallbackFn(1),
        url: if cfg!(any(windows, target_os = "android")) {
            "http://tauri.localhost"
        } else {
            "tauri://localhost"
        }
        .parse()
        .unwrap(),
        body: InvokeBody::Json(body),
        headers: HeaderMap::default(),
        invoke_key: tauri::test::INVOKE_KEY.to_owned(),
    }
}

fn package_zip() -> Vec<u8> {
    let mut archive = ZipWriter::new(Cursor::new(Vec::new()));
    for (path, contents) in [
        ("manifest.toml", MANIFEST.as_bytes()),
        ("document.toml", SCHEMA.as_bytes()),
        ("protocol.rhai", SCRIPT.as_bytes()),
    ] {
        archive
            .start_file(path, SimpleFileOptions::default())
            .unwrap();
        archive.write_all(contents).unwrap();
    }
    archive.finish().unwrap().into_inner()
}
