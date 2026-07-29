mod app_state;
mod commands;
mod native_dialog;

use std::{error::Error, path::PathBuf, sync::Arc};

use gmofg_proxy_application::{
    Application, BreakpointCoordinator, BreakpointValidator, EventHub, SettingsRepositoryPort,
};
#[cfg(not(target_os = "macos"))]
use gmofg_proxy_infrastructure::DpapiProtector;
#[cfg(target_os = "macos")]
use gmofg_proxy_infrastructure::MacKeychainProtector;
use gmofg_proxy_infrastructure::{
    InfrastructureServiceBundle, RuntimePipelineAdapter, SecretProtector, SqliteStore,
};
use gmofg_proxy_runtime::{
    ApplicationProxyAdapter, ProxySupervisor, RustlsRuntimeServiceFactory, SystemClock,
    TokioListenerBinder,
};
use specta_typescript::Typescript;
use tauri::Manager;
use tokio_util::sync::CancellationToken;

use crate::{app_state::AppState, native_dialog::TauriNativeFileDialog};

fn platform_secret_protector() -> Arc<dyn SecretProtector> {
    #[cfg(windows)]
    {
        Arc::new(DpapiProtector)
    }
    #[cfg(target_os = "macos")]
    {
        Arc::new(MacKeychainProtector::default())
    }
    #[cfg(not(any(windows, target_os = "macos")))]
    {
        Arc::new(DpapiProtector)
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
    std::fs::create_dir_all(&app_data_dir)?;
    let store = Arc::new(SqliteStore::open(
        &app_data_dir.join("gmofg-payment-proxy.sqlite3"),
    )?);
    let dialog = Arc::new(TauriNativeFileDialog::new(app.handle().clone()));
    let services = InfrastructureServiceBundle::new(store, platform_secret_protector(), dialog);
    let settings = tauri::async_runtime::block_on(services.settings.get())?;
    services.sessions.set_limits(
        settings.stored.max_sessions,
        settings.stored.max_memory_bytes,
    )?;

    let breakpoints = Arc::new(BreakpointCoordinator::default());
    let events = Arc::new(EventHub::new(EventHub::DEFAULT_CAPACITY));
    let pipeline = Arc::new(RuntimePipelineAdapter::new(
        services.rules.clone(),
        services.sessions.clone(),
        breakpoints.clone(),
        events.clone(),
        services.capture.clone(),
    ));
    let service_factory = Arc::new(RustlsRuntimeServiceFactory::new(
        services.certificates.clone(),
        pipeline.clone(),
        Arc::new(SystemClock),
    ));
    let supervisor = Arc::new(ProxySupervisor::with_factory(
        Arc::new(TokioListenerBinder),
        service_factory,
    ));
    let proxy = Arc::new(ApplicationProxyAdapter::new(
        supervisor,
        settings.stored,
        pipeline,
    ));
    let application = Arc::new(Application::new(
        proxy,
        services.capture,
        services.sessions,
        breakpoints,
        Arc::new(BreakpointValidator),
        services.rules,
        services.faults,
        services.certificates,
        services.settings,
        services.file_export,
        events.clone(),
    ));

    let shutdown = CancellationToken::new();
    let event_shutdown = shutdown.clone();
    tauri::async_runtime::spawn(async move {
        let task = events.spawn_capture_flush_task(event_shutdown);
        if let Err(error) = task.await {
            tracing::error!(?error, "capture event flush task failed");
        }
    });
    Ok(AppState::new(application, shutdown))
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
                let application = state.application.clone();
                let shutdown = state.shutdown_token();
                let app_handle = app_handle.clone();
                tauri::async_runtime::spawn(async move {
                    if let Err(error) = application.app_shutdown().await {
                        tracing::error!(
                            code = %error.view_model.code,
                            message = %error.view_model.message,
                            "graceful application shutdown failed"
                        );
                    }
                    shutdown.cancel();
                    app_handle.exit(code.unwrap_or(0));
                });
            }
        }
    });
}
