//! 桌面进程的 Tauri 组合根。
//!
//! 这里创建唯一 `AppState`、注册 Command/插件并协调退出。窗口关闭不会直接杀进程：首个
//! 退出请求启动异步优雅关闭，后续请求只等待同一流程，完成后才由应用显式退出。

mod app_state;
mod commands;
mod mcp;
mod native_dialog;

use std::{error::Error, path::PathBuf, sync::Arc};

use intercept_proxy_host::{ApplicationHostBuilder, HostPlatformServices};
use intercept_proxy_product_api::InterceptProxyProfile;
use specta_typescript::Typescript;
use tauri::{Manager, path::BaseDirectory};

use crate::{
    app_state::AppState,
    mcp::{ApplicationBackend, MCP_ENDPOINT, ReadOnlyMcpServer},
    native_dialog::TauriNativeFileDialog,
};

const BUILTIN_ISO8583_ARCHIVE: &[u8] = include_bytes!(concat!(
    env!("OUT_DIR"),
    "/iso8583-ascii-standard-1.0.0.zip"
));

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct ExitRequestPlan {
    prevent_exit: bool,
    start_shutdown: bool,
    exit_code: i32,
}

/// Converts one Tauri exit request into a deterministic shutdown decision.
///
/// Every request must be prevented until the single graceful-shutdown owner
/// explicitly exits the application. Repeated window-close or OS exit events
/// therefore wait for the existing shutdown task instead of bypassing it.
fn plan_exit_request(
    shutdown_completed: bool,
    start_shutdown: bool,
    requested_code: Option<i32>,
) -> ExitRequestPlan {
    ExitRequestPlan {
        prevent_exit: !shutdown_completed,
        start_shutdown: !shutdown_completed && start_shutdown,
        exit_code: requested_code.unwrap_or(0),
    }
}

pub fn export_bindings() -> Result<PathBuf, String> {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../src/generated/rust-types.ts");
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|error| error.to_string())?;
    }
    commands::builder()
        .export(Typescript::default(), &path)
        .map_err(|error| error.to_string())?;
    // Specta 会把带成员文档的 tagged union 排成多行，并在联合分隔符或空文档行后
    // 留下空格。生成文件属于可重复构建产物：统一移除行尾空白，避免每次新增强类型
    // DTO 都要删掉 Rust 文档，也保证 `pnpm bindings` 后 `git diff --check` 恒定通过。
    let generated = std::fs::read_to_string(&path).map_err(|error| error.to_string())?;
    let normalized = normalize_generated_typescript(&generated);
    std::fs::write(&path, normalized).map_err(|error| error.to_string())?;
    Ok(path)
}

fn normalize_generated_typescript(generated: &str) -> String {
    let mut normalized = generated
        .lines()
        .map(str::trim_end)
        .collect::<Vec<_>>()
        .join("\n");
    if generated.ends_with('\n') {
        normalized.push('\n');
    }
    normalized
}

fn initialize_application(app: &tauri::App) -> Result<AppState, Box<dyn Error>> {
    let app_data_dir = app.path().app_data_dir()?;
    // 资源在不同平台的安装目录不同。必须由 Tauri 的路径解析器给出真实绝对路径，
    // 不能从当前 exe 所在目录猜测，否则 Windows 安装版和便携版容易出现差异。
    let companion_apk = app
        .path()
        .resolve("resources/android-companion.apk", BaseDirectory::Resource)
        .ok()
        .filter(|path| path.is_file());
    let dialog = Arc::new(TauriNativeFileDialog::new(app.handle().clone()));
    // 新应用使用纯通用配置：不读取旧数据库、不加载业务模板，也不携带业务 CA、
    // 上游地址或客户端身份。包内固定测试 Root 的运行时副本由基础设施层交给系统密钥保护。
    let product = Arc::new(InterceptProxyProfile);
    let mut host_builder = ApplicationHostBuilder::new(
        app_data_dir,
        HostPlatformServices::production(dialog),
        product,
    );
    if let Some(companion_apk) = companion_apk {
        host_builder = host_builder.with_android_companion_apk(companion_apk);
    }
    host_builder = host_builder.with_builtin_protocol_package(Arc::from(BUILTIN_ISO8583_ARCHIVE));
    let host = tauri::async_runtime::block_on(host_builder.build())?;
    let backend = Arc::new(ApplicationBackend::new(host.application()));
    let mcp = match tauri::async_runtime::block_on(ReadOnlyMcpServer::start(backend)) {
        Ok(mcp) => {
            tracing::info!(endpoint = MCP_ENDPOINT, address = %mcp.local_addr(), "read-only MCP server started");
            Some(mcp)
        }
        Err(error) => {
            tracing::warn!(endpoint = MCP_ENDPOINT, %error, "read-only MCP unavailable; proxy startup continues");
            None
        }
    };
    Ok(AppState::production(host, mcp))
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    let command_builder = commands::builder();
    #[cfg(debug_assertions)]
    export_bindings().expect("failed to export Rust TypeScript bindings");

    let app = tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .invoke_handler(command_builder.invoke_handler())
        .setup(|app| {
            if cfg!(debug_assertions) {
                app.handle().plugin(
                    tauri_plugin_log::Builder::default()
                        .level(log::LevelFilter::Info)
                        .build(),
                )?;
            }
            app.manage(initialize_application(app)?);
            Ok(())
        })
        .build(tauri::generate_context!())
        .expect("error while building tauri application");
    app.run(|app_handle, event| {
        if let tauri::RunEvent::ExitRequested { api, code, .. } = event {
            // Tauri 可能因窗口、菜单或系统关机重复发出退出事件。所有事件先 prevent_exit，
            // 只有抢到 begin_shutdown 门闩的调用者启动异步清理；清理完成后显式 exit。
            let state = app_handle.state::<AppState>();
            let shutdown_completed = state.shutdown_completed();
            let start_shutdown = !shutdown_completed && state.begin_shutdown();
            let plan = plan_exit_request(shutdown_completed, start_shutdown, code);
            if plan.prevent_exit {
                api.prevent_exit();
            }
            if plan.start_shutdown {
                let host = state.host();
                let mcp = state.mcp();
                let app_handle = app_handle.clone();
                tauri::async_runtime::spawn(async move {
                    let host_result = if let Some(mcp) = mcp {
                        let (host_result, ()) = tokio::join!(host.shutdown(), mcp.shutdown());
                        host_result
                    } else {
                        host.shutdown().await
                    };
                    if let Err(error) = host_result {
                        tracing::error!(
                            code = %error.view_model.code,
                            message = %error.view_model.message,
                            "graceful application shutdown failed"
                        );
                    }
                    app_handle.exit(plan.exit_code);
                });
            }
        }
    });
}

#[cfg(test)]
mod tests {
    use super::{ExitRequestPlan, normalize_generated_typescript, plan_exit_request};

    #[test]
    fn generated_typescript_normalization_removes_only_line_end_whitespace() {
        assert_eq!(
            normalize_generated_typescript("export type A = \n\tstring;  \n"),
            "export type A =\n\tstring;\n"
        );
    }

    #[test]
    fn repeated_exit_requests_are_prevented_but_start_shutdown_once() {
        let first = plan_exit_request(false, true, Some(7));
        let repeated = plan_exit_request(false, false, Some(9));

        assert_eq!(
            first,
            ExitRequestPlan {
                prevent_exit: true,
                start_shutdown: true,
                exit_code: 7,
            }
        );
        assert_eq!(
            repeated,
            ExitRequestPlan {
                prevent_exit: true,
                start_shutdown: false,
                exit_code: 9,
            }
        );
    }

    #[test]
    fn missing_exit_code_defaults_to_success_after_shutdown() {
        assert_eq!(
            plan_exit_request(false, true, None),
            ExitRequestPlan {
                prevent_exit: true,
                start_shutdown: true,
                exit_code: 0,
            }
        );
    }

    #[test]
    fn completed_shutdown_allows_the_explicit_exit_request() {
        assert_eq!(
            plan_exit_request(true, false, Some(7)),
            ExitRequestPlan {
                prevent_exit: false,
                start_shutdown: false,
                exit_code: 7,
            }
        );
    }
}
