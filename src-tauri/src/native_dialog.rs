//! Tauri 原生文件对话框到基础设施文件端口的适配器。
//!
//! `WebView` 只说明用途，操作系统负责路径选择和覆盖确认；用户取消不是错误，选中路径后的
//! 大小限制、解析和原子写入仍由基础设施层负责。

use std::path::PathBuf;

use intercept_proxy_application::{AppError, AppResult};
use intercept_proxy_infrastructure::{FileSelection, NativeFileDialog};
use tauri::AppHandle;
use tauri_plugin_dialog::DialogExt;

const PKCS12_EXTENSIONS: &[&str] = &["p12", "pfx"];
const CLIENT_IDENTITY_EXTENSIONS: &[&str] = &["p12", "pfx", "pem"];
const SERVER_IDENTITY_EXTENSIONS: &[&str] = &["p12", "pfx", "pem"];
const TRUST_CERTIFICATE_EXTENSIONS: &[&str] = &["cer", "crt", "pem", "der"];
const PROTOCOL_PACKAGE_EXTENSIONS: &[&str] = &["zip"];
const APPLICATION_BACKUP_EXTENSIONS: &[&str] = &["zip"];
const MARKDOWN_EXTENSIONS: &[&str] = &["md"];

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
            "protocol_package_zip" => builder
                .set_title("导入 Socket 协议包")
                .add_filter("协议包 ZIP", PROTOCOL_PACKAGE_EXTENSIONS),
            "application_backup_zip" => builder
                .set_title("导入 Intercept Proxy 应用备份")
                .add_filter("应用备份 ZIP", APPLICATION_BACKUP_EXTENSIONS),
            "pkcs12" => builder
                .set_title("导入上游 PKCS12")
                .add_filter("PKCS12", PKCS12_EXTENSIONS),
            "upstream_client_identity" => builder
                .set_title("导入上游 mTLS 客户端身份")
                .add_filter("客户端身份", CLIENT_IDENTITY_EXTENSIONS),
            "server_identity_pem" => builder
                .set_title("导入本监听服务端身份（证书链 + 私钥）")
                .add_filter("服务端身份", SERVER_IDENTITY_EXTENSIONS),
            "downstream_client_ca" => builder
                .set_title("导入用于验证客户端证书的 CA")
                .add_filter("客户端证书 CA", TRUST_CERTIFICATE_EXTENSIONS),
            "upstream_ca" => builder
                .set_title("选择替换用上游 CA")
                .add_filter("证书", TRUST_CERTIFICATE_EXTENSIONS),
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
            "rules_json" => builder.set_title("导出规则").add_filter("JSON", &["json"]),
            "root_ca" => builder
                .set_title("导出 Intercept Proxy Root CA 公开证书")
                .add_filter("X.509 证书", &["crt", "cer", "pem"]),
            "application_backup_zip" => builder
                .set_title("导出 Intercept Proxy 应用备份")
                .add_filter("应用备份 ZIP", APPLICATION_BACKUP_EXTENSIONS),
            "protocol_package_export_zip" => builder
                .set_title("导出 ISO 8583 协议包模板")
                .add_filter("协议包 ZIP", PROTOCOL_PACKAGE_EXTENSIONS),
            "diagnostic_reproduction_markdown" => builder
                .set_title("导出故障复现报告")
                .add_filter("Markdown", MARKDOWN_EXTENSIONS),
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
        "rules_json" => "rules.json",
        "root_ca" => "intercept-proxy-root-ca.crt",
        "application_backup_zip" => "intercept-proxy-backup.zip",
        "protocol_package_export_zip" => "iso8583-ascii-standard-1.0.0.zip",
        "diagnostic_reproduction_markdown" => "intercept-proxy-reproduction.md",
        _ => "export",
    }
}

#[cfg(test)]
mod tests {
    use super::{
        APPLICATION_BACKUP_EXTENSIONS, PKCS12_EXTENSIONS, PROTOCOL_PACKAGE_EXTENSIONS,
        SERVER_IDENTITY_EXTENSIONS, TRUST_CERTIFICATE_EXTENSIONS,
    };

    #[test]
    fn listener_certificate_picker_formats_match_supported_content_types() {
        assert_eq!(PKCS12_EXTENSIONS, ["p12", "pfx"]);
        assert_eq!(SERVER_IDENTITY_EXTENSIONS, ["p12", "pfx", "pem"]);
        assert_eq!(TRUST_CERTIFICATE_EXTENSIONS, ["cer", "crt", "pem", "der"]);
        assert_eq!(PROTOCOL_PACKAGE_EXTENSIONS, ["zip"]);
        assert_eq!(APPLICATION_BACKUP_EXTENSIONS, ["zip"]);
    }
}
