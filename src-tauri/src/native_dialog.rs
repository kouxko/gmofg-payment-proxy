use std::path::PathBuf;

use gmofg_proxy_application::{AppError, AppResult};
use gmofg_proxy_infrastructure::{NativeFileDialog, adapters::FileSelection};
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
            "rules_json" => builder
                .set_title("导入规则")
                .add_filter("JSON 规则", &["json"]),
            "pkcs12" => builder
                .set_title("导入上游 PKCS12")
                .add_filter("PKCS12", &["p12", "pfx"]),
            "upstream_ca" => builder
                .set_title("导入上游 CA")
                .add_filter("证书", &["cer", "crt", "pem", "der"]),
            _ => builder.set_title("选择文件"),
        }
    }

    fn save_builder(&self, purpose: &str) -> tauri_plugin_dialog::FileDialogBuilder<tauri::Wry> {
        let builder = self.app.dialog().file();
        match purpose {
            "session_json" => builder
                .set_title("导出会话")
                .set_file_name("session.json")
                .add_filter("JSON", &["json"]),
            "rules_json" => builder
                .set_title("导出规则")
                .set_file_name("rules.json")
                .add_filter("JSON", &["json"]),
            "root_ca" => builder
                .set_title("导出 Root CA")
                .set_file_name("gmofg-proxy-root-ca.cer")
                .add_filter("证书", &["cer"]),
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
