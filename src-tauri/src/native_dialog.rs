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
            "server_identity_pem" => builder
                .set_title("导入本监听服务端身份（证书链 + 私钥）")
                .add_filter("PEM 服务端身份", &["pem"]),
            "downstream_client_ca" => builder
                .set_title("导入用于验证客户端证书的 CA")
                .add_filter("客户端证书 CA", &["cer", "crt", "pem", "der"]),
            "upstream_ca" => builder
                .set_title("选择替换用上游 CA")
                .add_filter("证书", &["cer", "crt", "pem", "der"]),
            _ => builder.set_title("选择文件"),
        }
    }

    fn save_builder(
        &self,
        purpose: &str,
        suggested_file_name: &str,
    ) -> tauri_plugin_dialog::FileDialogBuilder<tauri::Wry> {
        let builder = self.app.dialog().file();
        let builder = match purpose {
            "intercept_workspace" => builder
                .set_title("导出 Intercept Proxy Workspace")
                .add_filter("Intercept Workspace", &["intercept-workspace"]),
            "intercept_configuration" => builder
                .set_title("导出 Intercept Proxy 完整配置")
                .add_filter("Intercept Config", &["intercept-config"]),
            "session_json" => builder.set_title("导出会话").add_filter("JSON", &["json"]),
            "rules_json" => builder.set_title("导出规则").add_filter("JSON", &["json"]),
            "root_ca" => builder
                .set_title("导出 Intercept Proxy Root CA 公开证书")
                .add_filter("X.509 证书", &["crt", "cer", "pem"]),
            _ => builder.set_title("保存文件"),
        };
        builder.set_file_name(safe_suggested_file_name(purpose, suggested_file_name))
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

    fn choose_save_file(
        &self,
        purpose: &str,
        suggested_file_name: &str,
    ) -> AppResult<Option<FileSelection>> {
        self.save_builder(purpose, suggested_file_name)
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

fn safe_suggested_file_name<'a>(purpose: &str, suggested_file_name: &'a str) -> &'a str {
    let valid = !suggested_file_name.is_empty()
        && suggested_file_name != "."
        && suggested_file_name != ".."
        && !suggested_file_name
            .chars()
            .any(|character| character.is_control() || matches!(character, '/' | '\\'));
    if valid {
        suggested_file_name
    } else {
        default_file_name(purpose)
    }
}

fn default_file_name(purpose: &str) -> &'static str {
    match purpose {
        "intercept_workspace" => "workspace.intercept-workspace",
        "intercept_configuration" => "intercept-proxy.intercept-config",
        "session_json" => "session.json",
        "rules_json" => "rules.json",
        "root_ca" => "intercept-proxy-root-ca.crt",
        _ => "export",
    }
}

#[cfg(test)]
mod tests {
    use super::safe_suggested_file_name;

    #[test]
    fn save_dialog_uses_safe_suggestion_and_rejects_path_components() {
        assert_eq!(
            safe_suggested_file_name("intercept_workspace", "Lab_Updated.intercept-workspace"),
            "Lab_Updated.intercept-workspace"
        );
        assert_eq!(
            safe_suggested_file_name("intercept_workspace", "../escaped.intercept-workspace"),
            "workspace.intercept-workspace"
        );
        assert_eq!(
            safe_suggested_file_name("intercept_workspace", "..\\escaped.intercept-workspace"),
            "workspace.intercept-workspace"
        );
    }
}
