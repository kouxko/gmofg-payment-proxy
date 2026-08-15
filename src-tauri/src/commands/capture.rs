//! 抓包、会话与断点命令适配层：查询和决策语义由 application 门面定义，本层保持 IPC 形状。

use std::sync::Arc;

use intercept_proxy_application::{
    AppError, BreakpointDecision, BreakpointDetailViewModel, BreakpointDraft, BreakpointId,
    BreakpointSummaryViewModel, BreakpointValidationViewModel, CaptureDetailViewModel,
    CapturePageViewModel, CaptureQuery, OperationResultViewModel, RuntimeEpoch,
    SessionDetailViewModel, SessionId, SessionListViewModel, SessionQuery,
    SocketCaptureDetailViewModel, SocketCaptureId, SocketCapturePageViewModel, SocketCaptureQuery,
    WorkspaceId,
};
use tauri::State;

use super::{CommandResult, command_error};
use crate::app_state::AppState;

#[tauri::command]
#[specta::specta]
pub async fn capture_query(
    state: State<'_, AppState>,
    query: CaptureQuery,
) -> CommandResult<CapturePageViewModel> {
    state
        .application
        .capture_query(query)
        .await
        .map_err(command_error)
}

#[tauri::command]
#[specta::specta]
pub async fn capture_get_detail(
    state: State<'_, AppState>,
    session_id: SessionId,
    runtime_epoch: RuntimeEpoch,
) -> CommandResult<CaptureDetailViewModel> {
    state
        .application
        .capture_get_detail(session_id, runtime_epoch)
        .await
        .map_err(command_error)
}

#[tauri::command]
#[specta::specta]
pub async fn capture_clear_view(
    state: State<'_, AppState>,
    current_cursor: u64,
) -> CommandResult<u64> {
    state
        .application
        .capture_clear_view(current_cursor)
        .await
        .map_err(command_error)
}

/// 查询独立的 Socket Frame/LocalExchange 时间线。
///
/// 该命令不返回 HTTP start line、Header、status 或 JSONPath；T28 只需消费这一套
/// 协议中立 DTO，不必从 HTTP 抓包字段推断 Socket 语义。
#[tauri::command]
#[specta::specta]
pub async fn socket_capture_query(
    state: State<'_, AppState>,
    query: SocketCaptureQuery,
) -> CommandResult<SocketCapturePageViewModel> {
    state
        .application
        .socket_capture_query(query)
        .await
        .map_err(command_error)
}

#[tauri::command]
#[specta::specta]
pub async fn socket_capture_get_detail(
    state: State<'_, AppState>,
    capture_id: SocketCaptureId,
) -> CommandResult<SocketCaptureDetailViewModel> {
    state
        .application
        .socket_capture_get_detail(capture_id)
        .await
        .map_err(command_error)
}

#[tauri::command]
#[specta::specta]
pub async fn socket_capture_clear(
    state: State<'_, AppState>,
    workspace_id: WorkspaceId,
    confirmed: bool,
) -> CommandResult<OperationResultViewModel> {
    state
        .application
        .socket_capture_clear(workspace_id, confirmed)
        .await
        .map_err(command_error)
}

#[tauri::command]
#[specta::specta]
pub async fn session_query(
    state: State<'_, AppState>,
    query: SessionQuery,
) -> CommandResult<SessionListViewModel> {
    state
        .application
        .session_query(query)
        .await
        .map_err(command_error)
}

#[tauri::command]
#[specta::specta]
pub async fn session_get(
    state: State<'_, AppState>,
    session_id: SessionId,
) -> CommandResult<SessionDetailViewModel> {
    state
        .application
        .session_get(session_id)
        .await
        .map_err(command_error)
}

#[tauri::command]
#[specta::specta]
pub async fn session_clear(
    state: State<'_, AppState>,
    confirmed: bool,
) -> CommandResult<OperationResultViewModel> {
    state
        .application
        .session_clear(confirmed)
        .await
        .map_err(command_error)
}

#[tauri::command]
#[specta::specta]
pub async fn breakpoint_query(
    state: State<'_, AppState>,
    runtime_epoch: Option<RuntimeEpoch>,
) -> CommandResult<Vec<BreakpointSummaryViewModel>> {
    Ok(state.application.breakpoint_query(runtime_epoch))
}

#[tauri::command]
#[specta::specta]
pub async fn breakpoint_get(
    state: State<'_, AppState>,
    breakpoint_id: BreakpointId,
    runtime_epoch: RuntimeEpoch,
) -> CommandResult<BreakpointDetailViewModel> {
    state
        .application
        .breakpoint_get(breakpoint_id, runtime_epoch)
        .map_err(command_error)
}

#[tauri::command]
#[specta::specta]
pub async fn breakpoint_format_json(
    state: State<'_, AppState>,
    draft: BreakpointDraft,
) -> CommandResult<BreakpointDraft> {
    state
        .application
        .breakpoint_format_json(draft)
        .map_err(command_error)
}

#[tauri::command]
#[specta::specta]
pub async fn breakpoint_restore_original(
    state: State<'_, AppState>,
    breakpoint_id: BreakpointId,
    runtime_epoch: RuntimeEpoch,
) -> CommandResult<BreakpointDraft> {
    state
        .application
        .breakpoint_restore_original(breakpoint_id, runtime_epoch)
        .map_err(command_error)
}

#[tauri::command]
#[specta::specta]
pub async fn breakpoint_validate(
    state: State<'_, AppState>,
    draft: BreakpointDraft,
    runtime_epoch: RuntimeEpoch,
) -> CommandResult<BreakpointValidationViewModel> {
    state
        .application
        .breakpoint_validate(&draft, runtime_epoch)
        .map_err(command_error)
}

#[tauri::command]
#[specta::specta]
pub async fn breakpoint_resolve(
    state: State<'_, AppState>,
    runtime_epoch: RuntimeEpoch,
    decision: BreakpointDecision,
) -> CommandResult<BreakpointSummaryViewModel> {
    let application = Arc::clone(&state.application);
    tokio::task::spawn_blocking(move || application.breakpoint_resolve(runtime_epoch, decision))
        .await
        .map_err(|error| {
            command_error(AppError::new(
                "BREAKPOINT_RESOLVE_TASK_FAILED",
                format!("断点处理任务异常结束：{error}"),
            ))
        })?
        .map_err(command_error)
}

#[cfg(test)]
#[path = "capture/tests.rs"]
mod tests;
