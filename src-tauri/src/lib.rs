#![recursion_limit = "512"]

//! 桌面进程的 Tauri 组合根。
//!
//! 这里创建唯一 `AppState`、注册 Command/插件并协调退出。窗口关闭不会直接杀进程：首个
//! 退出请求启动异步优雅关闭，后续请求只等待同一流程，完成后才由应用显式退出。

mod app_state;
mod commands;
mod mcp;
mod native_dialog;
mod reproduction_report;
mod runtime_logs;

use std::{error::Error, path::PathBuf, sync::Arc};

use intercept_proxy_application::ExchangeObservationQueries;
#[cfg(debug_assertions)]
use intercept_proxy_host::DatabaseStartupPolicy;
use intercept_proxy_host::{ApplicationHostBuilder, HostPlatformServices};
use intercept_proxy_infrastructure::ExchangeObservationStore;
use intercept_proxy_product_api::InterceptProxyProfile;
use specta_typescript::Typescript;
use tauri::{Manager, path::BaseDirectory};

use crate::{
    app_state::AppState,
    mcp::{ApplicationBackend, MCP_BIND_ENDPOINT, McpServer},
    native_dialog::TauriNativeFileDialog,
    runtime_logs::{ApplicationLogLevel, RuntimeLogStore, TracingBridge, install_tracing_bridge},
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
    let mut lines = generated.lines().map(str::trim_end).collect::<Vec<_>>();
    while lines.last().is_some_and(|line| line.is_empty()) {
        lines.pop();
    }
    let mut normalized = lines.join("\n");
    normalized.push('\n');
    normalized
}

fn initialize_application(
    app: &tauri::App,
    runtime_logs: Arc<RuntimeLogStore>,
) -> Result<(AppState, TracingBridge), Box<dyn Error>> {
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
    #[cfg(debug_assertions)]
    {
        // TASK_20260829_002_PRE_RELEASE_DATABASE_RESET
        host_builder =
            host_builder.with_database_startup_policy(DatabaseStartupPolicy::RecreateCurrent);
    }
    #[cfg(not(debug_assertions))]
    {
        host_builder = host_builder
            .with_database_startup_policy(intercept_proxy_host::DatabaseStartupPolicy::Preserve);
    }
    let host = tauri::async_runtime::block_on(host_builder.build())?;
    let observation_queue_capacity =
        tauri::async_runtime::block_on(host.application().settings_get())?
            .stored
            .ui_event_capacity;
    // Runtime logs and Exchange observation share at most one quarter of the configured process
    // memory budget while waiting in their non-blocking queues. Retained UI evidence is accounted
    // separately by CapacityLedger after the consumer accepts it.
    let observation_queue_bytes = usize::try_from(host.capacity().max_bytes().saturating_div(4))
        .unwrap_or(usize::MAX)
        .max(1);
    let exchange_observations = Arc::new(ExchangeObservationStore::new(host.capacity()));
    let tracing_bridge = install_tracing_bridge(
        Arc::clone(&runtime_logs),
        Arc::clone(&exchange_observations),
        host.events(),
        observation_queue_capacity,
        observation_queue_bytes,
    )?;
    let backend = Arc::new(ApplicationBackend::new(
        host.application(),
        Arc::clone(&runtime_logs),
        ExchangeObservationQueries::new(exchange_observations.clone()),
    ));
    // IPv4 all-interface MCP is part of the accepted product boundary. Its bind
    // failure is fatal; only IPv6 may degrade according to the server capability table.
    let mcp = tauri::async_runtime::block_on(McpServer::start(backend))?;
    tracing::info!(endpoint = MCP_BIND_ENDPOINT, address = %mcp.local_addr(), "MCP server started");
    Ok((
        AppState::production(host, Some(mcp), runtime_logs, exchange_observations),
        tracing_bridge,
    ))
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
            const LOG_CAPACITY: usize = 20_000;
            const LOG_FILE_BYTES: u64 = 32 * 1024 * 1024;
            let app_data_dir = app.path().app_data_dir()?;
            let runtime_logs = Arc::new(RuntimeLogStore::open(
                app_data_dir
                    .join("runtime-logs")
                    .join("application-runtime.jsonl"),
                LOG_CAPACITY,
                LOG_FILE_BYTES,
            )?);
            let dispatch_logs = Arc::clone(&runtime_logs);
            let dispatch = tauri_plugin_log::fern::Dispatch::new().chain(
                tauri_plugin_log::fern::Output::call(move |record| {
                    let level = match record.level() {
                        log::Level::Trace => ApplicationLogLevel::Trace,
                        log::Level::Debug => ApplicationLogLevel::Debug,
                        log::Level::Info => ApplicationLogLevel::Info,
                        log::Level::Warn => ApplicationLogLevel::Warning,
                        log::Level::Error => ApplicationLogLevel::Error,
                    };
                    dispatch_logs.record(level, record.target(), &record.args().to_string());
                }),
            );
            app.handle().plugin(
                tauri_plugin_log::Builder::default()
                    .level(log::LevelFilter::Debug)
                    .filter(|metadata| {
                        metadata.level() <= log::Level::Info
                            || metadata.target().starts_with("intercept_proxy")
                    })
                    .max_file_size(8 * 1024 * 1024)
                    .rotation_strategy(tauri_plugin_log::RotationStrategy::KeepSome(8))
                    .target(tauri_plugin_log::Target::new(
                        tauri_plugin_log::TargetKind::Dispatch(dispatch),
                    ))
                    .build(),
            )?;
            let (state, tracing_bridge) = initialize_application(app, runtime_logs)?;
            app.manage(tracing_bridge);
            app.manage(state);
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
                    if let Err(error) = app_handle.state::<TracingBridge>().shutdown() {
                        // tracing consumer 已关闭或 join 失败时不能再递归写 tracing；stderr
                        // 是进程退出阶段唯一不会重新进入该桥接器的诊断出口。
                        eprintln!("tracing bridge shutdown failed: {error}");
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
    fn generated_typescript_normalization_removes_line_end_whitespace_and_blank_eof() {
        assert_eq!(
            normalize_generated_typescript("export type A = \n\tstring;  \n\n"),
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
