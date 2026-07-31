//! 桌面进程的 Tauri 组合根。
//!
//! 这里创建唯一 `AppState`、注册 Command/插件并协调退出。窗口关闭不会直接杀进程：首个
//! 退出请求启动异步优雅关闭，后续请求只等待同一流程，完成后才由应用显式退出。

mod app_state;
mod commands;
mod native_dialog;

use std::{error::Error, path::PathBuf, sync::Arc};

use gmofg_proxy_host::{ApplicationHostBuilder, HostPlatformServices};
use gmofg_proxy_product_payment::PaymentProductProfile;
use specta_typescript::Typescript;
use tauri::Manager;

use crate::{app_state::AppState, native_dialog::TauriNativeFileDialog};

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
    Ok(path)
}

fn initialize_application(app: &tauri::App) -> Result<AppState, Box<dyn Error>> {
    let app_data_dir = app.path().app_data_dir()?;
    let dialog = Arc::new(TauriNativeFileDialog::new(app.handle().clone()));
    // This executable is the isolated diagnostic proxy defined by CERT-005,
    // not a production payment client. It intentionally embeds the TEST ONLY
    // authority so Rust can issue LAN-matching leaf certificates; only the
    // public Root CA is exportable. Other hosts should use the fail-closed
    // `PaymentProductProfile::default()` unless they make the same test-only
    // packaging decision explicitly.
    let product = Arc::new(PaymentProductProfile::isolated_test_tool());
    let host = tauri::async_runtime::block_on(
        ApplicationHostBuilder::new(
            app_data_dir,
            HostPlatformServices::production(dialog),
            product,
        )
        .build(),
    )?;
    Ok(AppState::new(host))
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
                let app_handle = app_handle.clone();
                tauri::async_runtime::spawn(async move {
                    if let Err(error) = host.shutdown().await {
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
    use super::{ExitRequestPlan, plan_exit_request};

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
