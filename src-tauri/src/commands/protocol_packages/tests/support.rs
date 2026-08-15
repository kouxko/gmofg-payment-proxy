use super::*;

pub(super) fn test_app(data_dir: &Path, dialog: Arc<dyn NativeFileDialog>) -> tauri::App<MockRuntime> {
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
            protocol_package_delete,
            protocol_package_usage,
        ])
        .build(tauri::test::mock_context(tauri::test::noop_assets()))
        .unwrap()
}

pub(super) fn invoke_ok<T: DeserializeOwned>(
    webview: &tauri::WebviewWindow<MockRuntime>,
    command: &str,
    body: Value,
) -> T {
    tauri::test::get_ipc_response(webview, request(command, body))
        .unwrap()
        .deserialize()
        .unwrap()
}

pub(super) fn invoke_error(
    webview: &tauri::WebviewWindow<MockRuntime>,
    command: &str,
    body: Value,
) -> AppErrorViewModel {
    serde_json::from_value(
        tauri::test::get_ipc_response(webview, request(command, body)).unwrap_err(),
    )
    .unwrap()
}

pub(super) fn request(command: &str, body: Value) -> tauri::webview::InvokeRequest {
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

pub(super) fn assert_no_source_fields(value: &Value) {
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

pub(super) fn unused_local_port() -> u16 {
    std::net::TcpListener::bind("127.0.0.1:0")
        .unwrap()
        .local_addr()
        .unwrap()
        .port()
}

pub(super) fn package_zip() -> Vec<u8> {
    package_zip_with_script(SCRIPT)
}

pub(super) fn package_zip_with_script(script: &str) -> Vec<u8> {
    let mut archive = ZipWriter::new(Cursor::new(Vec::new()));
    for (path, contents) in [
        ("manifest.toml", MANIFEST.as_bytes()),
        ("document.toml", SCHEMA.as_bytes()),
        ("protocol.rhai", script.as_bytes()),
    ] {
        archive
            .start_file(path, SimpleFileOptions::default())
            .unwrap();
        archive.write_all(contents).unwrap();
    }
    archive.finish().unwrap().into_inner()
}
