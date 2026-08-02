//! Tauri Command 的薄适配层与 TypeScript 绑定清单。
//!
//! 每个命令只做参数/错误映射并调用 `AppState.application`；业务规则、数据库和网络 I/O
//! 不应写在这里。事件订阅先重放缺失事件再接实时通道，发送端关闭会返回可重试错误。

use intercept_proxy_application::{
    ActiveFaultViewModel, AndroidAdbViewModel, AndroidCompanionInstallViewModel,
    AndroidDeviceViewModel, AndroidNetworkProfile, AndroidNetworkProfileSummary,
    AndroidNetworkStatusViewModel, AndroidPackageViewModel, AndroidProfileEditIntent,
    AppBootstrapViewModel, AppError, AppErrorViewModel, BreakpointDecision,
    BreakpointDetailViewModel, BreakpointDraft, BreakpointId, BreakpointSummaryViewModel,
    BreakpointValidationViewModel, CaptureDetailViewModel, CapturePageViewModel, CaptureQuery,
    CertificateOverviewViewModel, CertificateValidationViewModel, FaultConfigurationDraft,
    FaultTemplateViewModel, ListenerId, ListenerOverviewViewModel, ListenerStatusViewModel,
    ListenerUpstreamTlsTestViewModel, OperationResultViewModel, ProxyListener, ProxyWorkspace,
    RuleAction, RuleActionKind, RuleByteInputViewModel, RuleCondition, RuleConditionKind,
    RuleDraft, RuleHeaderInputViewModel, RuleId, RuleMatchField, RuleMatchFieldKind,
    RuleMatchOperator, RuleMatchOperatorKind, RuleSummaryViewModel, RuleViewModel, RuntimeEpoch,
    SecretReference, SessionDetailViewModel, SessionId, SessionPageViewModel, SessionQuery,
    SettingsDraft, SettingsValidationViewModel, SettingsViewModel, SubscriptionAckViewModel,
    UiEventEnvelope, WorkspaceId, WorkspaceSummaryViewModel, WorkspaceValidationViewModel,
};
use tauri::{State, Wry, ipc::Channel};
use tauri_specta::{Builder, collect_commands};

use crate::app_state::AppState;

type CommandResult<T> = Result<T, AppErrorViewModel>;

fn command_error(error: AppError) -> AppErrorViewModel {
    error.into()
}

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

#[tauri::command]
#[specta::specta]
pub async fn android_adb_get(state: State<'_, AppState>) -> CommandResult<AndroidAdbViewModel> {
    state
        .application
        .android_adb_get()
        .await
        .map_err(command_error)
}

#[tauri::command]
#[specta::specta]
pub async fn android_adb_select(
    state: State<'_, AppState>,
    serial: String,
) -> CommandResult<AndroidAdbViewModel> {
    state
        .application
        .android_adb_select(serial)
        .await
        .map_err(command_error)
}

#[tauri::command]
#[specta::specta]
pub async fn android_device_list(
    state: State<'_, AppState>,
) -> CommandResult<Vec<AndroidDeviceViewModel>> {
    state
        .application
        .android_device_list()
        .await
        .map_err(command_error)
}

#[tauri::command]
#[specta::specta]
pub async fn android_package_list(
    state: State<'_, AppState>,
) -> CommandResult<Vec<AndroidPackageViewModel>> {
    state
        .application
        .android_package_list()
        .await
        .map_err(command_error)
}

#[tauri::command]
#[specta::specta]
pub async fn android_package_query(
    state: State<'_, AppState>,
    query: String,
) -> CommandResult<Vec<AndroidPackageViewModel>> {
    state
        .application
        .android_package_query(query)
        .await
        .map_err(command_error)
}

#[tauri::command]
#[specta::specta]
pub async fn android_package_get(
    state: State<'_, AppState>,
    package_name: String,
) -> CommandResult<AndroidPackageViewModel> {
    state
        .application
        .android_package_get(package_name)
        .await
        .map_err(command_error)
}

#[tauri::command]
#[specta::specta]
pub async fn android_companion_install(
    state: State<'_, AppState>,
) -> CommandResult<AndroidCompanionInstallViewModel> {
    state
        .application
        .android_companion_install()
        .await
        .map_err(command_error)
}

#[tauri::command]
#[specta::specta]
pub async fn android_companion_update(
    state: State<'_, AppState>,
) -> CommandResult<AndroidCompanionInstallViewModel> {
    state
        .application
        .android_companion_update()
        .await
        .map_err(command_error)
}

#[tauri::command]
#[specta::specta]
pub async fn android_vpn_open_consent(
    state: State<'_, AppState>,
) -> CommandResult<AndroidNetworkStatusViewModel> {
    state
        .application
        .android_vpn_open_consent()
        .await
        .map_err(command_error)
}

#[tauri::command]
#[specta::specta]
pub async fn device_network_profile_list(
    state: State<'_, AppState>,
) -> CommandResult<Vec<AndroidNetworkProfileSummary>> {
    state
        .application
        .device_network_profile_list()
        .await
        .map_err(command_error)
}

#[tauri::command]
#[specta::specta]
// Tauri owns extracted State/JSON arguments, and the IPC contract intentionally keeps the same
// structured error envelope even for a Rust-owned default constructor.
#[allow(
    clippy::needless_pass_by_value,
    clippy::result_large_err,
    clippy::unnecessary_wraps
)]
pub fn device_network_profile_new(
    state: State<'_, AppState>,
) -> CommandResult<AndroidNetworkProfile> {
    Ok(state.application.device_network_profile_new())
}

#[tauri::command]
#[specta::specta]
pub async fn device_network_profile_get(
    state: State<'_, AppState>,
    profile_id: String,
) -> CommandResult<AndroidNetworkProfile> {
    state
        .application
        .device_network_profile_get(profile_id)
        .await
        .map_err(command_error)
}

#[tauri::command]
#[specta::specta]
pub async fn device_network_profile_apply_intent(
    state: State<'_, AppState>,
    profile: AndroidNetworkProfile,
    intent: AndroidProfileEditIntent,
) -> CommandResult<AndroidNetworkProfile> {
    state
        .application
        .device_network_profile_apply_intent(profile, intent)
        .await
        .map_err(command_error)
}

#[tauri::command]
#[specta::specta]
pub async fn device_network_profile_save(
    state: State<'_, AppState>,
    profile: AndroidNetworkProfile,
) -> CommandResult<AndroidNetworkProfile> {
    state
        .application
        .device_network_profile_save(profile)
        .await
        .map_err(command_error)
}

#[tauri::command]
#[specta::specta]
pub async fn device_network_profile_delete(
    state: State<'_, AppState>,
    profile_id: String,
) -> CommandResult<OperationResultViewModel> {
    state
        .application
        .device_network_profile_delete(profile_id)
        .await
        .map_err(command_error)
}

#[tauri::command]
#[specta::specta]
pub async fn device_network_start(
    state: State<'_, AppState>,
    profile_id: String,
    dangerous_confirmed: bool,
) -> CommandResult<AndroidNetworkStatusViewModel> {
    state
        .application
        .device_network_start(profile_id, dangerous_confirmed)
        .await
        .map_err(command_error)
}

#[tauri::command]
#[specta::specta]
pub async fn device_network_apply(
    state: State<'_, AppState>,
    profile_id: String,
    dangerous_confirmed: bool,
) -> CommandResult<AndroidNetworkStatusViewModel> {
    state
        .application
        .device_network_apply(profile_id, dangerous_confirmed)
        .await
        .map_err(command_error)
}

#[tauri::command]
#[specta::specta]
pub async fn device_network_stop(
    state: State<'_, AppState>,
) -> CommandResult<AndroidNetworkStatusViewModel> {
    state
        .application
        .device_network_stop()
        .await
        .map_err(command_error)
}

#[tauri::command]
#[specta::specta]
pub async fn device_network_emergency_restore(
    state: State<'_, AppState>,
) -> CommandResult<AndroidNetworkStatusViewModel> {
    state
        .application
        .device_network_emergency_restore()
        .await
        .map_err(command_error)
}

#[tauri::command]
#[specta::specta]
pub async fn device_network_status(
    state: State<'_, AppState>,
) -> CommandResult<AndroidNetworkStatusViewModel> {
    state
        .application
        .device_network_status()
        .await
        .map_err(command_error)
}

#[tauri::command]
#[specta::specta]
pub async fn workspace_list(
    state: State<'_, AppState>,
) -> CommandResult<Vec<WorkspaceSummaryViewModel>> {
    state
        .application
        .workspace_list()
        .await
        .map_err(command_error)
}

#[tauri::command]
#[specta::specta]
pub async fn workspace_get(
    state: State<'_, AppState>,
    workspace_id: WorkspaceId,
) -> CommandResult<ProxyWorkspace> {
    state
        .application
        .workspace_get(workspace_id)
        .await
        .map_err(command_error)
}

#[tauri::command]
#[specta::specta]
// Tauri passes deserialized command values by ownership; changing these to references would make
// the generated IPC command incompatible with its extractor.
#[allow(clippy::needless_pass_by_value, clippy::result_large_err)]
pub fn workspace_component_new(
    state: State<'_, AppState>,
    workspace: ProxyWorkspace,
    kind: String,
) -> CommandResult<ProxyWorkspace> {
    state
        .application
        .workspace_component_new(workspace, &kind)
        .map_err(command_error)
}

#[tauri::command]
#[specta::specta]
#[allow(clippy::needless_pass_by_value, clippy::result_large_err)]
pub fn workspace_component_apply_intent(
    state: State<'_, AppState>,
    workspace: ProxyWorkspace,
    component_kind: String,
    component_id: String,
    operation: String,
    value: String,
) -> CommandResult<ProxyWorkspace> {
    state
        .application
        .workspace_component_apply_intent(
            workspace,
            &component_kind,
            &component_id,
            &operation,
            &value,
        )
        .map_err(command_error)
}

#[tauri::command]
#[specta::specta]
pub async fn workspace_secret_store_basic(
    state: State<'_, AppState>,
    username: String,
    password: String,
) -> CommandResult<SecretReference> {
    state
        .application
        .workspace_secret_store_basic(username, password)
        .await
        .map_err(command_error)
}

#[tauri::command]
#[specta::specta]
pub async fn workspace_create(
    state: State<'_, AppState>,
    name: String,
) -> CommandResult<ProxyWorkspace> {
    state
        .application
        .workspace_create(name)
        .await
        .map_err(command_error)
}

#[tauri::command]
#[specta::specta]
pub async fn workspace_copy(
    state: State<'_, AppState>,
    workspace_id: WorkspaceId,
) -> CommandResult<ProxyWorkspace> {
    state
        .application
        .workspace_copy(workspace_id)
        .await
        .map_err(command_error)
}

#[tauri::command]
#[specta::specta]
pub async fn workspace_select(
    state: State<'_, AppState>,
    workspace_id: WorkspaceId,
) -> CommandResult<WorkspaceSummaryViewModel> {
    state
        .application
        .workspace_select(workspace_id)
        .await
        .map_err(command_error)
}

#[tauri::command]
#[specta::specta]
pub async fn workspace_validate(
    state: State<'_, AppState>,
    workspace: ProxyWorkspace,
) -> CommandResult<WorkspaceValidationViewModel> {
    state
        .application
        .workspace_validate(workspace)
        .await
        .map_err(command_error)
}

#[tauri::command]
#[specta::specta]
pub async fn workspace_save(
    state: State<'_, AppState>,
    workspace: ProxyWorkspace,
) -> CommandResult<ProxyWorkspace> {
    state
        .application
        .workspace_save(workspace)
        .await
        .map_err(command_error)
}

#[tauri::command]
#[specta::specta]
pub async fn workspace_delete(
    state: State<'_, AppState>,
    workspace_id: WorkspaceId,
    expected_revision: u64,
) -> CommandResult<OperationResultViewModel> {
    state
        .application
        .workspace_delete(workspace_id, expected_revision)
        .await
        .map_err(command_error)
}

#[tauri::command]
#[specta::specta]
pub async fn workspace_import(
    state: State<'_, AppState>,
) -> CommandResult<OperationResultViewModel> {
    state
        .application
        .workspace_import()
        .await
        .map_err(command_error)
}

#[tauri::command]
#[specta::specta]
pub async fn workspace_export(
    state: State<'_, AppState>,
    workspace_id: WorkspaceId,
) -> CommandResult<OperationResultViewModel> {
    state
        .application
        .workspace_export(workspace_id)
        .await
        .map_err(command_error)
}

#[tauri::command]
#[specta::specta]
pub async fn listener_list(
    state: State<'_, AppState>,
    workspace_id: WorkspaceId,
) -> CommandResult<Vec<ProxyListener>> {
    state
        .application
        .listener_list(workspace_id)
        .await
        .map_err(command_error)
}

#[tauri::command]
#[specta::specta]
// See `workspace_component_new`: owned command parameters are part of the Tauri adapter boundary.
#[allow(clippy::needless_pass_by_value, clippy::result_large_err)]
pub fn listener_new(state: State<'_, AppState>, kind: String) -> CommandResult<ProxyListener> {
    state.application.listener_new(&kind).map_err(command_error)
}

#[tauri::command]
#[specta::specta]
#[allow(clippy::needless_pass_by_value, clippy::result_large_err)]
pub fn listener_copy(
    state: State<'_, AppState>,
    source: ProxyListener,
) -> CommandResult<ProxyListener> {
    state
        .application
        .listener_copy(source)
        .map_err(command_error)
}

#[tauri::command]
#[specta::specta]
pub async fn listener_get(
    state: State<'_, AppState>,
    workspace_id: WorkspaceId,
    listener_id: ListenerId,
) -> CommandResult<ProxyListener> {
    state
        .application
        .listener_get(workspace_id, listener_id)
        .await
        .map_err(command_error)
}

#[tauri::command]
#[specta::specta]
pub async fn listener_save(
    state: State<'_, AppState>,
    workspace_id: WorkspaceId,
    expected_workspace_revision: u64,
    listener: ProxyListener,
) -> CommandResult<ProxyListener> {
    state
        .application
        .listener_save(workspace_id, expected_workspace_revision, listener)
        .await
        .map_err(command_error)
}

#[tauri::command]
#[specta::specta]
pub async fn listener_delete(
    state: State<'_, AppState>,
    workspace_id: WorkspaceId,
    expected_workspace_revision: u64,
    listener_id: ListenerId,
) -> CommandResult<OperationResultViewModel> {
    state
        .application
        .listener_delete(workspace_id, expected_workspace_revision, listener_id)
        .await
        .map_err(command_error)
}

#[tauri::command]
#[specta::specta]
pub async fn listener_statuses(
    state: State<'_, AppState>,
) -> CommandResult<Vec<ListenerStatusViewModel>> {
    state
        .application
        .listener_statuses()
        .await
        .map_err(command_error)
}

#[tauri::command]
#[specta::specta]
pub async fn listener_overview(
    state: State<'_, AppState>,
    workspace_id: WorkspaceId,
) -> CommandResult<ListenerOverviewViewModel> {
    state
        .application
        .listener_overview(workspace_id)
        .await
        .map_err(command_error)
}

#[tauri::command]
#[specta::specta]
pub async fn listener_start(
    state: State<'_, AppState>,
    workspace_id: WorkspaceId,
    expected_workspace_revision: u64,
    listener_id: ListenerId,
) -> CommandResult<ListenerStatusViewModel> {
    state
        .application
        .listener_start(workspace_id, expected_workspace_revision, listener_id)
        .await
        .map_err(command_error)
}

#[tauri::command]
#[specta::specta]
pub async fn listener_stop(
    state: State<'_, AppState>,
    workspace_id: WorkspaceId,
    expected_workspace_revision: u64,
    listener_id: ListenerId,
) -> CommandResult<ListenerStatusViewModel> {
    state
        .application
        .listener_stop(workspace_id, expected_workspace_revision, listener_id)
        .await
        .map_err(command_error)
}

#[tauri::command]
#[specta::specta]
pub async fn listener_test_upstream_tls(
    state: State<'_, AppState>,
    workspace_id: WorkspaceId,
    listener_id: ListenerId,
) -> CommandResult<ListenerUpstreamTlsTestViewModel> {
    state
        .application
        .listener_test_upstream_tls(workspace_id, listener_id)
        .await
        .map_err(command_error)
}

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

#[tauri::command]
#[specta::specta]
pub async fn session_query(
    state: State<'_, AppState>,
    query: SessionQuery,
) -> CommandResult<SessionPageViewModel> {
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
pub async fn session_export(
    state: State<'_, AppState>,
    session_id: SessionId,
    sensitive_data_confirmed: bool,
) -> CommandResult<OperationResultViewModel> {
    state
        .application
        .session_export(session_id, sensitive_data_confirmed)
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
    state
        .application
        .breakpoint_resolve(runtime_epoch, decision)
        .await
        .map_err(command_error)
}

#[tauri::command]
#[specta::specta]
pub async fn rule_list(state: State<'_, AppState>) -> CommandResult<Vec<RuleSummaryViewModel>> {
    state.application.rule_list().await.map_err(command_error)
}

#[tauri::command]
#[specta::specta]
pub async fn rule_get(state: State<'_, AppState>, rule_id: RuleId) -> CommandResult<RuleViewModel> {
    state
        .application
        .rule_get(rule_id)
        .await
        .map_err(command_error)
}

#[tauri::command]
#[specta::specta]
pub async fn rule_new_draft(state: State<'_, AppState>) -> CommandResult<RuleDraft> {
    state
        .application
        .rule_new_draft()
        .await
        .map_err(command_error)
}

#[tauri::command]
#[specta::specta]
pub async fn rule_condition_draft(
    state: State<'_, AppState>,
    kind: RuleConditionKind,
) -> CommandResult<RuleCondition> {
    Ok(state.application.rule_condition_draft(kind))
}

#[tauri::command]
#[specta::specta]
pub async fn rule_action_draft(
    state: State<'_, AppState>,
    kind: RuleActionKind,
) -> CommandResult<RuleAction> {
    Ok(state.application.rule_action_draft(kind))
}

#[tauri::command]
#[specta::specta]
pub async fn rule_match_field_draft(
    state: State<'_, AppState>,
    kind: RuleMatchFieldKind,
) -> CommandResult<RuleMatchField> {
    Ok(state.application.rule_match_field_draft(kind))
}

#[tauri::command]
#[specta::specta]
pub async fn rule_match_operator_draft(
    state: State<'_, AppState>,
    kind: RuleMatchOperatorKind,
) -> CommandResult<RuleMatchOperator> {
    Ok(state.application.rule_match_operator_draft(kind))
}

#[tauri::command]
#[specta::specta]
pub async fn rule_parse_byte_input(
    state: State<'_, AppState>,
    raw: String,
) -> CommandResult<RuleByteInputViewModel> {
    state
        .application
        .rule_parse_byte_input(&raw)
        .map_err(command_error)
}

#[tauri::command]
#[specta::specta]
pub async fn rule_parse_header_input(
    state: State<'_, AppState>,
    raw: String,
) -> CommandResult<RuleHeaderInputViewModel> {
    state
        .application
        .rule_parse_header_input(&raw)
        .map_err(command_error)
}

#[tauri::command]
#[specta::specta]
pub async fn rule_create_from_session(
    state: State<'_, AppState>,
    session_id: SessionId,
) -> CommandResult<RuleDraft> {
    state
        .application
        .rule_create_from_session(session_id)
        .await
        .map_err(command_error)
}

#[tauri::command]
#[specta::specta]
pub async fn rule_save(
    state: State<'_, AppState>,
    draft: RuleDraft,
) -> CommandResult<RuleViewModel> {
    state
        .application
        .rule_save(draft)
        .await
        .map_err(command_error)
}

#[tauri::command]
#[specta::specta]
pub async fn rule_copy(
    state: State<'_, AppState>,
    rule_id: RuleId,
) -> CommandResult<RuleViewModel> {
    state
        .application
        .rule_copy(rule_id)
        .await
        .map_err(command_error)
}

#[tauri::command]
#[specta::specta]
pub async fn rule_delete(
    state: State<'_, AppState>,
    rule_id: RuleId,
    expected_revision: u64,
    confirmed: bool,
) -> CommandResult<OperationResultViewModel> {
    state
        .application
        .rule_delete(rule_id, expected_revision, confirmed)
        .await
        .map_err(command_error)
}

#[tauri::command]
#[specta::specta]
pub async fn rule_toggle(
    state: State<'_, AppState>,
    rule_id: RuleId,
    expected_revision: u64,
    enabled: bool,
) -> CommandResult<RuleViewModel> {
    state
        .application
        .rule_toggle(rule_id, expected_revision, enabled)
        .await
        .map_err(command_error)
}

#[tauri::command]
#[specta::specta]
pub async fn rule_import(state: State<'_, AppState>) -> CommandResult<OperationResultViewModel> {
    state.application.rule_import().await.map_err(command_error)
}

#[tauri::command]
#[specta::specta]
pub async fn rule_export(state: State<'_, AppState>) -> CommandResult<OperationResultViewModel> {
    state.application.rule_export().await.map_err(command_error)
}

#[tauri::command]
#[specta::specta]
pub async fn fault_template_list(
    state: State<'_, AppState>,
) -> CommandResult<Vec<FaultTemplateViewModel>> {
    state
        .application
        .fault_template_list()
        .await
        .map_err(command_error)
}

#[tauri::command]
#[specta::specta]
pub async fn fault_configure(
    state: State<'_, AppState>,
    draft: FaultConfigurationDraft,
) -> CommandResult<ActiveFaultViewModel> {
    state
        .application
        .fault_configure(draft)
        .await
        .map_err(command_error)
}

#[tauri::command]
#[specta::specta]
pub async fn fault_active_list(
    state: State<'_, AppState>,
) -> CommandResult<Vec<ActiveFaultViewModel>> {
    state
        .application
        .fault_active_list()
        .await
        .map_err(command_error)
}

#[tauri::command]
#[specta::specta]
pub async fn fault_stop(
    state: State<'_, AppState>,
    rule_id: RuleId,
    expected_revision: u64,
    confirmed: bool,
) -> CommandResult<ActiveFaultViewModel> {
    state
        .application
        .fault_stop(rule_id, expected_revision, confirmed)
        .await
        .map_err(command_error)
}

#[tauri::command]
#[specta::specta]
pub async fn certificate_overview(
    state: State<'_, AppState>,
) -> CommandResult<CertificateOverviewViewModel> {
    state
        .application
        .certificate_overview()
        .await
        .map_err(command_error)
}

#[tauri::command]
#[specta::specta]
pub async fn certificate_generate_ca(
    state: State<'_, AppState>,
    sans: Vec<String>,
) -> CommandResult<CertificateOverviewViewModel> {
    state
        .application
        .certificate_generate_ca(sans)
        .await
        .map_err(command_error)
}

#[tauri::command]
#[specta::specta]
pub async fn certificate_export_ca(
    state: State<'_, AppState>,
) -> CommandResult<OperationResultViewModel> {
    state
        .application
        .certificate_export_ca()
        .await
        .map_err(command_error)
}

#[tauri::command]
#[specta::specta]
pub async fn certificate_reissue_leaf(
    state: State<'_, AppState>,
    expected_revision: u64,
    sans: Vec<String>,
) -> CommandResult<CertificateOverviewViewModel> {
    state
        .application
        .certificate_reissue_leaf(expected_revision, sans)
        .await
        .map_err(command_error)
}

#[tauri::command]
#[specta::specta]
pub async fn certificate_import_pkcs12(
    state: State<'_, AppState>,
    password: String,
) -> CommandResult<CertificateOverviewViewModel> {
    state
        .application
        .certificate_import_pkcs12(password)
        .await
        .map_err(command_error)
}

#[tauri::command]
#[specta::specta]
pub async fn certificate_import_upstream_ca(
    state: State<'_, AppState>,
) -> CommandResult<CertificateOverviewViewModel> {
    state
        .application
        .certificate_import_upstream_ca()
        .await
        .map_err(command_error)
}

#[tauri::command]
#[specta::specta]
pub async fn certificate_validate(
    state: State<'_, AppState>,
) -> CommandResult<CertificateValidationViewModel> {
    state
        .application
        .certificate_validate()
        .await
        .map_err(command_error)
}

#[tauri::command]
#[specta::specta]
pub async fn certificate_reset_ca(
    state: State<'_, AppState>,
    expected_revision: u64,
    confirmed: bool,
) -> CommandResult<CertificateOverviewViewModel> {
    state
        .application
        .certificate_reset_ca(expected_revision, confirmed)
        .await
        .map_err(command_error)
}

#[tauri::command]
#[specta::specta]
pub async fn settings_get(state: State<'_, AppState>) -> CommandResult<SettingsViewModel> {
    state
        .application
        .settings_get()
        .await
        .map_err(command_error)
}

#[tauri::command]
#[specta::specta]
pub async fn settings_validate(
    state: State<'_, AppState>,
    draft: SettingsDraft,
) -> CommandResult<SettingsValidationViewModel> {
    state
        .application
        .settings_validate(draft)
        .await
        .map_err(command_error)
}

#[tauri::command]
#[specta::specta]
pub async fn settings_save(
    state: State<'_, AppState>,
    draft: SettingsDraft,
) -> CommandResult<SettingsViewModel> {
    state
        .application
        .settings_save(draft)
        .await
        .map_err(command_error)
}

#[tauri::command]
#[specta::specta]
pub async fn settings_reset_defaults(
    state: State<'_, AppState>,
    confirmed: bool,
) -> CommandResult<SettingsDraft> {
    state
        .application
        .settings_reset_defaults(confirmed)
        .await
        .map_err(command_error)
}

pub fn builder() -> Builder<Wry> {
    // Revisions, cursors and logical byte counters are bounded well below
    // JavaScript's safe integer ceiling by product capacity limits.
    Builder::<Wry>::new()
        .dangerously_cast_bigints_to_number()
        .commands(collect_commands![
            app_bootstrap,
            app_subscribe_events,
            app_unsubscribe_events,
            android_adb_get,
            android_adb_select,
            android_device_list,
            android_package_list,
            android_package_query,
            android_package_get,
            android_companion_install,
            android_companion_update,
            android_vpn_open_consent,
            device_network_profile_list,
            device_network_profile_new,
            device_network_profile_get,
            device_network_profile_apply_intent,
            device_network_profile_save,
            device_network_profile_delete,
            device_network_start,
            device_network_apply,
            device_network_stop,
            device_network_emergency_restore,
            device_network_status,
            workspace_list,
            workspace_get,
            workspace_component_new,
            workspace_component_apply_intent,
            workspace_secret_store_basic,
            workspace_create,
            workspace_copy,
            workspace_select,
            workspace_validate,
            workspace_save,
            workspace_delete,
            workspace_import,
            workspace_export,
            listener_list,
            listener_new,
            listener_copy,
            listener_get,
            listener_save,
            listener_delete,
            listener_statuses,
            listener_overview,
            listener_start,
            listener_stop,
            listener_test_upstream_tls,
            capture_query,
            capture_get_detail,
            capture_clear_view,
            session_query,
            session_get,
            session_export,
            session_clear,
            breakpoint_query,
            breakpoint_get,
            breakpoint_format_json,
            breakpoint_restore_original,
            breakpoint_validate,
            breakpoint_resolve,
            rule_list,
            rule_get,
            rule_new_draft,
            rule_condition_draft,
            rule_action_draft,
            rule_match_field_draft,
            rule_match_operator_draft,
            rule_parse_byte_input,
            rule_parse_header_input,
            rule_create_from_session,
            rule_save,
            rule_copy,
            rule_delete,
            rule_toggle,
            rule_import,
            rule_export,
            fault_template_list,
            fault_configure,
            fault_active_list,
            fault_stop,
            certificate_overview,
            certificate_generate_ca,
            certificate_export_ca,
            certificate_reissue_leaf,
            certificate_import_pkcs12,
            certificate_import_upstream_ca,
            certificate_validate,
            certificate_reset_ca,
            settings_get,
            settings_validate,
            settings_save,
            settings_reset_defaults,
        ])
}
use std::collections::BTreeMap;
