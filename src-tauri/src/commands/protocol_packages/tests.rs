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
        FileSelection, InfrastructureError, NativeFileDialog, SecretProtector,
    };
    use intercept_proxy_product_api::InterceptProxyProfile;
    use serde::de::DeserializeOwned;
    use serde_json::{Value, json};
    use tauri::{
        WebviewUrl, WebviewWindowBuilder, http::HeaderMap, ipc::InvokeBody, test::MockRuntime,
    };
    use tempfile::TempDir;
    use zip::{ZipWriter, write::SimpleFileOptions};

    use super::{
        listener_protocol_package_catalog, protocol_package_delete, protocol_package_detail,
        protocol_package_disable, protocol_package_enable, protocol_package_import,
        protocol_package_import_commit, protocol_package_import_discard, protocol_package_list,
        protocol_package_restart, protocol_package_usage,
    };
    use crate::app_state::AppState;

    const MANIFEST: &str = include_str!(
        "../../../../test-support/fixtures/task-20260829-002/phase-4/package-contract/http-manifest.json"
    );

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
    fn strict_import_contract_fails_closed_through_real_tauri_ipc() {
        let fixture = TempDir::new().unwrap();
        let invalid = fixture.path().join("invalid.zip");
        let javascript = fixture.path().join("javascript.zip");
        std::fs::write(&invalid, b"not a zip archive").unwrap();
        std::fs::write(&javascript, javascript_package_zip()).unwrap();
        let app = test_app(
            fixture.path(),
            Arc::new(QueueDialog::with_opens(vec![
                Ok(Some(invalid)),
                Ok(Some(javascript)),
            ])),
        );
        let webview = WebviewWindowBuilder::new(&app, "main", WebviewUrl::default())
            .build()
            .unwrap();

        let invalid_archive = invoke_error(&webview, "protocol_package_import", json!({}));
        assert_eq!(invalid_archive.code, "PROTOCOL_PACKAGE_INVALID");
        assert!(!invalid_archive.field_errors.contains_key("runtime"));

        let preview: Value = invoke_ok(&webview, "protocol_package_import", json!({}));
        assert_eq!(preview["disposition"], "new");
        assert!(preview["token"].is_string());
        let committed: Value = invoke_ok(
            &webview,
            "protocol_package_import_commit",
            json!({ "token": preview["token"] }),
        );
        assert_eq!(committed["outcome"], "installed");
        assert_eq!(committed["version"]["enabled"], true);
        assert_eq!(committed["version"]["package_source"]["online"], false);
        let package_ref = committed["version"]["package"].clone();
        let disabled: Value = invoke_ok(
            &webview,
            "protocol_package_disable",
            json!({ "packageRef": package_ref.clone() }),
        );
        assert_eq!(disabled["enabled"], false);
        assert_eq!(
            invoke_error(
                &webview,
                "protocol_package_restart",
                json!({ "packageRef": package_ref }),
            )
            .code,
            "PROTOCOL_PACKAGE_DISABLED"
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
            json!({ "packageRef": { "id": "../escape", "version": "latest" } }),
        );
        assert_eq!(invalid_identity.code, "PROTOCOL_PACKAGE_INVALID");
        assert!(!invalid_identity.field_errors.is_empty());
        let list: Value = invoke_ok(&webview, "protocol_package_list", json!({}));
        assert_eq!(list.as_array().unwrap().len(), 1);
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
            json!({ "path": "/tmp/forged.zip", "zipBytes": [1, 2, 3] }),
        );
        assert_eq!(error.code, "FILE_DIALOG_PERMISSION_DENIED");
        let list: Value = invoke_ok(&webview, "protocol_package_list", json!({}));
        assert_eq!(list, json!([]));
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
                listener_protocol_package_catalog,
                protocol_package_detail,
                protocol_package_import,
                protocol_package_import_commit,
                protocol_package_import_discard,
                protocol_package_enable,
                protocol_package_disable,
                protocol_package_restart,
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

    fn javascript_package_zip() -> Vec<u8> {
        let mut archive = ZipWriter::new(Cursor::new(Vec::new()));
        for (path, contents) in [
            ("manifest.json", MANIFEST.as_bytes()),
            ("protocol.js", b"export {}".as_slice()),
            ("display.js", b"export {}".as_slice()),
        ] {
            archive
                .start_file(path, SimpleFileOptions::default())
                .unwrap();
            archive.write_all(contents).unwrap();
        }
        archive.finish().unwrap().into_inner()
    }
}
