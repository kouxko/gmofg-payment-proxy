use std::{collections::BTreeMap, sync::Arc};

use chrono::Utc;
use http::{HeaderName, HeaderValue};

use crate::{
    ActiveFaultViewModel, AppBootstrapViewModel, AppError, AppResult, BreakpointCoordinator,
    BreakpointDecision, BreakpointDetailViewModel, BreakpointDraft, BreakpointId,
    BreakpointSummaryViewModel, BreakpointValidationPort, BreakpointValidationViewModel,
    CaptureDetailViewModel, CapturePageViewModel, CaptureQuery, CertificateOverviewViewModel,
    CertificateServicePort, CertificateValidationViewModel, EventHub, EventSubscription,
    FaultConfigurationDraft, FaultServicePort, FaultTemplateViewModel, FieldValidationViewModel,
    FileExportPort, OperationResultViewModel, PageRequest, ProxyState, ProxyStatusViewModel,
    ProxySupervisorPort, RuleAction, RuleActionKind, RuleByteInputViewModel, RuleCondition,
    RuleConditionKind, RuleDraft, RuleDropResponseMode, RuleHeaderInputViewModel, RuleId,
    RuleMatchField, RuleMatchFieldKind, RuleMatchOperator, RuleMatchOperatorKind,
    RuleRepositoryPort, RuleSummaryViewModel, RuleTerminalAction, RuleViewModel, RuntimeEpoch,
    SessionDetailViewModel, SessionId, SessionPageViewModel, SessionQuery, SessionQueryPort,
    SettingsDraft, SettingsRepositoryPort, SettingsValidationViewModel, SettingsViewModel,
    UiEventEnvelope, UiEventPayload,
};

#[derive(Debug)]
pub struct Application {
    proxy: Arc<dyn ProxySupervisorPort>,
    capture: Arc<dyn crate::CaptureRepositoryPort>,
    sessions: Arc<dyn SessionQueryPort>,
    breakpoints: Arc<BreakpointCoordinator>,
    breakpoint_validation: Arc<dyn BreakpointValidationPort>,
    rules: Arc<dyn RuleRepositoryPort>,
    faults: Arc<dyn FaultServicePort>,
    certificates: Arc<dyn CertificateServicePort>,
    settings: Arc<dyn SettingsRepositoryPort>,
    file_export: Arc<dyn FileExportPort>,
    events: Arc<EventHub>,
    mutation_gate: tokio::sync::Mutex<()>,
}

impl Application {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        proxy: Arc<dyn ProxySupervisorPort>,
        capture: Arc<dyn crate::CaptureRepositoryPort>,
        sessions: Arc<dyn SessionQueryPort>,
        breakpoints: Arc<BreakpointCoordinator>,
        breakpoint_validation: Arc<dyn BreakpointValidationPort>,
        rules: Arc<dyn RuleRepositoryPort>,
        faults: Arc<dyn FaultServicePort>,
        certificates: Arc<dyn CertificateServicePort>,
        settings: Arc<dyn SettingsRepositoryPort>,
        file_export: Arc<dyn FileExportPort>,
        events: Arc<EventHub>,
    ) -> Self {
        Self {
            proxy,
            capture,
            sessions,
            breakpoints,
            breakpoint_validation,
            rules,
            faults,
            certificates,
            settings,
            file_export,
            events,
            mutation_gate: tokio::sync::Mutex::new(()),
        }
    }

    pub async fn app_bootstrap(&self) -> AppResult<AppBootstrapViewModel> {
        let proxy = self.proxy.status().await?;
        let recent_capture = self
            .capture_query(CaptureQuery {
                keyword: None,
                terminal_ip: None,
                channel: None,
                stage: None,
                result: None,
                rule_id: None,
                exceptions_only: false,
                after_event_id: None,
                sort: crate::CaptureSort::OccurredAt,
                direction: crate::SortDirection::Desc,
                page: PageRequest {
                    page: 1,
                    page_size: 5,
                },
            })
            .await?;
        let pending_breakpoints = self
            .breakpoints
            .query(proxy.runtime_epoch)
            .into_iter()
            .collect();
        let certificate = self.certificates.overview().await?;
        let settings = self.settings.get().await?;
        Ok(AppBootstrapViewModel {
            proxy,
            recent_capture,
            pending_breakpoints,
            certificate,
            settings,
            event_cursor: self.events.current_cursor(),
        })
    }

    pub fn app_subscribe_events(&self, after_event_id: u64) -> AppResult<EventSubscription> {
        self.events.subscribe_default(after_event_id)
    }

    pub fn app_take_subscription_failure(&self, subscription_id: u64) -> Option<UiEventEnvelope> {
        self.events.take_subscription_failure(subscription_id)
    }

    pub fn app_unsubscribe_events(&self, subscription_id: u64) -> OperationResultViewModel {
        self.events.unsubscribe(subscription_id);
        OperationResultViewModel::success("实时事件订阅已取消。")
    }

    /// Gracefully stops the complete runtime during application exit.
    ///
    /// This bypasses the interactive lifecycle guard so an in-flight start or
    /// stop operation is awaited and then fully cleaned up by the supervisor.
    pub async fn app_shutdown(&self) -> AppResult<ProxyStatusViewModel> {
        let _gate = self.mutation_gate.lock().await;
        let before = self.proxy.status().await?;
        let stop_result = if before.state == ProxyState::Stopped {
            Ok(before.clone())
        } else {
            self.proxy.stop().await
        };
        if let Some(epoch) = before.runtime_epoch {
            for summary in self.breakpoints.proxy_stopped(epoch) {
                self.events.publish(
                    Some(epoch),
                    Utc::now(),
                    Some(summary.breakpoint_id.to_string()),
                    Some(summary.revision),
                    UiEventPayload::BreakpointResolved(summary),
                );
            }
        }
        let clear_result = self.settings.clear_effective().await;
        match (stop_result, clear_result) {
            (Ok(status), Ok(_)) => {
                self.publish_runtime(&status);
                Ok(status)
            }
            (Err(stop_error), Ok(_)) => Err(stop_error),
            (Ok(status), Err(clear_error)) => {
                self.publish_runtime(&status);
                Err(clear_error)
            }
            (Err(stop_error), Err(clear_error)) => Err(AppError::new(
                "APP_SHUTDOWN_FAILED",
                format!(
                    "Proxy 停止失败 [{}] {}；生效设置清理失败 [{}] {}。",
                    stop_error.view_model.code,
                    stop_error.view_model.message,
                    clear_error.view_model.code,
                    clear_error.view_model.message
                ),
            )),
        }
    }

    pub async fn proxy_get_status(&self) -> AppResult<ProxyStatusViewModel> {
        self.proxy.status().await
    }

    pub async fn proxy_start(&self) -> AppResult<ProxyStatusViewModel> {
        let _gate = self.mutation_gate.lock().await;
        self.proxy_start_inner().await
    }

    async fn proxy_start_inner(&self) -> AppResult<ProxyStatusViewModel> {
        let current = self.proxy.status().await?;
        match current.state {
            ProxyState::Running => {
                return Err(AppError::new("PROXY_ALREADY_RUNNING", "Proxy 已在运行。"));
            }
            ProxyState::Starting | ProxyState::Stopping => {
                return Err(AppError::new(
                    "OPERATION_IN_PROGRESS",
                    "Proxy 正在启动或停止，请等待当前操作完成。",
                ));
            }
            ProxyState::Stopped | ProxyState::Faulted => {}
        }
        let settings = self.settings.get().await?.stored;
        let status = self.proxy.start(settings.clone()).await?;
        if let Err(error) = self.settings.apply_effective(settings).await {
            return Err(self
                .cleanup_failed_start(error, "Proxy 启动后无法记录生效设置")
                .await);
        }
        self.publish_runtime(&status);
        Ok(status)
    }

    pub async fn proxy_stop(&self) -> AppResult<ProxyStatusViewModel> {
        let _gate = self.mutation_gate.lock().await;
        self.proxy_stop_inner().await
    }

    async fn proxy_stop_inner(&self) -> AppResult<ProxyStatusViewModel> {
        let current = self.proxy.status().await?;
        if current.state == ProxyState::Stopped {
            return Ok(current);
        }
        if matches!(current.state, ProxyState::Starting | ProxyState::Stopping) {
            return Err(AppError::new(
                "OPERATION_IN_PROGRESS",
                "Proxy 正在启动或停止，请等待当前操作完成。",
            ));
        }
        let status = self.proxy.stop().await?;
        let clear_effective_result = self.settings.clear_effective().await;
        if let Some(epoch) = current.runtime_epoch {
            for summary in self.breakpoints.proxy_stopped(epoch) {
                self.events.publish(
                    Some(epoch),
                    Utc::now(),
                    Some(summary.breakpoint_id.to_string()),
                    Some(summary.revision),
                    UiEventPayload::BreakpointResolved(summary),
                );
            }
        }
        self.publish_runtime(&status);
        clear_effective_result?;
        Ok(status)
    }

    pub async fn proxy_restart(&self) -> AppResult<ProxyStatusViewModel> {
        let _gate = self.mutation_gate.lock().await;
        let current = self.proxy.status().await?;
        if matches!(current.state, ProxyState::Starting | ProxyState::Stopping) {
            return Err(AppError::new(
                "OPERATION_IN_PROGRESS",
                "Proxy 正在启动或停止，请等待当前操作完成。",
            ));
        }
        let settings = self.settings.get().await?;
        let restart_settings = if current.state == ProxyState::Running {
            settings.effective.unwrap_or(settings.stored)
        } else {
            settings.stored
        };
        if current.state != ProxyState::Stopped {
            self.proxy_stop_inner().await?;
        }
        let status = self.proxy.start(restart_settings.clone()).await?;
        if let Err(error) = self.settings.apply_effective(restart_settings).await {
            return Err(self
                .cleanup_failed_start(error, "Proxy 重启后无法记录生效设置")
                .await);
        }
        self.publish_runtime(&status);
        Ok(status)
    }

    pub async fn capture_query(&self, mut query: CaptureQuery) -> AppResult<CapturePageViewModel> {
        query.keyword = normalized_optional(query.keyword);
        query.terminal_ip = normalized_optional(query.terminal_ip);
        query.result = normalized_optional(query.result);
        query.page = query.page.normalized();
        self.capture.query(query).await
    }

    pub async fn capture_get_detail(
        &self,
        session_id: SessionId,
        runtime_epoch: RuntimeEpoch,
    ) -> AppResult<CaptureDetailViewModel> {
        self.capture.get_detail(session_id, runtime_epoch).await
    }

    pub async fn capture_clear_view(&self, current_cursor: u64) -> AppResult<u64> {
        self.capture.clear_view(current_cursor).await
    }

    pub async fn session_query(&self, mut query: SessionQuery) -> AppResult<SessionPageViewModel> {
        query.keyword = normalized_optional(query.keyword);
        query.terminal_ip = normalized_optional(query.terminal_ip);
        query.result = normalized_optional(query.result);
        query.page = query.page.normalized();
        self.sessions.query(query).await
    }

    pub async fn session_get(&self, session_id: SessionId) -> AppResult<SessionDetailViewModel> {
        self.sessions.get(session_id).await
    }

    pub async fn session_export(
        &self,
        session_id: SessionId,
        sensitive_data_confirmed: bool,
    ) -> AppResult<OperationResultViewModel> {
        if !sensitive_data_confirmed {
            return Err(AppError::new(
                "EXPORT_CONFIRMATION_REQUIRED",
                "导出文件包含原始敏感数据，请确认后再导出。",
            ));
        }
        let session = self.sessions.get(session_id).await?;
        self.file_export
            .export_session(session, sensitive_data_confirmed)
            .await
    }

    pub async fn session_clear(&self, confirmed: bool) -> AppResult<OperationResultViewModel> {
        if !confirmed {
            return Err(AppError::new(
                "CONFIRMATION_REQUIRED",
                "清空已完成会话需要确认。",
            ));
        }
        let count = self.sessions.clear_completed().await?;
        Ok(OperationResultViewModel::success(format!(
            "已清空 {count} 个已完成会话，待处理断点未受影响。"
        )))
    }

    pub fn breakpoint_query(
        &self,
        runtime_epoch: Option<RuntimeEpoch>,
    ) -> Vec<BreakpointSummaryViewModel> {
        self.breakpoints.query(runtime_epoch)
    }

    pub fn breakpoint_get(
        &self,
        breakpoint_id: BreakpointId,
        runtime_epoch: RuntimeEpoch,
    ) -> AppResult<BreakpointDetailViewModel> {
        self.breakpoints.get(breakpoint_id, runtime_epoch)
    }

    pub fn breakpoint_format_json(&self, draft: BreakpointDraft) -> AppResult<BreakpointDraft> {
        self.breakpoint_validation.format_json(draft)
    }

    pub fn breakpoint_restore_original(
        &self,
        breakpoint_id: BreakpointId,
        runtime_epoch: RuntimeEpoch,
    ) -> AppResult<BreakpointDraft> {
        let detail = self.breakpoints.get(breakpoint_id, runtime_epoch)?;
        self.breakpoint_validation.restore_original(&detail)
    }

    pub fn breakpoint_validate(
        &self,
        draft: &BreakpointDraft,
        runtime_epoch: RuntimeEpoch,
    ) -> AppResult<BreakpointValidationViewModel> {
        let detail = self.breakpoints.get(draft.breakpoint_id, runtime_epoch)?;
        self.breakpoint_validation.validate(&detail, draft)
    }

    pub async fn breakpoint_resolve(
        &self,
        runtime_epoch: RuntimeEpoch,
        mut decision: BreakpointDecision,
    ) -> AppResult<BreakpointSummaryViewModel> {
        let status = self.proxy.status().await?;
        if status.state != ProxyState::Running || status.runtime_epoch != Some(runtime_epoch) {
            return Err(AppError::new(
                "BREAKPOINT_PROXY_STOPPED",
                "Proxy 未在对应运行周期中运行，不能处理断点。",
            )
            .epoch(runtime_epoch));
        }
        let detail = self
            .breakpoints
            .get(decision.breakpoint_id, runtime_epoch)?;
        if matches!(
            decision.kind,
            crate::BreakpointDecisionKind::ForwardModified
                | crate::BreakpointDecisionKind::MockResponse
        ) {
            let draft = BreakpointDraft {
                breakpoint_id: decision.breakpoint_id,
                expected_revision: decision.expected_revision,
                message: decision.message.clone().ok_or_else(|| {
                    AppError::field(
                        "CONFIG_INVALID",
                        "该断点处理方式必须提供报文。",
                        std::collections::BTreeMap::from([(
                            "message".into(),
                            vec!["该操作必须提供报文。".into()],
                        )]),
                    )
                })?,
            };
            decision.message = Some(self.breakpoint_validation.format_json(draft)?.message);
        }
        let validation = self
            .breakpoint_validation
            .validate_decision(&detail, &decision)?;
        ensure_valid("CONFIG_INVALID", "断点决策校验失败。", &validation)?;
        let summary = self.breakpoints.resolve(runtime_epoch, decision)?;
        self.events.publish(
            Some(runtime_epoch),
            Utc::now(),
            Some(summary.breakpoint_id.to_string()),
            Some(summary.revision),
            UiEventPayload::BreakpointResolved(summary.clone()),
        );
        Ok(summary)
    }

    pub async fn rule_list(&self) -> AppResult<Vec<RuleSummaryViewModel>> {
        let mut rules = self.rules.list().await?;
        rules.sort_by_key(|rule| (rule.priority, rule.creation_order, rule.rule_id));
        Ok(rules)
    }

    pub async fn rule_get(&self, rule_id: RuleId) -> AppResult<RuleViewModel> {
        self.rules.get(rule_id).await
    }

    pub async fn rule_new_draft(&self) -> AppResult<RuleDraft> {
        self.rules.new_draft().await
    }

    pub fn rule_condition_draft(&self, kind: RuleConditionKind) -> RuleCondition {
        match kind {
            RuleConditionKind::Field => RuleCondition::Field {
                field: RuleMatchField::PathOrRequestType,
                operator: RuleMatchOperator::Equals {
                    value: String::new(),
                },
            },
            RuleConditionKind::NthHit => RuleCondition::NthHit { count: 1 },
        }
    }

    pub fn rule_match_field_draft(&self, kind: RuleMatchFieldKind) -> RuleMatchField {
        match kind {
            RuleMatchFieldKind::TerminalIp => RuleMatchField::TerminalIp,
            RuleMatchFieldKind::CertificateFingerprint => RuleMatchField::CertificateFingerprint,
            RuleMatchFieldKind::PathOrRequestType => RuleMatchField::PathOrRequestType,
            RuleMatchFieldKind::JsonPath => RuleMatchField::JsonPath { path: "$".into() },
        }
    }

    pub fn rule_match_operator_draft(&self, kind: RuleMatchOperatorKind) -> RuleMatchOperator {
        match kind {
            RuleMatchOperatorKind::Equals => RuleMatchOperator::Equals {
                value: String::new(),
            },
            RuleMatchOperatorKind::Contains => RuleMatchOperator::Contains {
                value: String::new(),
            },
            RuleMatchOperatorKind::Regex => RuleMatchOperator::Regex {
                pattern: String::new(),
            },
        }
    }

    pub fn rule_action_draft(&self, kind: RuleActionKind) -> RuleAction {
        match kind {
            RuleActionKind::SetJsonField => RuleAction::SetJsonField {
                path: "$.field".into(),
                value_json: "null".into(),
            },
            RuleActionKind::ReplaceBodyText => RuleAction::ReplaceBodyText {
                text: String::new(),
            },
            RuleActionKind::SetHeader => RuleAction::SetHeader {
                name: "x-gmofg-test".into(),
                value: String::new(),
            },
            RuleActionKind::Delay => RuleAction::Delay { milliseconds: 100 },
            RuleActionKind::Pause => RuleAction::Pause,
            RuleActionKind::CustomHttpStatus => RuleAction::CustomHttpStatus { status: 500 },
            RuleActionKind::RejectTlsHandshake => RuleAction::Terminal {
                action: RuleTerminalAction::RejectTlsHandshake,
            },
            RuleActionKind::DisconnectBeforeUpstream => RuleAction::Terminal {
                action: RuleTerminalAction::DisconnectBeforeUpstream,
            },
            RuleActionKind::UpstreamConnectTimeout => RuleAction::Terminal {
                action: RuleTerminalAction::UpstreamConnectTimeout {
                    milliseconds: 1_000,
                },
            },
            RuleActionKind::UpstreamWriteTimeout => RuleAction::Terminal {
                action: RuleTerminalAction::UpstreamWriteTimeout {
                    milliseconds: 1_000,
                },
            },
            RuleActionKind::UpstreamReadTimeout => RuleAction::Terminal {
                action: RuleTerminalAction::UpstreamReadTimeout {
                    milliseconds: 1_000,
                },
            },
            RuleActionKind::DropUpstreamResponse => RuleAction::Terminal {
                action: RuleTerminalAction::DropUpstreamResponse {
                    mode: RuleDropResponseMode::ReadCompleteResponse,
                },
            },
            RuleActionKind::MockResponse => RuleAction::Terminal {
                action: RuleTerminalAction::MockResponse {
                    status: 200,
                    headers: vec![("content-type".into(), "application/json".into())],
                    shift_jis_body: b"{}".to_vec(),
                },
            },
            RuleActionKind::InvalidJson => RuleAction::Terminal {
                action: RuleTerminalAction::InvalidJson {
                    shift_jis_body: b"{".to_vec(),
                },
            },
            RuleActionKind::IncorrectContentLength => RuleAction::Terminal {
                action: RuleTerminalAction::IncorrectContentLength { delta: 1 },
            },
            RuleActionKind::TruncateResponse => RuleAction::Terminal {
                action: RuleTerminalAction::TruncateResponse { bytes: 1 },
            },
        }
    }

    pub fn rule_parse_byte_input(&self, raw: &str) -> AppResult<RuleByteInputViewModel> {
        let trimmed = raw.trim();
        if trimmed.is_empty() {
            return Ok(RuleByteInputViewModel {
                bytes: Vec::new(),
                normalized: String::new(),
            });
        }
        let mut bytes = Vec::new();
        for (index, part) in trimmed.split(',').enumerate() {
            let value = part.trim().parse::<u8>().map_err(|_| {
                AppError::field(
                    "RULE_INVALID",
                    "字节输入必须是以逗号分隔的 0 到 255 十进制整数。",
                    BTreeMap::from([(
                        "raw".into(),
                        vec![format!(
                            "第 {} 项“{}”不是有效字节。",
                            index + 1,
                            part.trim()
                        )],
                    )]),
                )
            })?;
            bytes.push(value);
        }
        Ok(RuleByteInputViewModel {
            normalized: bytes
                .iter()
                .map(u8::to_string)
                .collect::<Vec<_>>()
                .join(", "),
            bytes,
        })
    }

    pub fn rule_parse_header_input(&self, raw: &str) -> AppResult<RuleHeaderInputViewModel> {
        let mut headers = Vec::new();
        for (index, line) in raw.lines().enumerate() {
            let line = line.trim();
            if line.is_empty() {
                continue;
            }
            let Some((raw_name, raw_value)) = line.split_once(':') else {
                return Err(AppError::field(
                    "RULE_INVALID",
                    "响应 Header 必须使用每行“name: value”的格式。",
                    BTreeMap::from([(
                        "raw".into(),
                        vec![format!("第 {} 行缺少冒号分隔符。", index + 1)],
                    )]),
                ));
            };
            let name = raw_name.trim();
            let value = raw_value.trim();
            HeaderName::from_bytes(name.as_bytes()).map_err(|_| {
                AppError::field(
                    "RULE_INVALID",
                    "响应 Header 名称无效。",
                    BTreeMap::from([(
                        "raw".into(),
                        vec![format!("第 {} 行包含无效 Header 名称。", index + 1)],
                    )]),
                )
            })?;
            HeaderValue::from_bytes(value.as_bytes()).map_err(|_| {
                AppError::field(
                    "RULE_INVALID",
                    "响应 Header 值无效。",
                    BTreeMap::from([(
                        "raw".into(),
                        vec![format!("第 {} 行包含无效 Header 值。", index + 1)],
                    )]),
                )
            })?;
            headers.push((name.to_ascii_lowercase(), value.to_owned()));
        }
        let normalized = headers
            .iter()
            .map(|(name, value)| format!("{name}: {value}"))
            .collect::<Vec<_>>()
            .join("\n");
        Ok(RuleHeaderInputViewModel {
            headers,
            normalized,
        })
    }

    pub async fn rule_create_from_session(&self, session_id: SessionId) -> AppResult<RuleDraft> {
        self.sessions.get(session_id).await?;
        self.rules.create_from_session(session_id).await
    }

    pub async fn rule_save(&self, mut draft: RuleDraft) -> AppResult<RuleViewModel> {
        let _gate = self.mutation_gate.lock().await;
        self.ensure_rule_or_fault_write_allowed().await?;
        draft.name = draft.name.trim().to_owned();
        draft.description = draft.description.trim().to_owned();
        let validation = self.rules.validate(&draft).await?;
        ensure_valid("RULE_INVALID", "规则配置校验失败。", &validation)?;
        self.rules.save(draft).await
    }

    pub async fn rule_copy(&self, rule_id: RuleId) -> AppResult<RuleViewModel> {
        let _gate = self.mutation_gate.lock().await;
        self.ensure_rule_or_fault_write_allowed().await?;
        self.rules.copy(rule_id).await
    }

    pub async fn rule_delete(
        &self,
        rule_id: RuleId,
        expected_revision: u64,
        confirmed: bool,
    ) -> AppResult<OperationResultViewModel> {
        let _gate = self.mutation_gate.lock().await;
        require_confirmation(confirmed, "删除规则需要确认。")?;
        self.ensure_rule_or_fault_write_allowed().await?;
        self.rules.delete(rule_id, expected_revision).await
    }

    pub async fn rule_toggle(
        &self,
        rule_id: RuleId,
        expected_revision: u64,
        enabled: bool,
    ) -> AppResult<RuleViewModel> {
        let _gate = self.mutation_gate.lock().await;
        self.ensure_rule_or_fault_write_allowed().await?;
        self.rules.toggle(rule_id, expected_revision, enabled).await
    }

    pub async fn rule_import(&self) -> AppResult<OperationResultViewModel> {
        let _gate = self.mutation_gate.lock().await;
        self.ensure_rule_or_fault_write_allowed().await?;
        self.rules.import().await
    }

    pub async fn rule_export(&self) -> AppResult<OperationResultViewModel> {
        self.rules.export().await
    }

    pub async fn fault_template_list(&self) -> AppResult<Vec<FaultTemplateViewModel>> {
        let mut templates = self.faults.templates().await?;
        templates.sort_by(|left, right| left.template_id.cmp(&right.template_id));
        Ok(templates)
    }

    pub async fn fault_configure(
        &self,
        draft: FaultConfigurationDraft,
    ) -> AppResult<ActiveFaultViewModel> {
        let _gate = self.mutation_gate.lock().await;
        self.ensure_rule_or_fault_write_allowed().await?;
        self.faults.configure(draft).await
    }

    pub async fn fault_active_list(&self) -> AppResult<Vec<ActiveFaultViewModel>> {
        let mut active = self.faults.active().await?;
        active.sort_by_key(|fault| (fault.priority, fault.rule_id));
        Ok(active)
    }

    pub async fn fault_stop(
        &self,
        rule_id: RuleId,
        expected_revision: u64,
        confirmed: bool,
    ) -> AppResult<ActiveFaultViewModel> {
        let _gate = self.mutation_gate.lock().await;
        require_confirmation(confirmed, "停止活动故障需要确认。")?;
        self.ensure_rule_or_fault_write_allowed().await?;
        self.faults.stop(rule_id, expected_revision).await
    }

    pub async fn certificate_overview(&self) -> AppResult<CertificateOverviewViewModel> {
        self.certificates.overview().await
    }

    pub async fn certificate_generate_ca(
        &self,
        sans: Vec<String>,
    ) -> AppResult<CertificateOverviewViewModel> {
        let _gate = self.mutation_gate.lock().await;
        self.ensure_proxy_stopped_for_write().await?;
        let overview = self.certificates.generate_ca(normalize_sans(sans)).await?;
        self.publish_certificate(&overview);
        Ok(overview)
    }

    pub async fn certificate_export_ca(&self) -> AppResult<OperationResultViewModel> {
        self.certificates.export_ca().await
    }

    pub async fn certificate_reissue_leaf(
        &self,
        expected_revision: u64,
        sans: Vec<String>,
    ) -> AppResult<CertificateOverviewViewModel> {
        let _gate = self.mutation_gate.lock().await;
        self.ensure_proxy_stopped_for_write().await?;
        let overview = self
            .certificates
            .reissue_leaf(expected_revision, normalize_sans(sans))
            .await?;
        self.publish_certificate(&overview);
        Ok(overview)
    }

    pub async fn certificate_import_pkcs12(
        &self,
        password: String,
    ) -> AppResult<CertificateOverviewViewModel> {
        let _gate = self.mutation_gate.lock().await;
        self.ensure_proxy_stopped_for_write().await?;
        let overview = self.certificates.import_pkcs12(password).await?;
        self.publish_certificate(&overview);
        Ok(overview)
    }

    pub async fn certificate_import_upstream_ca(&self) -> AppResult<CertificateOverviewViewModel> {
        let _gate = self.mutation_gate.lock().await;
        self.ensure_proxy_stopped_for_write().await?;
        let overview = self.certificates.import_upstream_ca().await?;
        self.publish_certificate(&overview);
        Ok(overview)
    }

    pub async fn certificate_validate(&self) -> AppResult<CertificateValidationViewModel> {
        self.certificates.validate().await
    }

    pub async fn certificate_reset_ca(
        &self,
        expected_revision: u64,
        confirmed: bool,
    ) -> AppResult<CertificateOverviewViewModel> {
        require_confirmation(
            confirmed,
            "重置本地 CA 后所有 Payment 终端都需要重新导入新 CA。",
        )?;
        let _gate = self.mutation_gate.lock().await;
        self.ensure_proxy_stopped_for_write().await?;
        let overview = self.certificates.reset_ca(expected_revision).await?;
        self.publish_certificate(&overview);
        Ok(overview)
    }

    pub async fn settings_get(&self) -> AppResult<SettingsViewModel> {
        self.settings.get().await
    }

    pub async fn settings_validate(
        &self,
        draft: SettingsDraft,
    ) -> AppResult<SettingsValidationViewModel> {
        let draft = normalize_settings(draft);
        let mut validation = validate_settings_locally(&draft);
        if !validation.valid {
            return Ok(validation);
        }
        validation = self.settings.validate(&draft).await?;
        if !validation.valid {
            return Ok(validation);
        }

        let certificate_overview = self.certificates.overview().await?;
        if certificate_overview.items.is_empty() {
            validation
                .warnings
                .push("证书材料尚未配置；保存设置后请先完成证书配置再启动 Proxy。".into());
            return Ok(validation);
        }

        let certificate_validation = self.certificates.validate().await?;
        for (field, messages) in certificate_validation.field_errors {
            validation
                .field_errors
                .insert(format!("certificates.{field}"), messages);
        }
        let leaf_sans = certificate_overview
            .items
            .iter()
            .find(|item| item.usage.contains("App → Proxy"))
            .map(|item| &item.sans);
        if leaf_sans.is_none_or(|sans| {
            draft
                .leaf_sans
                .iter()
                .any(|required| !sans.contains(required))
        }) {
            push_error(
                &mut validation.field_errors,
                "leaf_sans",
                "当前 Proxy 叶子证书 SAN 未覆盖设置中要求的全部地址。",
            );
        }
        validation.valid = validation.field_errors.is_empty();
        Ok(validation)
    }

    pub async fn settings_validate_input(
        &self,
        mut draft: SettingsDraft,
        leaf_sans_raw: String,
    ) -> AppResult<SettingsValidationViewModel> {
        draft.leaf_sans = parse_sans_raw(&leaf_sans_raw);
        self.settings_validate(draft).await
    }

    pub async fn settings_save(&self, draft: SettingsDraft) -> AppResult<SettingsViewModel> {
        let _gate = self.mutation_gate.lock().await;
        self.settings_save_inner(draft).await
    }

    async fn settings_save_inner(&self, draft: SettingsDraft) -> AppResult<SettingsViewModel> {
        self.ensure_settings_write_allowed().await?;
        let draft = normalize_settings(draft);
        let validation = self.settings_validate(draft.clone()).await?;
        ensure_valid("CONFIG_INVALID", "设置校验失败。", &validation)?;
        self.settings.save(draft).await
    }

    pub async fn settings_save_input(
        &self,
        mut draft: SettingsDraft,
        leaf_sans_raw: String,
    ) -> AppResult<SettingsViewModel> {
        draft.leaf_sans = parse_sans_raw(&leaf_sans_raw);
        self.settings_save(draft).await
    }

    pub async fn settings_save_and_restart(
        &self,
        draft: SettingsDraft,
    ) -> AppResult<SettingsViewModel> {
        let _gate = self.mutation_gate.lock().await;
        self.ensure_settings_write_allowed().await?;
        let old_settings = self.settings.get().await?;
        let old_status = self.proxy.status().await?;
        let was_running = old_status.state == ProxyState::Running;
        let saved = self.settings_save_inner(draft).await?;
        if !was_running {
            return Ok(saved);
        }

        self.proxy_stop_inner().await?;
        let candidate_status = match self.proxy.start(saved.stored.clone()).await {
            Ok(status) => status,
            Err(error) => {
                return self
                    .rollback_settings_and_recover(old_settings, error, None)
                    .await;
            }
        };
        match self.settings.apply_effective(saved.stored).await {
            Ok(_) => {
                self.publish_runtime(&candidate_status);
                self.settings.get().await
            }
            Err(apply_error) => match self.proxy.stop().await {
                Ok(stopped) => {
                    self.publish_runtime(&stopped);
                    let clear_error = self.settings.clear_effective().await.err();
                    self.rollback_settings_and_recover(old_settings, apply_error, clear_error)
                        .await
                }
                Err(stop_error) => {
                    let restore = self.settings.restore(old_settings).await;
                    let restore_text = restore.map_or_else(
                        |error| format!("旧设置恢复失败：{}", error.view_model.message),
                        |_| "旧设置数据库已恢复，但运行状态未恢复".to_owned(),
                    );
                    Err(AppError::new(
                        "CONFIG_ROLLBACK_FAILED",
                        format!(
                            "候选 Proxy 已启动，但生效设置记录失败且无法停止；原始错误 [{}] {}；停止错误 [{}] {}；{restore_text}。",
                            apply_error.view_model.code,
                            apply_error.view_model.message,
                            stop_error.view_model.code,
                            stop_error.view_model.message
                        ),
                    )
                    .retryable("请立即检查 Proxy 实际监听状态，停止后再恢复配置。"))
                }
            },
        }
    }

    pub async fn settings_save_and_restart_input(
        &self,
        mut draft: SettingsDraft,
        leaf_sans_raw: String,
    ) -> AppResult<SettingsViewModel> {
        draft.leaf_sans = parse_sans_raw(&leaf_sans_raw);
        self.settings_save_and_restart(draft).await
    }

    pub async fn settings_reset_defaults(&self, confirmed: bool) -> AppResult<SettingsDraft> {
        require_confirmation(confirmed, "恢复默认设置需要确认。")?;
        self.ensure_settings_write_allowed().await?;
        Ok(SettingsDraft::default())
    }

    async fn rollback_settings_and_recover(
        &self,
        old_settings: SettingsViewModel,
        mut candidate_error: AppError,
        cleanup_error: Option<AppError>,
    ) -> AppResult<SettingsViewModel> {
        let restored = self
            .settings
            .restore(old_settings.clone())
            .await
            .map_err(|error| {
                AppError::new(
                    "CONFIG_ROLLBACK_FAILED",
                    format!(
                        "候选设置失败 [{}] {}；旧设置数据库恢复失败 [{}] {}。",
                        candidate_error.view_model.code,
                        candidate_error.view_model.message,
                        error.view_model.code,
                        error.view_model.message
                    ),
                )
                .retryable("Proxy 当前保持停止；请检查设置存储后手动恢复。")
            })?;
        let recovery_settings = old_settings
            .effective
            .unwrap_or_else(|| restored.stored.clone());
        let recovery_status =
            self.proxy
                .start(recovery_settings.clone())
                .await
                .map_err(|error| {
                    AppError::new(
                        "CONFIG_ROLLBACK_FAILED",
                        format!(
                            "候选设置失败 [{}] {}；旧设置已恢复，但旧 Proxy 启动失败 [{}] {}。",
                            candidate_error.view_model.code,
                            candidate_error.view_model.message,
                            error.view_model.code,
                            error.view_model.message
                        ),
                    )
                    .retryable("请检查旧配置的端口和证书；Proxy 当前未恢复运行。")
                })?;
        if let Err(error) = self.settings.apply_effective(recovery_settings).await {
            let cleanup = self
                .cleanup_failed_start(error.clone(), "旧 Proxy 恢复后无法记录生效设置")
                .await;
            return Err(AppError::new(
                "CONFIG_ROLLBACK_FAILED",
                format!(
                    "候选设置失败 [{}] {}；旧 Proxy 虽已启动，但恢复事务未完成：{}。",
                    candidate_error.view_model.code,
                    candidate_error.view_model.message,
                    cleanup.view_model.message
                ),
            )
            .retryable("请检查 Proxy 实际状态和设置存储。"));
        }
        self.publish_runtime(&recovery_status);
        let cleanup_note = cleanup_error.map_or_else(String::new, |error| {
            format!(
                "；候选清理附加错误 [{}] {}",
                error.view_model.code, error.view_model.message
            )
        });
        candidate_error.view_model.message = format!(
            "新设置未生效，旧设置和运行状态已恢复：{}{}。",
            candidate_error.view_model.message, cleanup_note
        );
        candidate_error.view_model.retryable = true;
        candidate_error.view_model.suggested_action = Some("请按原错误码检查新设置后重试。".into());
        Err(candidate_error)
    }

    fn publish_runtime(&self, status: &ProxyStatusViewModel) {
        self.events.publish(
            status.runtime_epoch,
            Utc::now(),
            None,
            Some(status.revision),
            UiEventPayload::RuntimeStatusChanged(Box::new(status.clone())),
        );
    }

    fn publish_certificate(&self, overview: &CertificateOverviewViewModel) {
        self.events.publish(
            None,
            Utc::now(),
            Some("certificates".into()),
            Some(overview.revision),
            UiEventPayload::CertificateStatusChanged(overview.clone()),
        );
    }

    async fn cleanup_failed_start(&self, primary: AppError, context: &str) -> AppError {
        let stop = self.proxy.stop().await;
        let clear = if stop.is_ok() {
            self.settings.clear_effective().await.map(|_| ())
        } else {
            Ok(())
        };
        match (stop, clear) {
            (Ok(status), Ok(())) => {
                self.publish_runtime(&status);
                primary
            }
            (stop, clear) => {
                let stop_message = stop
                    .err()
                    .map_or_else(|| "停止成功".to_owned(), |error| error.view_model.message);
                let clear_message = clear.err().map_or_else(
                    || "生效快照清理成功或未执行".to_owned(),
                    |error| error.view_model.message,
                );
                AppError::new(
                    "PROXY_START_CLEANUP_FAILED",
                    format!(
                        "{context}：{}；清理结果：{stop_message}；{clear_message}。",
                        primary.view_model.message
                    ),
                )
                .retryable("请先确认 Proxy 实际状态，再停止 Proxy 并检查设置存储。")
            }
        }
    }

    async fn ensure_proxy_stopped_for_write(&self) -> AppResult<()> {
        let state = self.proxy.status().await?.state;
        if state != ProxyState::Stopped {
            return Err(AppError::new(
                "OPERATION_IN_PROGRESS",
                "只有 Proxy 已停止时才能变更证书。",
            ));
        }
        Ok(())
    }

    async fn ensure_settings_write_allowed(&self) -> AppResult<()> {
        let state = self.proxy.status().await?.state;
        if matches!(state, ProxyState::Starting | ProxyState::Stopping) {
            return Err(AppError::new(
                "OPERATION_IN_PROGRESS",
                "Proxy 正在启动或停止，暂时不能修改设置。",
            ));
        }
        Ok(())
    }

    async fn ensure_rule_or_fault_write_allowed(&self) -> AppResult<()> {
        let state = self.proxy.status().await?.state;
        if matches!(state, ProxyState::Starting | ProxyState::Stopping) {
            return Err(AppError::new(
                "OPERATION_IN_PROGRESS",
                "Proxy 正在启动或停止，暂时不能修改规则或故障配置。",
            ));
        }
        Ok(())
    }
}

fn normalized_optional(value: Option<String>) -> Option<String> {
    value
        .map(|value| value.trim().to_owned())
        .filter(|value| !value.is_empty())
}

fn normalize_sans(values: Vec<String>) -> Vec<String> {
    let mut values = values
        .into_iter()
        .map(|value| value.trim().to_lowercase())
        .filter(|value| !value.is_empty())
        .collect::<Vec<_>>();
    values.sort();
    values.dedup();
    values
}

fn parse_sans_raw(raw: &str) -> Vec<String> {
    normalize_sans(raw.split([',', '，']).map(ToOwned::to_owned).collect())
}

fn normalize_settings(mut draft: SettingsDraft) -> SettingsDraft {
    draft.bind_address = draft.bind_address.trim().to_owned();
    draft.upstream_transaction_url = draft.upstream_transaction_url.trim().to_owned();
    draft.upstream_dll_url = draft.upstream_dll_url.trim().to_owned();
    draft.leaf_sans = normalize_sans(draft.leaf_sans);
    draft
}

fn validate_settings_locally(draft: &SettingsDraft) -> SettingsValidationViewModel {
    let mut field_errors = BTreeMap::new();
    if !draft.transaction_enabled && !draft.dll_enabled {
        push_error(&mut field_errors, "channels", "至少启用一个代理通道。");
    }
    if draft.transaction_enabled && draft.dll_enabled && draft.transaction_port == draft.dll_port {
        push_error(
            &mut field_errors,
            "dll_port",
            "交易端口和 DLL 端口不能相同。",
        );
    }
    if draft.bind_address.is_empty() {
        push_error(&mut field_errors, "bind_address", "绑定地址不能为空。");
    }
    if draft.transaction_enabled && !is_https_url_with_host(&draft.upstream_transaction_url) {
        push_error(
            &mut field_errors,
            "upstream_transaction_url",
            "上游交易 URL 必须是包含主机名的 HTTPS URL。",
        );
    }
    if draft.dll_enabled && !is_https_url_with_host(&draft.upstream_dll_url) {
        push_error(
            &mut field_errors,
            "upstream_dll_url",
            "上游 DLL URL 必须是包含主机名的 HTTPS URL。",
        );
    }
    for (field, timeout) in [
        ("connect_timeout_seconds", draft.connect_timeout_seconds),
        ("write_timeout_seconds", draft.write_timeout_seconds),
        ("read_timeout_seconds", draft.read_timeout_seconds),
    ] {
        if timeout == 0 || timeout > 600 {
            push_error(&mut field_errors, field, "超时必须位于 1 到 600 秒之间。");
        }
    }
    if draft.max_body_bytes == 0 || draft.max_body_bytes > 64 * 1024 * 1024 {
        push_error(
            &mut field_errors,
            "max_body_bytes",
            "单个 Body 上限必须位于 1 字节到 64 MiB 之间。",
        );
    }
    if draft.max_sessions == 0 {
        push_error(&mut field_errors, "max_sessions", "会话容量必须至少为 1。");
    }
    if draft.max_memory_bytes == 0 {
        push_error(
            &mut field_errors,
            "max_memory_bytes",
            "内存容量必须至少为 1 字节。",
        );
    }
    SettingsValidationViewModel {
        valid: field_errors.is_empty(),
        field_errors,
        warnings: Vec::new(),
    }
}

fn is_https_url_with_host(value: &str) -> bool {
    gmofg_proxy_domain::is_valid_https_upstream_url(value)
}

fn push_error(errors: &mut BTreeMap<String, Vec<String>>, field: &str, message: &str) {
    errors
        .entry(field.to_owned())
        .or_default()
        .push(message.to_owned());
}

fn ensure_valid(code: &str, message: &str, validation: &FieldValidationViewModel) -> AppResult<()> {
    if validation.valid {
        Ok(())
    } else {
        Err(AppError::field(
            code,
            message,
            validation.field_errors.clone(),
        ))
    }
}

fn require_confirmation(confirmed: bool, message: &str) -> AppResult<()> {
    if confirmed {
        Ok(())
    } else {
        Err(AppError::new("CONFIRMATION_REQUIRED", message))
    }
}
