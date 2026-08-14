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
        protocol_package_import_discard, protocol_package_list, protocol_package_usage,
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

        fn with_opens(responses: Vec<AppResult<Option<PathBuf>>>) -> Self {
            Self {
                open: Mutex::new(responses.into()),
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
        assert_eq!(preview["disposition"], "new");
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
    fn import_disposition_discard_and_diagnostics_round_trip_through_real_tauri_ipc() {
        let fixture = TempDir::new().unwrap();
        let valid = fixture.path().join("valid.zip");
        let conflict = fixture.path().join("conflict.zip");
        let invalid = fixture.path().join("invalid.zip");
        std::fs::write(&valid, package_zip()).unwrap();
        std::fs::write(
            &conflict,
            package_zip_with_script(&format!("{SCRIPT}\n// different immutable bytes")),
        )
        .unwrap();
        std::fs::write(
            &invalid,
            package_zip_with_script("fn frame(reader, context) {\n  let card = ;\n}"),
        )
        .unwrap();
        let app = test_app(
            fixture.path(),
            Arc::new(QueueDialog::with_opens(vec![
                Ok(Some(valid.clone())),
                Ok(Some(valid)),
                Ok(Some(conflict)),
                Ok(Some(invalid.clone())),
            ])),
        );
        let webview = WebviewWindowBuilder::new(&app, "main", WebviewUrl::default())
            .build()
            .unwrap();

        let discarded_preview: Value = invoke_ok(&webview, "protocol_package_import", json!({}));
        let discarded_token = discarded_preview["token"].clone();
        let discarded: Value = invoke_ok(
            &webview,
            "protocol_package_import_discard",
            json!({ "token": discarded_token.clone() }),
        );
        assert_eq!(discarded["success"], true);
        assert_eq!(
            invoke_error(
                &webview,
                "protocol_package_import_commit",
                json!({ "token": discarded_token }),
            )
            .code,
            "PROTOCOL_PACKAGE_IMPORT_TOKEN_INVALID"
        );

        let ready: Value = invoke_ok(&webview, "protocol_package_import", json!({}));
        let _: Value = invoke_ok(
            &webview,
            "protocol_package_import_commit",
            json!({ "token": ready["token"] }),
        );
        let conflict_preview: Value =
            invoke_ok(&webview, "protocol_package_import", json!({}));
        assert_eq!(conflict_preview["disposition"], "identity_conflict");
        assert_eq!(conflict_preview["token"], Value::Null);

        let error = invoke_error(&webview, "protocol_package_import", json!({}));
        let diagnostic = error.diagnostic.expect("safe compiler diagnostic");
        assert_eq!(diagnostic.file.as_deref(), Some("protocol.rhai"));
        assert_eq!(diagnostic.line, Some(2));
        let serialized = serde_json::to_string(&diagnostic).unwrap();
        assert!(!serialized.contains(invalid.to_string_lossy().as_ref()));
        assert!(!serialized.contains("let card"));
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

    #[path = "support.rs"]
    mod support;
    use support::*;
}
