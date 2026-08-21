//! 统一诊断日志与故障复现报告命令。

use intercept_proxy_application::{
    AppError, DiagnosticLogPageViewModel, DiagnosticLogQuery, DiagnosticReportQuery,
};
use intercept_proxy_infrastructure::AtomicFileExporter;
use serde::{Deserialize, Serialize};
use specta::Type;
use tauri::State;

use super::{CommandResult, command_error};
use crate::{app_state::AppState, reproduction_report};

const REPRODUCTION_REPORT_DIALOG_PURPOSE: &str = "diagnostic_reproduction_markdown";
const REPRODUCTION_REPORT_FILE_NAME: &str = "intercept-proxy-reproduction.md";

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
pub struct DiagnosticReportExportOutcome {
    pub bytes_written: u64,
    pub replaced_existing: bool,
}

#[tauri::command]
#[specta::specta]
pub async fn diagnostic_log_query(
    state: State<'_, AppState>,
    query: DiagnosticLogQuery,
) -> Result<DiagnosticLogPageViewModel, intercept_proxy_application::AppErrorViewModel> {
    Ok(state.application.diagnostic_log_query(&query))
}

/// 生成与 MCP `reproduction_report` 同源的报告，并通过系统保存对话框原子写入 Markdown。
#[tauri::command]
#[specta::specta]
pub async fn diagnostic_reproduction_report_export(
    state: State<'_, AppState>,
    query: DiagnosticReportQuery,
) -> CommandResult<Option<DiagnosticReportExportOutcome>> {
    let report =
        reproduction_report::generate(&state.application, state.runtime_logs().as_ref(), query)
            .await
            .map_err(command_error)?;
    let host = state.host();
    let dialog = host.file_dialog();
    let selection = tokio::task::spawn_blocking(move || {
        dialog.choose_save_file(
            REPRODUCTION_REPORT_DIALOG_PURPOSE,
            REPRODUCTION_REPORT_FILE_NAME,
        )
    })
    .await
    .map_err(|_| command_error(report_task_failed()))?
    .map_err(command_error)?;
    let Some(selection) = selection else {
        return Ok(None);
    };
    let bytes = report.markdown.into_bytes();
    let outcome = tokio::task::spawn_blocking(move || {
        AtomicFileExporter.write(&selection.path, &bytes, selection.overwrite_confirmed)
    })
    .await
    .map_err(|_| command_error(report_task_failed()))?
    .map_err(|_| command_error(report_write_failed()))?;
    Ok(Some(DiagnosticReportExportOutcome {
        bytes_written: outcome.bytes_written,
        replaced_existing: outcome.replaced_existing,
    }))
}

fn report_task_failed() -> AppError {
    AppError::new(
        "DIAGNOSTIC_REPORT_FILE_TASK_FAILED",
        "故障复现报告文件任务未能完成。",
    )
}

fn report_write_failed() -> AppError {
    AppError::new(
        "DIAGNOSTIC_REPORT_WRITE_FAILED",
        "无法原子写入故障复现 Markdown 报告。",
    )
}
