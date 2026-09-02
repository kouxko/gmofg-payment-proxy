//! 抓包命令适配层：查询语义由 application 门面定义，本层保持 IPC 形状。

use chrono::Utc;
use intercept_proxy_application::{
    AppError, CaptureDetailViewModel, CapturePageViewModel, CaptureQuery, ExchangeObservationPage,
    ExchangeObservationQuery, ExchangeObservationRecord, OperationResultViewModel, RuntimeEpoch,
    SessionId, UiEventPayload, WorkspaceId,
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
