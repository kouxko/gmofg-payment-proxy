//! Tauri 原生文件对话框到基础设施文件端口的适配器。
//!
//! `WebView` 只说明用途，操作系统负责路径选择和覆盖确认；用户取消不是错误，选中路径后的
//! 大小限制、解析和原子写入仍由基础设施层负责。

use std::path::PathBuf;

use intercept_proxy_application::{AppError, AppResult};
use intercept_proxy_infrastructure::{NativeFileDialog, adapters::FileSelection};
use tauri::AppHandle;
use tauri_plugin_dialog::DialogExt;

/// Desktop-native file picker used by infrastructure adapters.
///
/// The `WebView` never receives filesystem capabilities or path-selection
/// responsibility. A returned save path is treated as overwrite-confirmed
/// because the operating-system save dialog owns that confirmation.
#[derive(Debug, Clone)]
pub struct TauriNativeFileDialog {
    app: AppHandle,
}

impl TauriNativeFileDialog {
    #[must_use]
    pub fn new(app: AppHandle) -> Self {
        Self { app }
    }

    fn open_builder(&self, purpose: &str) -> tauri_plugin_dialog::FileDialogBuilder<tauri::Wry> {
        let builder = self.app.dialog().file();
        match purpose {
            "intercept_workspace" => builder
                .set_title("导入 Intercept Proxy Workspace")
                .add_filter("Intercept Workspace", &["intercept-workspace"]),
            "intercept_configuration" => builder
                .set_title("导入 Intercept Proxy 完整配置")
                .add_filter("Intercept Config", &["intercept-config"]),
            "rules_json" => builder
                .set_title("导入规则")
                .add_filter("JSON 规则", &["json"]),
            "pkcs12" => builder
                .set_title("导入上游 PKCS12")
                .add_filter("PKCS12", &["p12", "pfx"]),
            "upstream_ca" => builder
                .set_title("选择替换用上游 CA")
                .add_filter("证书", &["cer", "crt", "pem", "der"]),
            _ => builder.set_title("选择文件"),
        }
    }

    fn save_builder(&self, purpose: &str) -> tauri_plugin_dialog::FileDialogBuilder<tauri::Wry> {
        let builder = self.app.dialog().file();
        match purpose {
            "intercept_workspace" => builder
                .set_title("导出 Intercept Proxy Workspace")
                .set_file_name("workspace.intercept-workspace")
                .add_filter("Intercept Workspace", &["intercept-workspace"]),
            "intercept_configuration" => builder
                .set_title("导出 Intercept Proxy 完整配置")
                .set_file_name("intercept-proxy.intercept-config")
                .add_filter("Intercept Config", &["intercept-config"]),
            "session_json" => builder
                .set_title("导出会话")
                .set_file_name("session.json")
                .add_filter("JSON", &["json"]),
            "rules_json" => builder
                .set_title("导出规则")
                .set_file_name("rules.json")
                .add_filter("JSON", &["json"]),
            "root_ca" => builder
                .set_title("导出 Intercept Proxy Root CA 公开证书")
                .set_file_name("intercept-proxy-root-ca.crt")
                .add_filter("X.509 证书", &["crt", "cer", "pem"]),
            _ => builder.set_title("保存文件"),
        }
    }

    fn into_path(path: tauri_plugin_dialog::FilePath) -> AppResult<PathBuf> {
        path.into_path().map_err(|error| {
            AppError::new(
                "FILE_DIALOG_PATH_INVALID",
                format!("系统文件选择器返回了无效路径：{error}"),
            )
        })
    }
}

impl NativeFileDialog for TauriNativeFileDialog {
    fn choose_open_file(&self, purpose: &str) -> AppResult<Option<PathBuf>> {
        self.open_builder(purpose)
            .blocking_pick_file()
            .map(Self::into_path)
            .transpose()
    }

    fn choose_save_file(&self, purpose: &str) -> AppResult<Option<FileSelection>> {
        self.save_builder(purpose)
            .blocking_save_file()
            .map(|path| {
                Ok(FileSelection {
                    path: Self::into_path(path)?,
                    overwrite_confirmed: true,
                })
            })
            .transpose()
    }
}
