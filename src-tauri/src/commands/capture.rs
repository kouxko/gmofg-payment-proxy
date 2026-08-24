//! 抓包与断点命令适配层：查询和决策语义由 application 门面定义，本层保持 IPC 形状。

use std::sync::Arc;

use chrono::Utc;
use intercept_proxy_application::{
    AppError, BreakpointDecision, BreakpointDetailViewModel, BreakpointDraft, BreakpointId,
    BreakpointSummaryViewModel, BreakpointValidationViewModel, CaptureDetailViewModel,
    CapturePageViewModel, CaptureQuery, ExchangeObservationPage, ExchangeObservationQuery,
    ExchangeObservationRecord, OperationResultViewModel, RuntimeEpoch, SessionId, UiEventPayload,
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

/// 查询 tracing UI Layer 与 MCP 共享的连接级 Exchange 时间线。
#[tauri::command]
#[specta::specta]
pub async fn exchange_observation_query(
    state: State<'_, AppState>,
    query: ExchangeObservationQuery,
) -> CommandResult<ExchangeObservationPage> {
    Ok(state.exchange_observations().query(&query))
}

#[tauri::command]
#[specta::specta]
pub async fn exchange_observation_get(
    state: State<'_, AppState>,
    exchange_id: String,
) -> CommandResult<ExchangeObservationRecord> {
    state
        .exchange_observations()
        .get(&exchange_id)
        .ok_or_else(|| {
            command_error(AppError::new(
                "EXCHANGE_OBSERVATION_NOT_FOUND",
                "Exchange 运行记录不存在或已被内存淘汰。",
            ))
        })
}

#[tauri::command]
#[specta::specta]
pub async fn exchange_observation_clear(
    state: State<'_, AppState>,
    workspace_id: WorkspaceId,
    confirmed: bool,
) -> CommandResult<OperationResultViewModel> {
    if !confirmed {
        return Err(command_error(AppError::new(
            "CONFIRMATION_REQUIRED",
            "清空 Exchange 运行记录需要确认。",
        )));
    }
    let count = state.exchange_observations().clear_workspace(workspace_id);
    state.host().events().publish(
        None,
        Utc::now(),
        Some(workspace_id.to_string()),
        None,
        UiEventPayload::ExchangeObservationChanged,
    );
    Ok(OperationResultViewModel::success(format!(
        "已清空 {count} 条 Exchange 运行记录。"
    )))
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
