//! 统一诊断日志命令；日志来源、脱敏、筛选和容量均由 Rust 应用层控制。

use intercept_proxy_application::{DiagnosticLogPageViewModel, DiagnosticLogQuery};
use tauri::State;

use crate::app_state::AppState;

#[tauri::command]
#[specta::specta]
pub async fn diagnostic_log_query(
    state: State<'_, AppState>,
    query: DiagnosticLogQuery,
) -> Result<DiagnosticLogPageViewModel, intercept_proxy_application::AppErrorViewModel> {
    Ok(state.application.diagnostic_log_query(&query))
}
