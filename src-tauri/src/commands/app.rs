//! 应用生命周期命令适配层：只负责 Tauri 状态/通道与应用门面的转换。

use std::collections::BTreeMap;

use intercept_proxy_application::{
    AppBootstrapViewModel, AppErrorViewModel, OperationResultViewModel, SubscriptionAckViewModel,
    UiEventEnvelope,
};
use tauri::{State, ipc::Channel};

use super::{CommandResult, command_error};
use crate::app_state::AppState;

#[tauri::command]
#[specta::specta]
pub async fn app_bootstrap(state: State<'_, AppState>) -> CommandResult<AppBootstrapViewModel> {
    state
        .application
        .app_bootstrap()
        .await
        .map_err(command_error)
}

#[tauri::command]
#[specta::specta]
/// 建立“先回放、后实时”的有序 UI 事件通道。
/// application 在同一临界区内确定回放边界并注册实时队列，因此同步发送 replay 时新事件
/// 只会排入 live，不会插到回放中间。回放全部成功后才启动异步转发任务，保证前端看到的
/// `event_id` 单调递增。Channel 关闭、实时队列终止或任务自然结束时，尾部清理都会读取终止
/// 原因并注销订阅，释放队列记账；显式取消命令只是同一清理路径的主动入口。
pub async fn app_subscribe_events(
    state: State<'_, AppState>,
    after_event_id: u64,
    on_event: Channel<UiEventEnvelope>,
) -> CommandResult<SubscriptionAckViewModel> {
    let mut subscription = state
        .application
        .app_subscribe_events(after_event_id)
        .map_err(command_error)?;
    let acknowledgement = subscription.ack.clone();
    subscription
        .replay
        .drain_with(|event| {
            on_event.send(event).map_err(|_| {
                Box::new(AppErrorViewModel {
                    code: "CHANNEL_SEND_FAILED".to_owned(),
                    message: "实时事件通道已关闭。".to_owned(),
                    field_errors: BTreeMap::default(),
                    retryable: true,
                    suggested_action: Some("请重新获取应用快照并订阅事件。".to_owned()),
                    entity_id: None,
                    runtime_epoch: None,
                })
            })
        })
        .map_err(|error| *error)?;
    let application = state.application.clone();
    tauri::async_runtime::spawn(async move {
        while let Some(event) = subscription.live.recv().await {
            if on_event.send(event).is_err() {
                break;
            }
        }
        if let Some(failure) =
            application.app_take_subscription_failure(subscription.subscription_id)
        {
            let _ = on_event.send(failure);
        }
        application.app_unsubscribe_events(subscription.subscription_id);
    });
    Ok(acknowledgement)
}

#[tauri::command]
#[specta::specta]
pub async fn app_unsubscribe_events(
    state: State<'_, AppState>,
    subscription_id: u64,
) -> CommandResult<OperationResultViewModel> {
    Ok(state.application.app_unsubscribe_events(subscription_id))
}
