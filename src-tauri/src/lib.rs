mod app_state;
mod commands;
mod native_dialog;

use std::{error::Error, path::PathBuf, sync::Arc};

use gmofg_proxy_host::{ApplicationHostBuilder, HostPlatformServices};
use gmofg_proxy_product_api::ProductProfile;
use gmofg_proxy_product_payment::PaymentProductProfile;
use specta_typescript::Typescript;
use tauri::Manager;

use crate::{app_state::AppState, native_dialog::TauriNativeFileDialog};

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
    let product = Arc::new(PaymentProductProfile::isolated_test_tool());
    let host = tauri::async_runtime::block_on(
        ApplicationHostBuilder::new(
            app_data_dir,
            HostPlatformServices::production(dialog, product.storage()),
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
            let state = app_handle.state::<AppState>();
            if state.begin_shutdown() {
                api.prevent_exit();
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
                    app_handle.exit(code.unwrap_or(0));
                });
            }
        }
    });
}
