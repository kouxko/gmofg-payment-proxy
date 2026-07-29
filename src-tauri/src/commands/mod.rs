use gmofg_proxy_application::{
    ActiveFaultViewModel, AppBootstrapViewModel, AppError, AppErrorViewModel, BreakpointDecision,
    BreakpointDetailViewModel, BreakpointDraft, BreakpointId, BreakpointSummaryViewModel,
    BreakpointValidationViewModel, CaptureDetailViewModel, CapturePageViewModel, CaptureQuery,
    CertificateOverviewViewModel, CertificateValidationViewModel, FaultConfigurationDraft,
    FaultTemplateViewModel, OperationResultViewModel, ProxyStatusViewModel, RuleAction,
    RuleActionKind, RuleByteInputViewModel, RuleCondition, RuleConditionKind, RuleDraft,
    RuleHeaderInputViewModel, RuleId, RuleMatchField, RuleMatchFieldKind, RuleMatchOperator,
    RuleMatchOperatorKind, RuleSummaryViewModel, RuleViewModel, RuntimeEpoch,
    SessionDetailViewModel, SessionId, SessionPageViewModel, SessionQuery, SettingsDraft,
    SettingsValidationViewModel, SettingsViewModel, SubscriptionAckViewModel, UiEventEnvelope,
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
    for event in subscription.replay.drain(..) {
        on_event.send(event).map_err(|_| AppErrorViewModel {
            code: "CHANNEL_SEND_FAILED".to_owned(),
            message: "实时事件通道已关闭。".to_owned(),
            field_errors: BTreeMap::default(),
            retryable: true,
            suggested_action: Some("请重新获取应用快照并订阅事件。".to_owned()),
            entity_id: None,
            runtime_epoch: None,
        })?;
    }
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
pub async fn proxy_get_status(state: State<'_, AppState>) -> CommandResult<ProxyStatusViewModel> {
    state
        .application
        .proxy_get_status()
        .await
        .map_err(command_error)
}

#[tauri::command]
#[specta::specta]
pub async fn proxy_start(state: State<'_, AppState>) -> CommandResult<ProxyStatusViewModel> {
    state.application.proxy_start().await.map_err(command_error)
}

#[tauri::command]
#[specta::specta]
pub async fn proxy_stop(state: State<'_, AppState>) -> CommandResult<ProxyStatusViewModel> {
    state.application.proxy_stop().await.map_err(command_error)
}

#[tauri::command]
#[specta::specta]
pub async fn proxy_restart(state: State<'_, AppState>) -> CommandResult<ProxyStatusViewModel> {
    state
        .application
        .proxy_restart()
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
    leaf_sans_raw: String,
) -> CommandResult<SettingsValidationViewModel> {
    state
        .application
        .settings_validate_input(draft, leaf_sans_raw)
        .await
        .map_err(command_error)
}

#[tauri::command]
#[specta::specta]
pub async fn settings_save(
    state: State<'_, AppState>,
    draft: SettingsDraft,
    leaf_sans_raw: String,
) -> CommandResult<SettingsViewModel> {
    state
        .application
        .settings_save_input(draft, leaf_sans_raw)
        .await
        .map_err(command_error)
}

#[tauri::command]
#[specta::specta]
pub async fn settings_save_and_restart(
    state: State<'_, AppState>,
    draft: SettingsDraft,
    leaf_sans_raw: String,
) -> CommandResult<SettingsViewModel> {
    state
        .application
        .settings_save_and_restart_input(draft, leaf_sans_raw)
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
            proxy_get_status,
            proxy_start,
            proxy_stop,
            proxy_restart,
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
            settings_save_and_restart,
            settings_reset_defaults,
        ])
}
use std::collections::BTreeMap;
