#[cfg(test)]
mod tests {
    use std::{
        collections::VecDeque,
        io::{Cursor, Write},
        path::{Path, PathBuf},
        sync::{Arc, Mutex},
    };

    use intercept_proxy_application::{AppError, AppErrorViewModel, AppResult};
    use intercept_proxy_host::{ApplicationHostBuilder, HostPlatformServices};
    use intercept_proxy_infrastructure::{
        InfrastructureError, NativeFileDialog, SecretProtector, adapters::FileSelection,
    };
    use intercept_proxy_domain::{
        DirectionProcessingOptions, ListenerDataPlane, ProtocolPackageRef,
        ScriptedSocketProcessing, SocketEndpoint, SocketPayloadProcessing, SocketRelaySettings,
    };
    use intercept_proxy_product_api::InterceptProxyProfile;
    use serde::de::DeserializeOwned;
    use serde_json::{Value, json};
    use tauri::{
        Manager, WebviewUrl, WebviewWindowBuilder, http::HeaderMap, ipc::InvokeBody,
        test::MockRuntime,
    };
    use tempfile::TempDir;
    use zip::{ZipWriter, write::SimpleFileOptions};

    use super::{
        protocol_package_delete, protocol_package_detail, protocol_package_disable,
        protocol_package_enable, protocol_package_import, protocol_package_import_commit,
        protocol_package_list, protocol_package_usage,
    };
    use crate::app_state::AppState;

    const MANIFEST: &str = r#"
api = 1

[package]
id = "example-protocol"
name = "Example Protocol"
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
"#;

    const SCHEMA: &str = r#"
id = "example-message"
version = 1
title = "Example Message"

[[fields]]
name = "trace_id"
label = "Trace ID"
type = "string"

[[fields]]
name = "amount"
label = "Amount"
type = "int"
"#;

    const SCRIPT: &str = r#"
fn frame(reader, context) { framing::complete(1) }
fn decode(origin, context) { document::create() }
fn encode(origin, document, context) { origin }
fn display(document, context) { "<p>ok</p>" }
"#;

    #[derive(Debug, Default)]
    struct QueueDialog {
        open: Mutex<VecDeque<AppResult<Option<PathBuf>>>>,
    }

    impl QueueDialog {
        fn with_open(response: AppResult<Option<PathBuf>>) -> Self {
            Self {
                open: Mutex::new(VecDeque::from([response])),
            }
        }
    }

    impl NativeFileDialog for QueueDialog {
        fn choose_open_file(&self, _: &str) -> AppResult<Option<PathBuf>> {
            self.open.lock().unwrap().pop_front().unwrap_or(Ok(None))
        }

        fn choose_save_file(&self, _: &str, _: &str) -> AppResult<Option<FileSelection>> {
            Ok(None)
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

    #[test]
    fn every_protocol_package_command_round_trips_through_real_tauri_ipc() {
        let fixture = TempDir::new().unwrap();
        let zip_path = fixture.path().join("example.zip");
        std::fs::write(&zip_path, package_zip()).unwrap();
        let app = test_app(
            fixture.path(),
            Arc::new(QueueDialog::with_open(Ok(Some(zip_path)))),
        );
        let webview = WebviewWindowBuilder::new(&app, "main", WebviewUrl::default())
            .build()
            .unwrap();
        let package = json!({ "id": "example-protocol", "version": "1.0.0" });

        let preview: Value = invoke_ok(&webview, "protocol_package_import", json!({}));
        assert_eq!(preview["package"], package);
        assert_eq!(preview["schema"]["fields"][0]["name"], "trace_id");
        let imported: Value = invoke_ok(
            &webview,
            "protocol_package_import_commit",
            json!({ "token": preview["token"] }),
        );
        assert_eq!(imported["outcome"], "installed");
        assert_eq!(imported["version"]["package"], package);

        let list: Value = invoke_ok(&webview, "protocol_package_list", json!({}));
        assert_eq!(list[0]["versions"][0]["package"], package);

        let detail: Value = invoke_ok(
            &webview,
            "protocol_package_detail",
            json!({ "packageRef": package }),
        );
        assert_eq!(detail["capabilities"]["upstream"]["encode"], true);
        assert_eq!(detail["capabilities"]["downstream"]["encode"], false);
        assert_eq!(detail["schema"]["fields"][0]["name"], "trace_id");
        assert_eq!(detail["schema"]["fields"][1]["name"], "amount");
        assert_no_source_fields(&detail);

        let usages: Value = invoke_ok(
            &webview,
            "protocol_package_usage",
            json!({ "packageRef": package }),
        );
        assert_eq!(usages, json!([]));
        let enabled: Value = invoke_ok(
            &webview,
            "protocol_package_enable",
            json!({ "packageRef": package }),
        );
        assert_eq!(enabled["enabled"], true);
        let disabled: Value = invoke_ok(
            &webview,
            "protocol_package_disable",
            json!({ "packageRef": package }),
        );
        assert_eq!(disabled["enabled"], false);
        let deleted: Value = invoke_ok(
            &webview,
            "protocol_package_delete",
            json!({ "packageRef": package }),
        );
        assert_eq!(deleted["success"], true);

        let error = invoke_error(
            &webview,
            "protocol_package_detail",
            json!({ "packageRef": package }),
        );
        assert_eq!(error.code, "PROTOCOL_PACKAGE_NOT_FOUND");
        assert_eq!(error.entity_id.as_deref(), Some("example-protocol@1.0.0"));
    }

    #[test]
    fn import_permission_error_is_structured_and_command_accepts_no_webview_path() {
        let fixture = TempDir::new().unwrap();
        let app = test_app(
            fixture.path(),
            Arc::new(QueueDialog::with_open(Err(AppError::new(
                "FILE_DIALOG_PERMISSION_DENIED",
                "系统拒绝文件选择。",
            )))),
        );
        let webview = WebviewWindowBuilder::new(&app, "main", WebviewUrl::default())
            .build()
            .unwrap();

        let error = invoke_error(
            &webview,
            "protocol_package_import",
            // 伪造路径不会成为命令参数；实际选择仍只能来自注入的 Rust 原生 Dialog。
            json!({ "path": "/tmp/forged.zip", "zipBytes": [1, 2, 3] }),
        );
        assert_eq!(error.code, "FILE_DIALOG_PERMISSION_DENIED");
        let list: Value = invoke_ok(&webview, "protocol_package_list", json!({}));
        assert_eq!(list, json!([]));
    }

    #[test]
    fn invalid_archive_identity_and_import_token_are_rejected_through_real_ipc() {
        let fixture = TempDir::new().unwrap();
        let invalid_zip = fixture.path().join("invalid.zip");
        std::fs::write(&invalid_zip, b"not a zip archive").unwrap();
        let app = test_app(
            fixture.path(),
            Arc::new(QueueDialog::with_open(Ok(Some(invalid_zip)))),
        );
        let webview = WebviewWindowBuilder::new(&app, "main", WebviewUrl::default())
            .build()
            .unwrap();

        assert_eq!(
            invoke_error(&webview, "protocol_package_import", json!({})).code,
            "INVALID_ZIP"
        );
        assert_eq!(
            invoke_error(
                &webview,
                "protocol_package_import_commit",
                json!({ "token": "00000000-0000-4000-8000-000000000000" }),
            )
            .code,
            "PROTOCOL_PACKAGE_IMPORT_TOKEN_INVALID"
        );
        let invalid_identity = invoke_error(
            &webview,
            "protocol_package_detail",
            json!({
                "packageRef": { "id": "../escape", "version": "latest" }
            }),
        );
        assert_eq!(invalid_identity.code, "PROTOCOL_PACKAGE_INVALID");
        assert!(!invalid_identity.field_errors.is_empty());
        let list: Value = invoke_ok(&webview, "protocol_package_list", json!({}));
        assert_eq!(list, json!([]));
    }

    #[test]
    fn forged_capability_cannot_bypass_reference_and_runtime_lifecycle_guards() {
        let fixture = TempDir::new().unwrap();
        let zip_path = fixture.path().join("example.zip");
        std::fs::write(&zip_path, package_zip()).unwrap();
        let app = test_app(
            fixture.path(),
            Arc::new(QueueDialog::with_open(Ok(Some(zip_path)))),
        );
        let webview = WebviewWindowBuilder::new(&app, "main", WebviewUrl::default())
            .build()
            .unwrap();
        let package_json = json!({ "id": "example-protocol", "version": "1.0.0" });
        let preview: Value = invoke_ok(&webview, "protocol_package_import", json!({}));
        let _: Value = invoke_ok(
            &webview,
            "protocol_package_import_commit",
            json!({ "token": preview["token"] }),
        );
        let _: Value = invoke_ok(
            &webview,
            "protocol_package_enable",
            // 未声明的 downstream encode 不是命令参数；伪造字段不能改变已编译能力。
            json!({ "packageRef": package_json, "downstreamEncode": true }),
        );
        let detail: Value = invoke_ok(
            &webview,
            "protocol_package_detail",
            json!({ "packageRef": package_json }),
        );
        assert_eq!(detail["capabilities"]["downstream"]["encode"], false);

        let package: ProtocolPackageRef = serde_json::from_value(package_json.clone()).unwrap();
        let application = Arc::clone(&app.state::<AppState>().application);
        let mut workspace = tauri::async_runtime::block_on(
            application.workspace_create("IPC lifecycle".into()),
        )
        .unwrap();
        let listener = &mut workspace.listeners[0];
        listener.port = unused_local_port();
        listener.data_plane = ListenerDataPlane::Socket(SocketRelaySettings {
            upstream: SocketEndpoint {
                host: "127.0.0.1".into(),
                port: 9_999,
            },
            processing: SocketPayloadProcessing::Scripted(ScriptedSocketProcessing {
                package,
                upstream: DirectionProcessingOptions::default(),
                downstream: DirectionProcessingOptions::default(),
            }),
            ..SocketRelaySettings::default()
        });
        let workspace =
            tauri::async_runtime::block_on(application.workspace_save(workspace)).unwrap();
        let listener_id = workspace.listeners[0].id;

        assert_eq!(
            invoke_error(
                &webview,
                "protocol_package_delete",
                json!({ "packageRef": package_json }),
            )
            .code,
            "PROTOCOL_PACKAGE_REFERENCE_IN_USE"
        );
        tauri::async_runtime::block_on(application.listener_start(
            workspace.id,
            workspace.revision.get(),
            listener_id,
        ))
        .unwrap();
        assert_eq!(
            invoke_error(
                &webview,
                "protocol_package_disable",
                json!({ "packageRef": package_json }),
            )
            .code,
            "PROTOCOL_PACKAGE_RUNTIME_IN_USE"
        );

        tauri::async_runtime::block_on(application.listener_stop(workspace.id, 0, listener_id))
            .unwrap();
        let latest = tauri::async_runtime::block_on(application.workspace_get(workspace.id)).unwrap();
        tauri::async_runtime::block_on(
            application.workspace_delete(latest.id, latest.revision.get()),
        )
        .unwrap();
        let disabled: Value = invoke_ok(
            &webview,
            "protocol_package_disable",
            json!({ "packageRef": package_json }),
        );
        assert_eq!(disabled["enabled"], false);
    }

    #[test]
    fn webview_has_no_dialog_or_filesystem_permission_for_protocol_package_import() {
        let capability: Value =
            serde_json::from_str(include_str!("../../../capabilities/default.json")).unwrap();
        let permissions = capability["permissions"].as_array().unwrap();
        assert_eq!(permissions, &[json!("core:default")]);
        assert!(permissions.iter().all(|permission| {
            permission
                .as_str()
                .is_some_and(|name| !name.starts_with("dialog:") && !name.starts_with("fs:"))
        }));
    }

    fn test_app(data_dir: &Path, dialog: Arc<dyn NativeFileDialog>) -> tauri::App<MockRuntime> {
        let host = tauri::async_runtime::block_on(
            ApplicationHostBuilder::new(
                data_dir,
                HostPlatformServices::new(Arc::new(TestSecretProtector), dialog),
                Arc::new(InterceptProxyProfile),
            )
            .build(),
        )
        .unwrap();
        tauri::test::mock_builder()
            .manage(AppState::new(host))
            .invoke_handler(tauri::generate_handler![
                protocol_package_list,
                protocol_package_detail,
                protocol_package_import,
                protocol_package_import_commit,
                protocol_package_enable,
                protocol_package_disable,
                protocol_package_delete,
                protocol_package_usage,
            ])
            .build(tauri::test::mock_context(tauri::test::noop_assets()))
            .unwrap()
    }

    fn invoke_ok<T: DeserializeOwned>(
        webview: &tauri::WebviewWindow<MockRuntime>,
        command: &str,
        body: Value,
    ) -> T {
        tauri::test::get_ipc_response(webview, request(command, body))
            .unwrap()
            .deserialize()
            .unwrap()
    }

    fn invoke_error(
        webview: &tauri::WebviewWindow<MockRuntime>,
        command: &str,
        body: Value,
    ) -> AppErrorViewModel {
        serde_json::from_value(
            tauri::test::get_ipc_response(webview, request(command, body)).unwrap_err(),
        )
        .unwrap()
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

    fn assert_no_source_fields(value: &Value) {
        const FORBIDDEN: &[&str] = &[
            "source",
            "script",
            "script_content",
            "ast",
            "absolute_path",
            "path",
            "zip",
            "zip_bytes",
            "files",
            "contents",
        ];
        match value {
            Value::Object(object) => {
                for (key, nested) in object {
                    assert!(!FORBIDDEN.contains(&key.as_str()), "forbidden key: {key}");
                    assert_no_source_fields(nested);
                }
            }
            Value::Array(items) => items.iter().for_each(assert_no_source_fields),
            _ => {}
        }
    }

    fn unused_local_port() -> u16 {
        std::net::TcpListener::bind("127.0.0.1:0")
            .unwrap()
            .local_addr()
            .unwrap()
            .port()
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
}
