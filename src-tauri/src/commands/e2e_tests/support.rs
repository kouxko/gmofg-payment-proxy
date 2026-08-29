use std::{
    collections::VecDeque,
    io::{Cursor, Write},
    path::PathBuf,
    sync::{Arc, Mutex},
};

use intercept_proxy_application::{AppErrorViewModel, AppResult};
use intercept_proxy_host::{ApplicationHostBuilder, HostPlatformServices};
use intercept_proxy_infrastructure::{
    FileSelection, InfrastructureError, NativeFileDialog, SecretProtector,
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
use crate::mcp::{ApplicationBackend, McpBackend};

const MANIFEST: &str = r#"
api = 1

[package]
id = "t30-iso-local"
name = "T30 ISO LocalResponder"
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
type = "object"
title = "T30 ISO8583"

[properties.message]
type = "array"
title = "Message"

[properties.message.items]
type = "number"
"#;

const SCRIPT: &str = r#"
fn frame(reader, context) {
    if reader.available() < 18 { framing::need_more(18) }
    else { framing::complete(18) }
}

fn decode(origin, context) {
    let result = document::create();
    result.set("/message", origin);
    result
}

fn encode(origin, document, context) {
    let message = document.get("/message");
    let result = blob(message.len(), 0);
    for index in 0..message.len() { result[index] = message[index]; }
    result
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
        let unused_save_path = directory.path().join("unused-save-target");
        std::fs::write(&zip_path, package_zip()).unwrap();
        let dialog = Arc::new(TestDialog {
            open_paths: Mutex::new(VecDeque::from([zip_path])),
            save_path: unused_save_path,
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
                listener_new,
                listener_save,
                listener_start,
                listener_stop,
                protocol_package_import,
                protocol_package_import_commit,
                protocol_package_detail,
                protocol_package_enable,
                rule_definition_save,
                exchange_observation_query,
                exchange_observation_get,
                diagnostic_log_query,
                diagnostic_reproduction_report_export,
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

    pub(super) fn assert_dialog_boundaries(&self) {
        assert_eq!(
            self.dialog.calls.lock().unwrap().as_slice(),
            &[("protocol_package_zip".into(), "open")]
        );
    }

    pub(super) fn saved_report(&self) -> String {
        std::fs::read_to_string(&self.dialog.save_path).expect("saved reproduction report")
    }

    pub(super) fn assert_report_dialog_boundary(&self) {
        assert_eq!(
            self.dialog.calls.lock().unwrap().as_slice(),
            &[("diagnostic_reproduction_markdown".into(), "save")]
        );
    }

    pub(super) fn call_mcp_tool(&self, name: &str, arguments: Value) -> Value {
        let state = self.app.state::<AppState>();
        let backend = ApplicationBackend::new(
            Arc::clone(&state.application),
            state.runtime_logs(),
            state.exchange_observations(),
        );
        tauri::async_runtime::block_on(backend.call_tool(name, arguments))
            .unwrap_or_else(|error| panic!("MCP {name} failed: {} {}", error.code, error.message))
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
        ("display.rhai", SCRIPT.as_bytes()),
    ] {
        archive
            .start_file(path, SimpleFileOptions::default())
            .unwrap();
        archive.write_all(contents).unwrap();
    }
    archive.finish().unwrap().into_inner()
}
