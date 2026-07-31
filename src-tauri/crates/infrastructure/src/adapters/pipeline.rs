//! Production bridge between the transport runtime and application/domain services.
//!
//! The adapter deliberately lives in infrastructure: it is the only layer that
//! knows both the byte-preserving proxy message and the application/domain view
//! models. Tauri only composes this adapter and never executes pipeline policy.

use std::{
    collections::{BTreeMap, HashMap},
    sync::{
        Arc,
        atomic::{AtomicU64, Ordering},
    },
    time::Duration,
};

use async_trait::async_trait;
use bytes::Bytes;
use chrono::{DateTime, Utc};
use gmofg_proxy_application::{
    AppError, AppResult, BreakpointCoordinator, BreakpointDecision, BreakpointDecisionKind,
    BreakpointDetailViewModel, BreakpointOutcome, BreakpointState, BreakpointSummaryViewModel,
    CaptureRowViewModel, ChannelId as AppChannelId, DisabledReason, EventHub, InMemorySessionStore,
    MessageContentViewModel, MessageStage as AppMessageStage, SessionDetailViewModel,
    SessionRecord, SessionStore, SessionSummaryViewModel, UiEventPayload, UiTone,
};
use gmofg_proxy_domain::{
    ChannelId as DomainChannelId, MessageStage as DomainMessageStage, Rule, RuleAction,
    RuleRuntimeSnapshot, TerminalAction,
};
use gmofg_proxy_product_api::{BodyCodec, RequestClassifier};
use gmofg_proxy_runtime::{
    ChannelId, ChannelRuntimeMetrics, ConnectionContext, ErrorCode, FaultAction, HandshakePolicy,
    Message, PipelinePorts, ProxyError, Result as ProxyResult, RuntimeMetricsProvider,
    RuntimeMetricsSnapshot, TlsPeerIdentity,
};
use parking_lot::Mutex;
use uuid::Uuid;

use super::{CaptureRepositoryAdapter, RuleRepositoryAdapter};
use message_projection::{
    classify_request, content_view, decode_json, encode_body, header_value, message_method,
    message_target, proxy_message,
};
#[cfg(test)]
use message_projection::{decode_body, display_headers, merge_edited_headers};
#[cfg(test)]
use rule_actions::map_terminal_action;
use rule_actions::{apply_rule_actions, weak_network_seed};
use rule_runtime::{EvaluatedRules, RuleRuntimeService};

mod message_projection;
mod rule_actions;
mod rule_runtime;

macro_rules! proxy_status {
    ($status:expr) => {
        $status.try_into().map_err(|error| {
            ProxyError::new(
                ErrorCode::ConfigInvalid,
                format!("invalid HTTP status: {error}"),
            )
        })
    };
}

/// One adapter instance is shared by both listeners for the lifetime of the app.
pub trait RuntimeRuleRepository: std::fmt::Debug + Send + Sync {
    fn runtime_snapshot(&self) -> AppResult<RuleRuntimeSnapshot>;
    fn commit_runtime_snapshot(
        &self,
        snapshot: &RuleRuntimeSnapshot,
        evaluated_rules: &[Rule],
    ) -> AppResult<u64>;
    fn reset_runtime_hit_metadata(&self) -> AppResult<()>;
}

impl RuntimeRuleRepository for RuleRepositoryAdapter {
    fn runtime_snapshot(&self) -> AppResult<RuleRuntimeSnapshot> {
        RuleRepositoryAdapter::runtime_snapshot(self)
    }

    fn commit_runtime_snapshot(
        &self,
        snapshot: &RuleRuntimeSnapshot,
        evaluated_rules: &[Rule],
    ) -> AppResult<u64> {
        RuleRepositoryAdapter::commit_runtime_snapshot(self, snapshot, evaluated_rules)
    }

    fn reset_runtime_hit_metadata(&self) -> AppResult<()> {
        RuleRepositoryAdapter::reset_runtime_hit_metadata(self)
    }
}

/// One adapter instance is shared by both listeners for the lifetime of the app.
#[derive(Debug)]
pub struct RuntimePipelineProductHooks {
    pub body_codec: Arc<dyn BodyCodec>,
    pub request_classifier: Arc<dyn RequestClassifier>,
    pub channel_labels: BTreeMap<String, String>,
}

/// One adapter instance is shared by both listeners for the lifetime of the app.
#[derive(Debug)]
pub struct RuntimePipelineAdapter {
    body_codec: Arc<dyn BodyCodec>,
    request_classifier: Arc<dyn RequestClassifier>,
    channel_labels: BTreeMap<String, String>,
    sessions: Arc<InMemorySessionStore>,
    breakpoints: Arc<BreakpointCoordinator>,
    events: Arc<EventHub>,
    captures: Arc<CaptureRepositoryAdapter>,
    capture_cursor: AtomicU64,
    rule_runtime: RuleRuntimeService,
    state: Mutex<PipelineState>,
}

#[derive(Debug, Default)]
struct PipelineState {
    connections: HashMap<Uuid, ConnectionRuntime>,
    live_sessions: HashMap<Uuid, LiveSession>,
    metrics_epoch: Option<Uuid>,
    channels: BTreeMap<ChannelId, ChannelRuntimeMetrics>,
}

#[derive(Debug)]
struct ConnectionRuntime {
    channel: ChannelId,
    session_id: Option<Uuid>,
    pending_breakpoints: Vec<Uuid>,
}

#[derive(Debug, Clone)]
struct LiveSession {
    started_at: DateTime<Utc>,
    runtime_epoch: Uuid,
}

#[derive(Clone, Copy)]
struct CapturePublication<'a> {
    stage: AppMessageStage,
    result: &'a str,
    tone: UiTone,
    breakpoint_id: Option<Uuid>,
    size_bytes: u64,
}

impl RuntimePipelineAdapter {
    #[must_use]
    pub fn new(
        product: RuntimePipelineProductHooks,
        rules: Arc<dyn RuntimeRuleRepository>,
        sessions: Arc<InMemorySessionStore>,
        breakpoints: Arc<BreakpointCoordinator>,
        events: Arc<EventHub>,
        captures: Arc<CaptureRepositoryAdapter>,
    ) -> Self {
        let rule_runtime = RuleRuntimeService::new(
            Arc::clone(&product.body_codec),
            product.channel_labels.clone(),
            rules,
            Arc::clone(&events),
        );
        Self {
            body_codec: product.body_codec,
            request_classifier: product.request_classifier,
            channel_labels: product.channel_labels,
            sessions,
            breakpoints,
            events,
            captures,
            capture_cursor: AtomicU64::new(0),
            rule_runtime,
            state: Mutex::new(PipelineState::default()),
        }
    }

    fn evaluate(
        &self,
        context: &ConnectionContext,
        stage: DomainMessageStage,
        message: Option<&Message>,
    ) -> ProxyResult<EvaluatedRules> {
        self.rule_runtime.evaluate(context, stage, message)
    }

    fn channel_label(&self, channel_id: &str) -> String {
        self.channel_labels
            .get(channel_id)
            .cloned()
            .unwrap_or_else(|| channel_id.to_owned())
    }

    fn begin_session(&self, context: &ConnectionContext, original: &Message) -> ProxyResult<Uuid> {
        let now = Utc::now();
        let session_id = Uuid::new_v4();
        let request = content_view(self.body_codec.as_ref(), original);
        let classified =
            classify_request(self.request_classifier.as_ref(), &context.channel, original);
        let request_id = classified
            .request_id
            .unwrap_or_else(|| session_id.to_string());
        let terminal_ip = context.peer_addr.ip().to_string();
        let fingerprint = fingerprint(context);
        let target = classified.request_type.unwrap_or_else(|| {
            message_target(&original.start_line)
                .unwrap_or_default()
                .to_owned()
        });
        let method = message_method(&original.start_line)
            .unwrap_or_default()
            .to_owned();
        let summary = SessionSummaryViewModel {
            session_id,
            request_id: request_id.clone(),
            started_at: now,
            completed_at: None,
            terminal_ip: terminal_ip.clone(),
            channel: app_channel(&context.channel)?,
            channel_text: self.channel_label(context.channel.as_str()),
            method,
            target,
            http_status: None,
            result: "处理中".into(),
            ui_tone: UiTone::Info,
            duration_ms: None,
            matched_rule_ids: Vec::new(),
            request_size_bytes: original.body.len() as u64,
            response_size_bytes: 0,
            pending_breakpoint: false,
            revision: 1,
        };
        let detail = SessionDetailViewModel {
            summary,
            runtime_epoch: context.runtime_epoch,
            connection_id: context.connection_id.to_string(),
            certificate_fingerprint: fingerprint.clone(),
            upstream_host: header_value(original, "host").unwrap_or_default(),
            app_to_proxy_tls: tls_summary(context),
            proxy_to_server_tls: "TLS 1.2 mTLS（等待上游）".into(),
            final_action: "处理中".into(),
            timings_ms: BTreeMap::new(),
            request: Some(request),
            response: None,
            rule_trace: Vec::new(),
        };
        let record = SessionRecord {
            detail,
            breakpoint_draft: None,
        };
        if let Err(error) = self.sessions.upsert(record.clone()) {
            self.resource_exhausted(context, &error);
            return Err(app_to_proxy(error));
        }
        {
            let mut state = self.state.lock();
            state.live_sessions.insert(
                session_id,
                LiveSession {
                    started_at: now,
                    runtime_epoch: context.runtime_epoch,
                },
            );
            if let Some(connection) = state.connections.get_mut(&context.connection_id) {
                connection.session_id = Some(session_id);
            }
            let metrics = state.channels.entry(context.channel.clone()).or_default();
            metrics.request_count = metrics.request_count.saturating_add(1);
        }
        Ok(session_id)
    }

    fn update_request(
        &self,
        context: &ConnectionContext,
        effective: &Message,
        rules: &EvaluatedRules,
        pending_breakpoint: bool,
        breakpoint_draft: Option<MessageContentViewModel>,
    ) -> ProxyResult<SessionRecord> {
        self.update_live_session(context, |record| {
            let summary = &mut record.detail.summary;
            summary.matched_rule_ids.clone_from(&rules.matched_ids);
            summary.request_size_bytes = effective.body.len() as u64;
            summary.pending_breakpoint = pending_breakpoint;
            summary.result = if pending_breakpoint {
                "断点等待".into()
            } else {
                "请求已处理".into()
            };
            summary.ui_tone = if pending_breakpoint {
                UiTone::Warning
            } else {
                UiTone::Info
            };
            summary.revision = summary.revision.saturating_add(1);
            record.detail.request = Some(content_view(self.body_codec.as_ref(), effective));
            record.detail.rule_trace.clone_from(&rules.traces);
            record.breakpoint_draft = breakpoint_draft;
        })
    }

    fn update_response(
        &self,
        context: &ConnectionContext,
        effective: &Message,
        rules: &EvaluatedRules,
        pending_breakpoint: bool,
        breakpoint_draft: Option<MessageContentViewModel>,
    ) -> ProxyResult<SessionRecord> {
        self.update_live_session(context, |record| {
            let summary = &mut record.detail.summary;
            for id in &rules.matched_ids {
                if !summary.matched_rule_ids.contains(id) {
                    summary.matched_rule_ids.push(*id);
                }
            }
            summary.response_size_bytes = effective.body.len() as u64;
            summary.http_status = effective.http_status();
            summary.pending_breakpoint = pending_breakpoint;
            summary.result = if pending_breakpoint {
                "断点等待".into()
            } else {
                "响应已处理".into()
            };
            summary.ui_tone = if pending_breakpoint {
                UiTone::Warning
            } else {
                UiTone::Info
            };
            summary.revision = summary.revision.saturating_add(1);
            record.detail.response = Some(content_view(self.body_codec.as_ref(), effective));
            record.detail.rule_trace.extend(rules.traces.clone());
            record.breakpoint_draft = breakpoint_draft;
        })
    }

    fn update_live_session(
        &self,
        context: &ConnectionContext,
        update: impl FnOnce(&mut SessionRecord),
    ) -> ProxyResult<SessionRecord> {
        let session_id = {
            let state = self.state.lock();
            let session_id = state
                .connections
                .get(&context.connection_id)
                .and_then(|connection| connection.session_id)
                .ok_or_else(|| {
                    ProxyError::new(ErrorCode::Internal, "connection has no active session")
                })?;
            if !state.live_sessions.contains_key(&session_id) {
                return Err(ProxyError::new(
                    ErrorCode::Internal,
                    "active session metadata is missing",
                ));
            }
            session_id
        };
        let mut record = self.sessions.get_record(session_id).map_err(app_to_proxy)?;
        update(&mut record);
        if let Err(error) = self.sessions.upsert(record.clone()) {
            self.resource_exhausted(context, &error);
            return Err(app_to_proxy(error));
        }
        self.events.publish(
            Some(context.runtime_epoch),
            Utc::now(),
            Some(record.id().to_string()),
            Some(record.detail.summary.revision),
            UiEventPayload::SessionUpdated(record.detail.summary.clone()),
        );
        Ok(record)
    }

    async fn pause(
        &self,
        context: &ConnectionContext,
        stage: AppMessageStage,
        original: &Message,
        effective: &mut Message,
        rules: &EvaluatedRules,
    ) -> ProxyResult<Vec<FaultAction>> {
        let detail = breakpoint_detail(
            self.body_codec.as_ref(),
            context,
            self.channel_label(context.channel.as_str()),
            stage,
            original,
            effective,
            self.session_id(context)?,
        )?;
        let breakpoint_id = detail.summary.breakpoint_id;
        let effective_view = detail.effective.clone();
        let ticket = self.breakpoints.register(detail).map_err(app_to_proxy)?;
        {
            let mut pipeline_state = self.state.lock();
            if let Some(connection) = pipeline_state.connections.get_mut(&context.connection_id) {
                connection.pending_breakpoints.push(breakpoint_id);
            }
        }
        let record = match stage {
            AppMessageStage::Request => {
                self.update_request(context, effective, rules, true, Some(effective_view))?
            }
            AppMessageStage::Response => {
                self.update_response(context, effective, rules, true, Some(effective_view))?
            }
            AppMessageStage::TlsHandshake | AppMessageStage::Terminal => {
                return Err(ProxyError::new(
                    ErrorCode::Internal,
                    "terminal messages cannot enter a breakpoint",
                ));
            }
        };
        self.events.publish(
            Some(context.runtime_epoch),
            Utc::now(),
            Some(breakpoint_id.to_string()),
            Some(ticket.detail.summary.revision),
            UiEventPayload::BreakpointQueued(ticket.detail.summary.clone()),
        );
        self.publish_capture(
            context,
            &record,
            CapturePublication {
                stage,
                result: "断点等待",
                tone: UiTone::Warning,
                breakpoint_id: Some(breakpoint_id),
                size_bytes: effective.body.len() as u64,
            },
        );

        let outcome = ticket
            .outcome
            .await
            .unwrap_or(BreakpointOutcome::ClientDisconnected);
        self.remove_pending_breakpoint(context.connection_id, breakpoint_id);
        match outcome {
            BreakpointOutcome::Decision(decision) => {
                let actions = apply_breakpoint_decision(
                    self.body_codec.as_ref(),
                    stage,
                    original,
                    effective,
                    decision.as_ref(),
                )?;
                match stage {
                    AppMessageStage::Request => {
                        self.update_request(context, effective, rules, false, None)?;
                    }
                    AppMessageStage::Response => {
                        self.update_response(context, effective, rules, false, None)?;
                    }
                    AppMessageStage::TlsHandshake | AppMessageStage::Terminal => {}
                }
                Ok(actions)
            }
            BreakpointOutcome::ClientDisconnected => Err(ProxyError::new(
                ErrorCode::ClientDisconnected,
                "客户端已断开，断点已终止。",
            )),
            BreakpointOutcome::ProxyStopped => Err(ProxyError::new(
                ErrorCode::ProxyStopped,
                "Proxy 已停止，断点已终止。",
            )),
        }
    }

    fn remove_pending_breakpoint(&self, connection_id: Uuid, breakpoint_id: Uuid) {
        if let Some(connection) = self.state.lock().connections.get_mut(&connection_id) {
            connection
                .pending_breakpoints
                .retain(|id| *id != breakpoint_id);
        }
    }

    fn session_id(&self, context: &ConnectionContext) -> ProxyResult<Uuid> {
        self.state
            .lock()
            .connections
            .get(&context.connection_id)
            .and_then(|connection| connection.session_id)
            .ok_or_else(|| ProxyError::new(ErrorCode::Internal, "connection has no active session"))
    }

    fn finish_session(&self, context: &ConnectionContext, result: &ProxyResult<()>) {
        let (session_id, live) = {
            let mut state = self.state.lock();
            let Some(session_id) = state
                .connections
                .get(&context.connection_id)
                .and_then(|connection| connection.session_id)
            else {
                return;
            };
            (session_id, state.live_sessions.remove(&session_id))
        };
        let Some(live) = live else {
            return;
        };
        let Ok(mut record) = self.sessions.get_record(session_id) else {
            return;
        };
        let now = Utc::now();
        let duration_ms = u64::try_from(
            now.signed_duration_since(live.started_at)
                .num_milliseconds()
                .max(0),
        )
        .unwrap_or(u64::MAX);
        {
            let summary = &mut record.detail.summary;
            summary.completed_at = Some(now);
            summary.duration_ms = Some(duration_ms);
            summary.pending_breakpoint = false;
            summary.revision = summary.revision.saturating_add(1);
            match result {
                Ok(()) => {
                    summary.result = "成功".into();
                    summary.ui_tone = UiTone::Positive;
                    record.detail.final_action = "响应已返回客户端".into();
                    record.detail.proxy_to_server_tls = "TLS 1.2 mTLS".into();
                }
                Err(error) => {
                    summary.result = result_text(error.code).into();
                    summary.ui_tone = result_tone(error.code);
                    record.detail.final_action.clone_from(&error.message);
                }
            }
        }
        record.detail.timings_ms.insert("total".into(), duration_ms);
        record.breakpoint_draft = None;
        let summary = record.detail.summary.clone();
        if let Err(error) = self.sessions.upsert(record.clone()) {
            self.resource_exhausted(context, &error);
            return;
        }
        self.events.publish(
            Some(context.runtime_epoch),
            now,
            Some(summary.session_id.to_string()),
            Some(summary.revision),
            UiEventPayload::SessionUpdated(summary.clone()),
        );
        self.publish_capture(
            context,
            &record,
            CapturePublication {
                stage: AppMessageStage::Terminal,
                result: &summary.result,
                tone: summary.ui_tone,
                breakpoint_id: None,
                size_bytes: summary
                    .request_size_bytes
                    .saturating_add(summary.response_size_bytes),
            },
        );
    }

    fn publish_capture(
        &self,
        context: &ConnectionContext,
        record: &SessionRecord,
        publication: CapturePublication<'_>,
    ) {
        let event_id = self
            .capture_cursor
            .fetch_add(1, Ordering::Relaxed)
            .saturating_add(1);
        let summary = &record.detail.summary;
        let row = CaptureRowViewModel {
            event_id,
            runtime_epoch: context.runtime_epoch,
            session_id: summary.session_id,
            occurred_at: Utc::now(),
            terminal_ip: summary.terminal_ip.clone(),
            channel: summary.channel.clone(),
            channel_text: self.channel_label(summary.channel.as_str()),
            stage: publication.stage,
            stage_text: publication.stage.display_zh().into(),
            method: summary.method.clone(),
            target: summary.target.clone(),
            http_status: summary.http_status,
            result: publication.result.into(),
            ui_tone: publication.tone,
            duration_ms: summary.duration_ms,
            matched_rule_ids: summary.matched_rule_ids.clone(),
            size_bytes: publication.size_bytes,
            breakpoint_id: publication.breakpoint_id,
            can_go_to_breakpoint: publication.breakpoint_id.is_some(),
            breakpoint_disabled_reason: publication.breakpoint_id.is_none().then(|| {
                DisabledReason {
                    code: "BREAKPOINT_NOT_PENDING".into(),
                    message: "该事件没有待处理断点。".into(),
                }
            }),
        };
        self.captures
            .push_for_epoch(row.clone(), context.runtime_epoch);
        let _ = self
            .events
            .push_capture(context.runtime_epoch, Utc::now(), row);
    }

    fn resource_exhausted(&self, context: &ConnectionContext, error: &AppError) {
        {
            let mut state = self.state.lock();
            let metrics = state.channels.entry(context.channel.clone()).or_default();
            metrics.error_count = metrics.error_count.saturating_add(1);
        }
        self.events.publish(
            Some(context.runtime_epoch),
            Utc::now(),
            None,
            None,
            UiEventPayload::ResourceWarning {
                message: error.view_model.message.clone(),
            },
        );
        self.events.publish(
            Some(context.runtime_epoch),
            Utc::now(),
            error.view_model.entity_id.clone(),
            None,
            UiEventPayload::OperationFailed((*error.view_model).clone()),
        );
    }

    fn terminate_connection_breakpoints(
        &self,
        context: &ConnectionContext,
    ) -> Vec<BreakpointSummaryViewModel> {
        let ids = self
            .state
            .lock()
            .connections
            .get(&context.connection_id)
            .map_or_else(Vec::new, |connection| {
                connection.pending_breakpoints.clone()
            });
        ids.into_iter()
            .filter_map(|id| self.breakpoints.client_disconnected(id).ok())
            .collect()
    }
}

impl HandshakePolicy for RuntimePipelineAdapter {
    fn reject_tls_handshake(
        &self,
        context: &ConnectionContext,
        peer: &TlsPeerIdentity,
    ) -> ProxyResult<bool> {
        // rustls invokes this policy while the peer identity is still being
        // verified, before the accepted ConnectionContext can be enriched.
        // Evaluate against a temporary context containing the verified peer.
        let mut verified_context = context.clone();
        verified_context.tls_peer = Some(peer.clone());
        let evaluated = self.evaluate(&verified_context, DomainMessageStage::TlsHandshake, None)?;
        self.rule_runtime
            .publish_rule_hits(verified_context.runtime_epoch, evaluated.hit_rules);
        Ok(evaluated.actions.into_iter().any(|action| {
            matches!(
                action,
                RuleAction::Terminal(TerminalAction::RejectTlsHandshake)
            )
        }))
    }
}

#[async_trait]
impl PipelinePorts for RuntimePipelineAdapter {
    async fn runtime_stopping(&self, epoch: Uuid) {
        let resolved = self.breakpoints.proxy_stopped(epoch);
        for summary in resolved {
            self.events.publish(
                Some(epoch),
                Utc::now(),
                Some(summary.breakpoint_id.to_string()),
                Some(summary.revision),
                UiEventPayload::BreakpointResolved(summary),
            );
        }
        self.rule_runtime.runtime_stopping(epoch);
    }

    async fn connection_opened(&self, context: &ConnectionContext) {
        let mut state = self.state.lock();
        if state.metrics_epoch != Some(context.runtime_epoch) {
            state.metrics_epoch = Some(context.runtime_epoch);
            state.channels.clear();
        }
        state.connections.insert(
            context.connection_id,
            ConnectionRuntime {
                channel: context.channel.clone(),
                session_id: None,
                pending_breakpoints: Vec::new(),
            },
        );
        let metrics = state.channels.entry(context.channel.clone()).or_default();
        metrics.connected_clients = metrics.connected_clients.saturating_add(1);
    }

    async fn request(
        &self,
        context: &ConnectionContext,
        message: &mut Message,
    ) -> ProxyResult<Vec<FaultAction>> {
        let original = message.clone();
        self.begin_session(context, &original)?;
        let evaluated = self.evaluate(context, DomainMessageStage::Request, Some(message))?;
        let seed = weak_network_seed(context, DomainMessageStage::Request, &evaluated.hit_rules);
        let (mut actions, pause) =
            apply_rule_actions(self.body_codec.as_ref(), message, &evaluated.actions, seed)?;
        self.rule_runtime
            .publish_rule_hits(context.runtime_epoch, evaluated.hit_rules.clone());
        if pause {
            actions.extend(
                self.pause(
                    context,
                    AppMessageStage::Request,
                    &original,
                    message,
                    &evaluated,
                )
                .await?,
            );
        } else {
            let record = self.update_request(context, message, &evaluated, false, None)?;
            self.publish_capture(
                context,
                &record,
                CapturePublication {
                    stage: AppMessageStage::Request,
                    result: "请求",
                    tone: UiTone::Info,
                    breakpoint_id: None,
                    size_bytes: message.body.len() as u64,
                },
            );
        }
        Ok(actions)
    }

    async fn response(
        &self,
        context: &ConnectionContext,
        message: &mut Message,
    ) -> ProxyResult<Vec<FaultAction>> {
        {
            let mut state = self.state.lock();
            let metrics = state.channels.entry(context.channel.clone()).or_default();
            metrics.upstream_response_count = metrics.upstream_response_count.saturating_add(1);
            metrics.last_upstream_error = None;
        }
        let original = message.clone();
        let evaluated = self.evaluate(context, DomainMessageStage::Response, Some(message))?;
        let seed = weak_network_seed(context, DomainMessageStage::Response, &evaluated.hit_rules);
        let (mut actions, pause) =
            apply_rule_actions(self.body_codec.as_ref(), message, &evaluated.actions, seed)?;
        self.rule_runtime
            .publish_rule_hits(context.runtime_epoch, evaluated.hit_rules.clone());
        if pause {
            actions.extend(
                self.pause(
                    context,
                    AppMessageStage::Response,
                    &original,
                    message,
                    &evaluated,
                )
                .await?,
            );
        } else {
            let record = self.update_response(context, message, &evaluated, false, None)?;
            self.publish_capture(
                context,
                &record,
                CapturePublication {
                    stage: AppMessageStage::Response,
                    result: "响应",
                    tone: UiTone::Info,
                    breakpoint_id: None,
                    size_bytes: message.body.len() as u64,
                },
            );
        }
        Ok(actions)
    }

    async fn connection_closed(&self, context: &ConnectionContext, result: &ProxyResult<()>) {
        for summary in self.terminate_connection_breakpoints(context) {
            self.events.publish(
                Some(context.runtime_epoch),
                Utc::now(),
                Some(summary.breakpoint_id.to_string()),
                Some(summary.revision),
                UiEventPayload::BreakpointResolved(summary),
            );
        }
        {
            // Update health metrics before SessionUpdated is published so a UI
            // refresh triggered by that event observes the new global state.
            let mut state = self.state.lock();
            if let Some(channel) = state
                .connections
                .get(&context.connection_id)
                .map(|connection| connection.channel.clone())
            {
                let metrics = state.channels.entry(channel).or_default();
                metrics.connected_clients = metrics.connected_clients.saturating_sub(1);
                if let Err(error) = result {
                    metrics.error_count = metrics.error_count.saturating_add(1);
                    if is_upstream_error(error.code) {
                        metrics.last_upstream_error = Some(error.message.clone());
                    }
                }
            }
        }
        self.finish_session(context, result);
        self.state.lock().connections.remove(&context.connection_id);
    }

    async fn runtime_fault(&self, epoch: Uuid, channel: ChannelId, error: &ProxyError) {
        {
            let mut state = self.state.lock();
            let metrics = state.channels.entry(channel).or_default();
            metrics.error_count = metrics.error_count.saturating_add(1);
        }
        self.events.publish(
            Some(epoch),
            Utc::now(),
            None,
            None,
            UiEventPayload::OperationFailed(
                AppError::new(error.code, error.message.clone()).into(),
            ),
        );
    }
}

fn is_upstream_error(code: &str) -> bool {
    matches!(
        code,
        "UPSTREAM_CONNECT_TIMEOUT"
            | "UPSTREAM_WRITE_TIMEOUT"
            | "UPSTREAM_READ_TIMEOUT"
            | "TLS_HANDSHAKE_FAILED"
            | "IO_ERROR"
    )
}

#[async_trait]
impl RuntimeMetricsProvider for RuntimePipelineAdapter {
    async fn configure_capacity(&self, max_sessions: usize, max_bytes: u64) -> ProxyResult<()> {
        self.events.reclaim_for_limit(max_bytes);
        self.sessions
            .set_limits(max_sessions, max_bytes)
            .map(|_| ())
            .map_err(app_to_proxy)
    }

    async fn snapshot(&self, runtime_epoch: Option<Uuid>) -> ProxyResult<RuntimeMetricsSnapshot> {
        let state = self.state.lock();
        let channels = if runtime_epoch.is_some() && runtime_epoch != state.metrics_epoch {
            BTreeMap::new()
        } else {
            state.channels.clone()
        };
        let active_sessions = state
            .live_sessions
            .values()
            .filter(|session| runtime_epoch.is_none_or(|epoch| session.runtime_epoch == epoch))
            .count();
        drop(state);
        let pending_breakpoints = self.breakpoints.query(runtime_epoch).len();
        Ok(RuntimeMetricsSnapshot {
            channels,
            active_sessions,
            pending_breakpoints,
            logical_memory_bytes: self.sessions.logical_bytes(),
        })
    }
}

fn apply_breakpoint_decision(
    body_codec: &dyn BodyCodec,
    stage: AppMessageStage,
    original: &Message,
    effective: &mut Message,
    decision: &BreakpointDecision,
) -> ProxyResult<Vec<FaultAction>> {
    let mut actions = Vec::new();
    match decision.kind {
        BreakpointDecisionKind::ForwardOriginal => *effective = original.clone(),
        BreakpointDecisionKind::ForwardModified => {
            *effective = proxy_message(
                decision.message.as_ref().ok_or_else(|| {
                    ProxyError::new(ErrorCode::ConfigInvalid, "modified message is missing")
                })?,
                &effective.start_line,
            )?;
        }
        BreakpointDecisionKind::MockResponse => {
            let message = decision.message.as_ref().ok_or_else(|| {
                ProxyError::new(ErrorCode::ConfigInvalid, "mock response is missing")
            })?;
            let mock = proxy_message(message, "HTTP/1.1 200 OK")?;
            actions.push(FaultAction::MockResponse {
                status: proxy_status!(decision.http_status.unwrap_or(200))?,
                headers: mock.header_map()?,
                body: Bytes::from(encode_body(
                    body_codec,
                    message.body_text.as_deref().ok_or_else(|| ProxyError {
                        code: "BODY_ENCODE_FAILED",
                        message: "mock body text is missing".into(),
                    })?,
                )?),
            });
        }
        BreakpointDecisionKind::Delay => {
            actions.push(FaultAction::Delay(Duration::from_millis(
                decision
                    .delay_ms
                    .ok_or_else(|| ProxyError::new(ErrorCode::ConfigInvalid, "delay is missing"))?,
            )));
        }
        BreakpointDecisionKind::DisconnectBeforeUpstream => {
            actions.push(FaultAction::DisconnectBeforeUpstream);
        }
        BreakpointDecisionKind::CustomHttpStatus => {
            actions.push(FaultAction::CustomStatus(proxy_status!(
                decision.http_status.ok_or_else(|| {
                    ProxyError::new(ErrorCode::ConfigInvalid, "HTTP status is missing")
                })?
            )?));
        }
        BreakpointDecisionKind::InvalidJson => actions.push(FaultAction::ReplaceBody {
            body: Bytes::from(encode_body(body_codec, "{invalid-json")?),
        }),
        BreakpointDecisionKind::WrongContentLength => {
            actions.push(FaultAction::ContentLengthOffset(
                decision.content_length_delta.ok_or_else(|| {
                    ProxyError::new(ErrorCode::ConfigInvalid, "content-length delta is missing")
                })?,
            ));
        }
        BreakpointDecisionKind::Truncate => {
            actions.push(FaultAction::TruncateResponse(
                decision.truncate_at.ok_or_else(|| {
                    ProxyError::new(ErrorCode::ConfigInvalid, "truncate position is missing")
                })?,
            ));
        }
        BreakpointDecisionKind::DropResponse => {
            actions.push(FaultAction::DropResponse {
                read_upstream: stage == AppMessageStage::Request,
            });
        }
    }
    Ok(actions)
}

#[cfg(test)]
fn view_to_domain_rule(view: gmofg_proxy_application::RuleViewModel) -> ProxyResult<Rule> {
    let draft = view.draft;
    let stage = match draft.stage {
        Some(AppMessageStage::Request) => DomainMessageStage::Request,
        Some(AppMessageStage::Response) => DomainMessageStage::Response,
        Some(AppMessageStage::TlsHandshake) => DomainMessageStage::TlsHandshake,
        _ => {
            return Err(ProxyError::new(
                ErrorCode::ConfigInvalid,
                "rule has an invalid stage",
            ));
        }
    };
    let conditions = draft
        .conditions
        .iter()
        .map(super::rules::condition_to_domain)
        .collect();
    let actions = draft
        .actions
        .iter()
        .map(super::rules::action_to_domain)
        .collect::<std::result::Result<Vec<_>, _>>()
        .map_err(|error| ProxyError::new(ErrorCode::ConfigInvalid, error.to_string()))?;
    Ok(Rule {
        id: gmofg_proxy_domain::RuleId::from_uuid(view.summary.rule_id),
        revision: gmofg_proxy_domain::Revision::new(view.summary.revision),
        name: draft.name,
        description: draft.description,
        enabled: draft.enabled,
        priority: u32::try_from(draft.priority).map_err(|_| {
            ProxyError::new(ErrorCode::ConfigInvalid, "rule priority cannot be negative")
        })?,
        created_order: view.summary.creation_order,
        channel: draft.channel,
        stage,
        conditions,
        actions,
        one_shot: draft.one_shot,
        hit_count: view.summary.hit_count,
        last_hit_at: view.summary.last_hit_at,
    })
}

fn breakpoint_detail(
    body_codec: &dyn BodyCodec,
    context: &ConnectionContext,
    channel_text: String,
    stage: AppMessageStage,
    original: &Message,
    effective: &Message,
    session_id: Uuid,
) -> ProxyResult<BreakpointDetailViewModel> {
    let title = match stage {
        AppMessageStage::Request => "请求断点·发送至服务器前",
        AppMessageStage::Response => "响应断点·返回 App 前",
        AppMessageStage::TlsHandshake | AppMessageStage::Terminal => "终态",
    };
    Ok(BreakpointDetailViewModel {
        summary: BreakpointSummaryViewModel {
            breakpoint_id: Uuid::new_v4(),
            session_id,
            runtime_epoch: context.runtime_epoch,
            stage,
            title: title.into(),
            terminal_ip: context.peer_addr.ip().to_string(),
            channel: app_channel(&context.channel)?,
            channel_text,
            method: message_method(&effective.start_line)
                .unwrap_or_default()
                .to_owned(),
            target: message_target(&effective.start_line)
                .unwrap_or_default()
                .to_owned(),
            waiting_since: Utc::now(),
            certificate_fingerprint_suffix: fingerprint_suffix(&fingerprint(context)),
            state: BreakpointState::Pending,
            state_text: "待处理".into(),
            ui_tone: UiTone::Warning,
            revision: 1,
        },
        original: content_view(body_codec, original),
        effective: content_view(body_codec, effective),
        can_resolve: true,
        resolve_disabled_reason: None,
        available_actions: Vec::new(),
    })
}

fn app_to_proxy(error: AppError) -> ProxyError {
    if matches!(
        error.view_model.code.as_str(),
        "RESOURCE_EXHAUSTED" | "REVISION_CONFLICT"
    ) {
        let code = if error.view_model.code == "RESOURCE_EXHAUSTED" {
            "RESOURCE_EXHAUSTED"
        } else {
            "REVISION_CONFLICT"
        };
        return ProxyError {
            code,
            message: error.view_model.message,
        };
    }
    if matches!(
        error.view_model.code.as_str(),
        "BODY_DECODE_FAILED" | "BODY_ENCODE_FAILED"
    ) {
        return ProxyError {
            code: if error.view_model.code == "BODY_DECODE_FAILED" {
                "BODY_DECODE_FAILED"
            } else {
                "BODY_ENCODE_FAILED"
            },
            message: error.view_model.message,
        };
    }
    let code = match error.view_model.code.as_str() {
        "JSON_INVALID" | "CONFIG_INVALID" | "RULE_INVALID" => ErrorCode::ConfigInvalid,
        _ => ErrorCode::Internal,
    };
    ProxyError::new(code, error.view_model.message)
}

fn app_channel(channel: &ChannelId) -> ProxyResult<AppChannelId> {
    AppChannelId::new(channel.as_str()).map_err(|error| {
        ProxyError::new(
            ErrorCode::ConfigInvalid,
            format!("invalid application channel `{channel}`: {error}"),
        )
    })
}

fn domain_channel(channel: &ChannelId) -> ProxyResult<DomainChannelId> {
    DomainChannelId::new(channel.as_str()).map_err(|error| {
        ProxyError::new(
            ErrorCode::ConfigInvalid,
            format!("invalid domain channel `{channel}`: {error}"),
        )
    })
}

fn fingerprint(context: &ConnectionContext) -> String {
    context
        .tls_peer
        .as_ref()
        .map_or_else(String::new, |identity| identity.sha256_fingerprint.clone())
}

fn fingerprint_suffix(fingerprint: &str) -> String {
    fingerprint
        .chars()
        .rev()
        .take(12)
        .collect::<String>()
        .chars()
        .rev()
        .collect()
}

fn tls_summary(context: &ConnectionContext) -> String {
    context.tls_peer.as_ref().map_or_else(
        || "TLS 1.2 mTLS（未取得客户端身份）".into(),
        |identity| format!("TLS 1.2 mTLS / {}", identity.subject_summary),
    )
}

fn result_text(code: &str) -> &'static str {
    match code {
        "UPSTREAM_CONNECT_TIMEOUT" | "UPSTREAM_WRITE_TIMEOUT" | "UPSTREAM_READ_TIMEOUT" => {
            "上游超时"
        }
        "BREAKPOINT_CLIENT_DISCONNECTED" => "App 断开",
        "BREAKPOINT_PROXY_STOPPED" | "FAULT_EXECUTION_CANCELLED" => "Proxy 停止",
        "TLS_HANDSHAKE_FAILED" => "TLS 失败",
        "INCORRECT_CONTENT_LENGTH" => "规则终止",
        "TRUNCATED_RESPONSE" => "截断",
        "FAULT_STREAM_ABORTED" => "弱网断连",
        _ => "内部错误",
    }
}

fn result_tone(code: &str) -> UiTone {
    match code {
        "BREAKPOINT_PROXY_STOPPED" => UiTone::Warning,
        _ => UiTone::Danger,
    }
}

#[cfg(test)]
mod tests {
    use std::{
        net::SocketAddr,
        sync::atomic::{AtomicBool, AtomicUsize, Ordering as AtomicOrdering},
        time::SystemTime,
    };

    use gmofg_proxy_application::{
        RawHttpHeaderViewModel, RuleDraft as AppRuleDraft, RuleSummaryViewModel, RuleViewModel,
    };
    use gmofg_proxy_domain::DropResponseMode;
    use gmofg_proxy_product_api::ProductMessageContext;
    use gmofg_proxy_runtime::{RawHeader, TlsPeerIdentity};
    use serde_json::json;

    use super::*;

    #[derive(Debug)]
    struct Utf8BodyCodec;

    impl BodyCodec for Utf8BodyCodec {
        fn id(&self) -> &'static str {
            "test-utf8"
        }

        fn name(&self) -> &'static str {
            "Test UTF-8"
        }

        fn decode(&self, bytes: &[u8]) -> Result<String, gmofg_proxy_product_api::ProductError> {
            std::str::from_utf8(bytes)
                .map(str::to_owned)
                .map_err(|error| {
                    gmofg_proxy_product_api::ProductError::new(
                        "BODY_DECODE_FAILED",
                        error.to_string(),
                    )
                })
        }

        fn encode(&self, text: &str) -> Result<Vec<u8>, gmofg_proxy_product_api::ProductError> {
            Ok(text.as_bytes().to_vec())
        }
    }

    fn test_body_codec() -> Arc<dyn BodyCodec> {
        Arc::new(Utf8BodyCodec)
    }

    #[derive(Debug)]
    struct TestRequestClassifier;

    impl RequestClassifier for TestRequestClassifier {
        fn classify(
            &self,
            message: ProductMessageContext<'_>,
        ) -> gmofg_proxy_product_api::ClassifiedRequest {
            let request_id = message
                .headers
                .iter()
                .find(|header| header.name.eq_ignore_ascii_case(b"x-test-request-id"))
                .map(|header| String::from_utf8_lossy(header.value).into_owned());
            gmofg_proxy_product_api::ClassifiedRequest {
                request_id,
                request_type: None,
            }
        }
    }

    fn test_request_classifier() -> Arc<dyn RequestClassifier> {
        Arc::new(TestRequestClassifier)
    }

    fn test_channel_labels() -> BTreeMap<String, String> {
        BTreeMap::from([
            ("transaction".into(), "交易".into()),
            ("dll".into(), "DLL".into()),
            ("alpha".into(), "Alpha".into()),
        ])
    }

    fn test_product_hooks() -> RuntimePipelineProductHooks {
        RuntimePipelineProductHooks {
            body_codec: test_body_codec(),
            request_classifier: test_request_classifier(),
            channel_labels: test_channel_labels(),
        }
    }

    #[derive(Debug)]
    struct StableErrorCodec;

    impl BodyCodec for StableErrorCodec {
        fn id(&self) -> &'static str {
            "stable-error"
        }

        fn name(&self) -> &'static str {
            "Stable Error"
        }

        fn decode(&self, _bytes: &[u8]) -> Result<String, gmofg_proxy_product_api::ProductError> {
            Err(gmofg_proxy_product_api::ProductError::new(
                "PRODUCT_DECODE_FAILED",
                "decode failed",
            ))
        }

        fn encode(&self, _text: &str) -> Result<Vec<u8>, gmofg_proxy_product_api::ProductError> {
            Err(gmofg_proxy_product_api::ProductError::new(
                "PRODUCT_ENCODE_FAILED",
                "encode failed",
            ))
        }
    }

    #[test]
    fn product_codec_error_codes_and_json_syntax_classification_are_stable() {
        let decode = decode_body(&StableErrorCodec, b"wire").expect_err("decode must fail");
        assert_eq!(decode.code, "PRODUCT_DECODE_FAILED");
        assert_eq!(decode.message, "decode failed");

        let encode = encode_body(&StableErrorCodec, "text").expect_err("encode must fail");
        assert_eq!(encode.code, "PRODUCT_ENCODE_FAILED");
        assert_eq!(encode.message, "encode failed");

        let json = decode_json(&Utf8BodyCodec, b"{invalid").expect_err("JSON must fail");
        assert_eq!(json.code, "JSON_INVALID");
        assert!(json.message.contains("not valid JSON"));
    }

    #[test]
    fn product_request_classifier_receives_canonical_wire_metadata() {
        #[derive(Debug)]
        struct InspectingClassifier;

        impl RequestClassifier for InspectingClassifier {
            fn classify(
                &self,
                message: ProductMessageContext<'_>,
            ) -> gmofg_proxy_product_api::ClassifiedRequest {
                assert_eq!(message.channel_id, "alpha");
                assert_eq!(message.start_line, b"POST /vendor HTTP/1.1");
                assert_eq!(
                    message
                        .headers
                        .iter()
                        .map(|header| (header.name, header.value))
                        .collect::<Vec<_>>(),
                    vec![
                        (b"X-Vendor".as_slice(), b"value\x80\xff".as_slice()),
                        (b"x-vendor".as_slice(), b"second".as_slice()),
                    ]
                );
                assert_eq!(message.body, b"opaque");
                gmofg_proxy_product_api::ClassifiedRequest {
                    request_id: Some("product-id".into()),
                    request_type: Some("product-type".into()),
                }
            }
        }

        let channel = ChannelId::new("alpha").expect("channel");
        let message = Message {
            start_line: "POST /vendor HTTP/1.1".into(),
            headers: vec![
                RawHeader::new(
                    Bytes::from_static(b"X-Vendor"),
                    Bytes::from_static(b"value\x80\xff"),
                ),
                RawHeader::new(
                    Bytes::from_static(b"x-vendor"),
                    Bytes::from_static(b"second"),
                ),
            ],
            body: Bytes::from_static(b"opaque"),
            body_modified: false,
        };

        let classified = classify_request(&InspectingClassifier, &channel, &message);
        assert_eq!(classified.request_id.as_deref(), Some("product-id"));
        assert_eq!(classified.request_type.as_deref(), Some("product-type"));
    }

    #[test]
    fn forward_modified_uses_canonical_wire_headers_instead_of_lossy_display_projection() {
        let original = Message {
            start_line: "HTTP/1.1 299 Vendor Specific Result".into(),
            headers: vec![
                RawHeader::new(
                    Bytes::from_static(b"X-Trace"),
                    Bytes::from_static(b"first\x80"),
                ),
                RawHeader::new(
                    Bytes::from_static(b"x-Other"),
                    Bytes::from_static(b"middle\xff"),
                ),
                RawHeader::new(
                    Bytes::from_static(b"x-TRACE"),
                    Bytes::from_static(b"second"),
                ),
                RawHeader::new(Bytes::from_static(b"x-Other"), Bytes::from_static(b"last")),
            ],
            body: Bytes::from_static(b"old"),
            body_modified: false,
        };
        let mut edited = content_view(&Utf8BodyCodec, &original);
        edited.body_bytes = b"new".to_vec();
        edited.body_text = Some("new".into());
        edited.content_length = 3;
        let decision = BreakpointDecision {
            breakpoint_id: Uuid::new_v4(),
            expected_revision: 1,
            kind: BreakpointDecisionKind::ForwardModified,
            message: Some(edited),
            delay_ms: None,
            http_status: None,
            content_length_delta: None,
            truncate_at: None,
        };
        let mut effective = original.clone();

        let faults = apply_breakpoint_decision(
            &Utf8BodyCodec,
            AppMessageStage::Response,
            &original,
            &mut effective,
            &decision,
        )
        .expect("forward modified");

        assert!(faults.is_empty());
        assert_eq!(effective.start_line, original.start_line);
        assert_eq!(effective.headers, original.headers);
        assert_eq!(effective.body, Bytes::from_static(b"new"));
        assert_eq!(
            effective.reconstruct(),
            Bytes::from_static(
                b"HTTP/1.1 299 Vendor Specific Result\r\n\
X-Trace: first\x80\r\n\
x-Other: middle\xff\r\n\
x-TRACE: second\r\n\
x-Other: last\r\n\r\nnew"
            )
        );
    }

    #[test]
    fn forward_modified_preserves_unedited_header_ows_byte_for_byte() {
        let original = Message::from_raw_http1_head(
            b"POST /ows HTTP/1.1\r\n\
X-Mixed:\t  value \t\r\n\
X-Compact:value\r\n\r\n",
            Bytes::from_static(b"body"),
        )
        .expect("raw message");
        let view = content_view(&Utf8BodyCodec, &original);

        let effective = proxy_message(&view, &original.start_line).expect("effective message");

        assert_eq!(effective.reconstruct(), original.reconstruct());
    }

    #[test]
    fn forward_modified_rejects_non_ows_wire_metadata_from_the_frontend() {
        let original = Message::from_raw_http1_head(
            b"POST /ows HTTP/1.1\r\nX-Test: value\r\n\r\n",
            Bytes::new(),
        )
        .expect("raw message");
        let mut view = content_view(&Utf8BodyCodec, &original);
        view.raw_headers[0].leading_ows_bytes = b"\r\nInjected: ".to_vec();

        let error = proxy_message(&view, &original.start_line).expect_err("invalid OWS");

        assert_eq!(error.code, ErrorCode::ConfigInvalid.as_str());
    }

    #[test]
    fn breakpoint_ipc_cannot_replace_the_rust_owned_start_line_or_use_status_600() {
        let original = Message::from_raw_http1_head(
            b"POST /safe HTTP/1.1\r\nHost: example.test\r\n\r\n",
            Bytes::new(),
        )
        .expect("raw message");
        let mut view = content_view(&Utf8BodyCodec, &original);
        view.start_line_bytes = b"POST / HTTP/1.1\r\nX-Injected: value".to_vec();

        let reconstructed = proxy_message(&view, &original.start_line).expect("safe message");
        assert_eq!(reconstructed.start_line, original.start_line);
        assert!(
            !reconstructed
                .reconstruct()
                .windows(10)
                .any(|window| window == b"X-Injected")
        );

        view.http_status = Some(600);
        let error = proxy_message(&view, "HTTP/1.1 200 OK").expect_err("invalid status");
        assert_eq!(error.code, ErrorCode::ConfigInvalid.as_str());
    }

    #[test]
    fn forward_modified_merges_header_edits_and_applies_changed_http_status() {
        let original = Message {
            start_line: "HTTP/1.1 299 Vendor Specific Result".into(),
            headers: vec![
                RawHeader::new(
                    Bytes::from_static(b"X-Keep"),
                    Bytes::from_static(b"binary\x80\xff"),
                ),
                RawHeader::new(Bytes::from_static(b"X-Edit"), Bytes::from_static(b"old")),
                RawHeader::new(Bytes::from_static(b"x-remove"), Bytes::from_static(b"gone")),
            ],
            body: Bytes::from_static(b"body"),
            body_modified: false,
        };
        let mut edited = content_view(&Utf8BodyCodec, &original);
        edited.http_status = Some(503);
        edited.headers.insert("X-Edit".into(), vec!["new".into()]);
        edited.headers.remove("x-remove");
        edited.headers.insert("X-Added".into(), vec!["yes".into()]);
        let decision = BreakpointDecision {
            breakpoint_id: Uuid::new_v4(),
            expected_revision: 1,
            kind: BreakpointDecisionKind::ForwardModified,
            message: Some(edited),
            delay_ms: None,
            http_status: None,
            content_length_delta: None,
            truncate_at: None,
        };
        let mut effective = original.clone();

        apply_breakpoint_decision(
            &Utf8BodyCodec,
            AppMessageStage::Response,
            &original,
            &mut effective,
            &decision,
        )
        .expect("forward modified");

        assert_eq!(effective.start_line, "HTTP/1.1 503 Vendor Specific Result");
        assert_eq!(effective.http_status(), Some(503));
        assert_eq!(
            effective
                .headers
                .iter()
                .map(|header| (header.name.as_ref(), header.value.as_ref()))
                .collect::<Vec<_>>(),
            vec![
                (b"X-Keep".as_slice(), b"binary\x80\xff".as_slice()),
                (b"X-Edit".as_slice(), b"new".as_slice()),
                (b"X-Added".as_slice(), b"yes".as_slice()),
            ],
            "untouched wire fields stay exact while edited/deleted/added fields follow the UI"
        );
    }

    #[test]
    fn mixed_case_header_edit_and_delete_apply_to_one_case_insensitive_field_group() {
        let raw = vec![
            RawHttpHeaderViewModel {
                name_bytes: b"X-Trace".to_vec(),
                value_bytes: b"first".to_vec(),
                leading_ows_bytes: b" ".to_vec(),
                trailing_ows_bytes: Vec::new(),
            },
            RawHttpHeaderViewModel {
                name_bytes: b"X-Keep".to_vec(),
                value_bytes: b"binary\x80\xff".to_vec(),
                leading_ows_bytes: b" ".to_vec(),
                trailing_ows_bytes: Vec::new(),
            },
            RawHttpHeaderViewModel {
                name_bytes: b"x-TRACE".to_vec(),
                value_bytes: b"second".to_vec(),
                leading_ows_bytes: b" ".to_vec(),
                trailing_ows_bytes: Vec::new(),
            },
            RawHttpHeaderViewModel {
                name_bytes: b"X-Remove".to_vec(),
                value_bytes: b"gone".to_vec(),
                leading_ows_bytes: b" ".to_vec(),
                trailing_ows_bytes: Vec::new(),
            },
        ];
        let mut edited = display_headers(&raw);
        assert_eq!(
            edited.get("X-Trace"),
            Some(&vec!["first".into(), "second".into()])
        );
        // Simulate an editor returning a different casing for the same field.
        edited.insert("x-trace".into(), vec!["replacement".into()]);
        edited.remove("X-Remove");

        let merged = merge_edited_headers(&raw, &edited).expect("valid wire whitespace");

        assert_eq!(
            merged
                .iter()
                .map(|header| (header.name.as_ref(), header.value.as_ref()))
                .collect::<Vec<_>>(),
            vec![
                (b"x-trace".as_slice(), b"replacement".as_slice()),
                (b"X-Keep".as_slice(), b"binary\x80\xff".as_slice()),
            ]
        );
    }

    #[test]
    fn intentional_wire_faults_are_not_reported_as_internal_errors() {
        assert_eq!(result_text("INCORRECT_CONTENT_LENGTH"), "规则终止");
        assert_eq!(result_text("TRUNCATED_RESPONSE"), "截断");
    }

    #[derive(Debug)]
    struct RejectingCodec;

    impl BodyCodec for RejectingCodec {
        fn id(&self) -> &'static str {
            "rejecting"
        }

        fn name(&self) -> &'static str {
            "Rejecting Codec"
        }

        fn decode(&self, _: &[u8]) -> Result<String, gmofg_proxy_product_api::ProductError> {
            Ok(String::new())
        }

        fn encode(&self, _: &str) -> Result<Vec<u8>, gmofg_proxy_product_api::ProductError> {
            Err(gmofg_proxy_product_api::ProductError::new(
                "PRODUCT_SPECIFIC_CODE",
                "rejected",
            ))
        }
    }

    #[test]
    fn codec_failures_keep_generic_stable_error_codes() {
        let codec = Utf8BodyCodec;
        let decode_error = decode_body(&codec, &[0xff]).expect_err("invalid UTF-8");
        assert_eq!(decode_error.code, "BODY_DECODE_FAILED");

        let encode_error = encode_body(&RejectingCodec, "body").expect_err("rejected body");
        assert_eq!(encode_error.code, "PRODUCT_SPECIFIC_CODE");
    }

    #[derive(Debug)]
    struct StaticRules {
        snapshot: Mutex<RuleRuntimeSnapshot>,
    }

    #[derive(Debug)]
    struct RejectingCommitRules {
        snapshot: Mutex<RuleRuntimeSnapshot>,
        reject_commit: AtomicBool,
    }

    #[derive(Debug)]
    struct ConflictOnceRules {
        snapshot: Mutex<RuleRuntimeSnapshot>,
        conflict_once: AtomicBool,
        commit_attempts: AtomicUsize,
    }

    impl RuntimeRuleRepository for RejectingCommitRules {
        fn runtime_snapshot(&self) -> AppResult<RuleRuntimeSnapshot> {
            Ok(self.snapshot.lock().clone())
        }

        fn commit_runtime_snapshot(&self, _: &RuleRuntimeSnapshot, _: &[Rule]) -> AppResult<u64> {
            if self.reject_commit.load(AtomicOrdering::Acquire) {
                Err(AppError::new(
                    "REVISION_CONFLICT",
                    "模拟运行态事务提交失败。",
                ))
            } else {
                Ok(1)
            }
        }

        fn reset_runtime_hit_metadata(&self) -> AppResult<()> {
            Ok(())
        }
    }

    impl RuntimeRuleRepository for StaticRules {
        fn runtime_snapshot(&self) -> AppResult<RuleRuntimeSnapshot> {
            Ok(self.snapshot.lock().clone())
        }

        fn commit_runtime_snapshot(
            &self,
            snapshot: &RuleRuntimeSnapshot,
            evaluated_rules: &[Rule],
        ) -> AppResult<u64> {
            let mut current = self.snapshot.lock();
            if current.signature != snapshot.signature
                || current.collection_revision != snapshot.collection_revision
            {
                return Err(AppError::new("REVISION_CONFLICT", "规则测试快照已变化。"));
            }
            let next_revision = current.collection_revision.saturating_add(1);
            *current = RuleRuntimeSnapshot::with_collection_revision(
                next_revision,
                evaluated_rules.to_vec(),
            );
            Ok(next_revision)
        }

        fn reset_runtime_hit_metadata(&self) -> AppResult<()> {
            let mut current = self.snapshot.lock();
            for rule in &mut current.rules {
                rule.hit_count = 0;
                rule.last_hit_at = None;
            }
            let next_revision = current.collection_revision.saturating_add(1);
            *current =
                RuleRuntimeSnapshot::with_collection_revision(next_revision, current.rules.clone());
            Ok(())
        }
    }

    impl RuntimeRuleRepository for ConflictOnceRules {
        fn runtime_snapshot(&self) -> AppResult<RuleRuntimeSnapshot> {
            Ok(self.snapshot.lock().clone())
        }

        fn commit_runtime_snapshot(
            &self,
            snapshot: &RuleRuntimeSnapshot,
            evaluated_rules: &[Rule],
        ) -> AppResult<u64> {
            self.commit_attempts.fetch_add(1, AtomicOrdering::AcqRel);
            let mut current = self.snapshot.lock();
            if self.conflict_once.swap(false, AtomicOrdering::AcqRel) {
                let externally_advanced_revision = current.collection_revision.saturating_add(1);
                let externally_preserved_rules = current.rules.clone();
                *current = RuleRuntimeSnapshot::with_collection_revision(
                    externally_advanced_revision,
                    externally_preserved_rules,
                );
                return Err(AppError::new(
                    "REVISION_CONFLICT",
                    "模拟评估后发生外部规则集合更新。",
                ));
            }
            if current.signature != snapshot.signature
                || current.collection_revision != snapshot.collection_revision
            {
                return Err(AppError::new("REVISION_CONFLICT", "规则测试快照已变化。"));
            }
            let next_revision = current.collection_revision.saturating_add(1);
            *current = RuleRuntimeSnapshot::with_collection_revision(
                next_revision,
                evaluated_rules.to_vec(),
            );
            Ok(next_revision)
        }

        fn reset_runtime_hit_metadata(&self) -> AppResult<()> {
            Ok(())
        }
    }

    fn pause_rule() -> RuleViewModel {
        let id = Uuid::new_v4();
        RuleViewModel {
            summary: RuleSummaryViewModel {
                rule_id: id,
                revision: 1,
                name: "暂停请求".into(),
                enabled: true,
                priority: 1,
                creation_order: 1,
                channel_text: "全部".into(),
                stage_text: "请求".into(),
                match_summary: "0 个条件".into(),
                action_summary: "1 个动作".into(),
                hit_count: 0,
                last_hit_at: None,
                ui_tone: UiTone::Positive,
            },
            draft: AppRuleDraft {
                rule_id: Some(id),
                expected_revision: Some(1),
                name: "暂停请求".into(),
                description: String::new(),
                enabled: true,
                priority: 1,
                channel: None,
                stage: Some(AppMessageStage::Request),
                conditions: Vec::new(),
                actions: vec![gmofg_proxy_application::RuleAction::Pause],
                one_shot: false,
            },
        }
    }

    fn one_shot_delay_rule() -> RuleViewModel {
        let mut rule = pause_rule();
        rule.summary.name = "一次性延迟".into();
        rule.draft.name = "一次性延迟".into();
        rule.draft.actions = vec![gmofg_proxy_application::RuleAction::Delay { milliseconds: 25 }];
        rule.draft.one_shot = true;
        rule
    }

    fn tls_fingerprint_reject_rule(fingerprint: &str) -> RuleViewModel {
        let id = Uuid::new_v4();
        RuleViewModel {
            summary: RuleSummaryViewModel {
                rule_id: id,
                revision: 1,
                name: "拒绝指定证书".into(),
                enabled: true,
                priority: 1,
                creation_order: 1,
                channel_text: "全部".into(),
                stage_text: "TLS 握手".into(),
                match_summary: "证书指纹".into(),
                action_summary: "拒绝 TLS".into(),
                hit_count: 0,
                last_hit_at: None,
                ui_tone: UiTone::Positive,
            },
            draft: AppRuleDraft {
                rule_id: Some(id),
                expected_revision: Some(1),
                name: "拒绝指定证书".into(),
                description: String::new(),
                enabled: true,
                priority: 1,
                channel: None,
                stage: Some(AppMessageStage::TlsHandshake),
                conditions: vec![gmofg_proxy_application::RuleCondition::Field {
                    field: gmofg_proxy_application::RuleMatchField::CertificateFingerprint,
                    operator: gmofg_proxy_application::RuleMatchOperator::Equals {
                        value: fingerprint.into(),
                    },
                }],
                actions: vec![gmofg_proxy_application::RuleAction::Terminal {
                    action: gmofg_proxy_application::RuleTerminalAction::RejectTlsHandshake,
                }],
                one_shot: false,
            },
        }
    }

    fn adapter(views: Vec<RuleViewModel>, max_sessions: usize) -> Arc<RuntimePipelineAdapter> {
        let rules = views
            .into_iter()
            .map(view_to_domain_rule)
            .collect::<ProxyResult<Vec<_>>>()
            .expect("valid test rules");
        Arc::new(RuntimePipelineAdapter::new(
            test_product_hooks(),
            Arc::new(StaticRules {
                snapshot: Mutex::new(RuleRuntimeSnapshot::new(rules)),
            }),
            Arc::new(InMemorySessionStore::new(max_sessions, 64 * 1024 * 1024)),
            Arc::new(BreakpointCoordinator::default()),
            Arc::new(EventHub::new(128)),
            Arc::new(CaptureRepositoryAdapter::default()),
        ))
    }

    fn transaction_channel() -> ChannelId {
        ChannelId::new("transaction").expect("valid transaction channel")
    }

    fn dll_channel() -> ChannelId {
        ChannelId::new("dll").expect("valid DLL channel")
    }

    fn test_context(epoch: Uuid, connection_id: Uuid, channel: ChannelId) -> ConnectionContext {
        ConnectionContext {
            runtime_epoch: epoch,
            connection_id,
            channel,
            peer_addr: "10.0.0.2:12345".parse::<SocketAddr>().expect("address"),
            accepted_at: SystemTime::now(),
            tls_peer: Some(TlsPeerIdentity {
                sha256_fingerprint: "AA:BB:CC:DD:EE:FF".into(),
                subject_summary: "CN=Test Client".into(),
            }),
        }
    }

    fn request_message(body: &str) -> Message {
        Message {
            start_line: "POST /payment HTTP/1.1".into(),
            headers: vec![
                RawHeader::new(b"host".to_vec(), b"example.test".to_vec()),
                RawHeader::new(b"x-request-id".to_vec(), b"REQ-1".to_vec()),
            ],
            body: body.as_bytes().to_vec().into(),
            body_modified: false,
        }
    }

    fn response_message() -> Message {
        Message {
            start_line: "HTTP/1.1 200 OK".into(),
            headers: vec![RawHeader::new(b"x-server".to_vec(), b"gmo-fg".to_vec())],
            body: br#"{"result":"ok"}"#.to_vec().into(),
            body_modified: false,
        }
    }

    #[tokio::test]
    async fn records_request_response_terminal_events_and_real_metrics() {
        let pipeline = adapter(Vec::new(), 10);
        let epoch = Uuid::new_v4();
        let context = test_context(epoch, Uuid::new_v4(), transaction_channel());

        pipeline.connection_opened(&context).await;
        let opened = pipeline.snapshot(Some(epoch)).await.expect("metrics");
        assert_eq!(opened.channels[&transaction_channel()].connected_clients, 1);

        let mut request = request_message(r#"{"amount":100}"#);
        assert!(
            pipeline
                .request(&context, &mut request)
                .await
                .expect("request")
                .is_empty()
        );
        let running = pipeline.snapshot(Some(epoch)).await.expect("metrics");
        assert_eq!(running.channels[&transaction_channel()].request_count, 1);
        assert_eq!(running.active_sessions, 1);

        let mut response = response_message();
        assert!(
            pipeline
                .response(&context, &mut response)
                .await
                .expect("response")
                .is_empty()
        );
        let session_id = pipeline
            .state
            .lock()
            .connections
            .get(&context.connection_id)
            .and_then(|connection| connection.session_id)
            .expect("active session");
        let recorded = pipeline
            .sessions
            .get_record(session_id)
            .expect("recorded session");
        let recorded_request = recorded.detail.request.as_ref().expect("request");
        assert_eq!(recorded_request.http_status, None);
        assert_eq!(recorded_request.start_line_bytes, b"POST /payment HTTP/1.1");
        assert_eq!(recorded_request.headers["x-request-id"], ["REQ-1"]);
        assert_eq!(
            recorded_request.raw_headers,
            vec![
                RawHttpHeaderViewModel {
                    name_bytes: b"host".to_vec(),
                    value_bytes: b"example.test".to_vec(),
                    leading_ows_bytes: b" ".to_vec(),
                    trailing_ows_bytes: Vec::new(),
                },
                RawHttpHeaderViewModel {
                    name_bytes: b"x-request-id".to_vec(),
                    value_bytes: b"REQ-1".to_vec(),
                    leading_ows_bytes: b" ".to_vec(),
                    trailing_ows_bytes: Vec::new(),
                },
            ]
        );
        let recorded_response = recorded.detail.response.as_ref().expect("response");
        assert_eq!(recorded_response.http_status, Some(200));
        assert_eq!(recorded.detail.summary.http_status, Some(200));
        assert_eq!(recorded_response.start_line_bytes, b"HTTP/1.1 200 OK");
        assert_eq!(recorded_response.headers["x-server"], ["gmo-fg"]);
        assert_eq!(
            recorded_response.raw_headers,
            vec![RawHttpHeaderViewModel {
                name_bytes: b"x-server".to_vec(),
                value_bytes: b"gmo-fg".to_vec(),
                leading_ows_bytes: b" ".to_vec(),
                trailing_ows_bytes: Vec::new(),
            }]
        );
        pipeline.connection_closed(&context, &Ok(())).await;

        let closed = pipeline.snapshot(Some(epoch)).await.expect("metrics");
        assert_eq!(closed.channels[&transaction_channel()].connected_clients, 0);
        assert_eq!(closed.active_sessions, 0);
        assert!(closed.logical_memory_bytes > 0);
        assert!(pipeline.events.current_cursor() > 0);
        assert_eq!(pipeline.sessions.len(), 1);
        let session_id = pipeline
            .state
            .lock()
            .connections
            .get(&context.connection_id)
            .and_then(|connection| connection.session_id);
        assert!(session_id.is_none(), "closed connection state is removed");

        let next_context = test_context(Uuid::new_v4(), Uuid::new_v4(), transaction_channel());
        pipeline.connection_opened(&next_context).await;
        let next_epoch = pipeline
            .snapshot(Some(next_context.runtime_epoch))
            .await
            .expect("next epoch metrics");
        assert_eq!(
            next_epoch.channels[&transaction_channel()].request_count,
            0,
            "runtime counters reset for a new epoch"
        );
        pipeline.connection_closed(&next_context, &Ok(())).await;
    }

    #[tokio::test]
    async fn pending_breakpoints_are_never_evicted_and_stop_unblocks_waiters() {
        let pipeline = adapter(vec![pause_rule()], 1);
        let epoch = Uuid::new_v4();
        let first_context = test_context(epoch, Uuid::new_v4(), transaction_channel());
        pipeline.connection_opened(&first_context).await;

        let first = {
            let pipeline = Arc::clone(&pipeline);
            let context = first_context.clone();
            tokio::spawn(async move {
                let mut message = request_message(r#"{"requestId":"first"}"#);
                pipeline.request(&context, &mut message).await
            })
        };
        for _ in 0..100 {
            if pipeline.breakpoints.query(Some(epoch)).len() == 1 {
                break;
            }
            tokio::task::yield_now().await;
        }
        assert_eq!(pipeline.breakpoints.query(Some(epoch)).len(), 1);

        let second_context = test_context(epoch, Uuid::new_v4(), dll_channel());
        pipeline.connection_opened(&second_context).await;
        let mut second_message = request_message(r#"{"requestId":"second"}"#);
        let exhausted = pipeline
            .request(&second_context, &mut second_message)
            .await
            .expect_err("pending session consumes the full capacity");
        assert_eq!(exhausted.code, "RESOURCE_EXHAUSTED");
        assert!(pipeline.events.replay_after(0).events.iter().any(|event| {
            matches!(
                event.payload,
                UiEventPayload::ResourceWarning { ref message }
                    if message.contains("容量")
            )
        }));

        pipeline.runtime_stopping(epoch).await;
        let stopped = first.await.expect("request task").expect_err("stopped");
        assert_eq!(stopped.code, ErrorCode::ProxyStopped.as_str());
        assert!(pipeline.breakpoints.query(Some(epoch)).is_empty());
    }

    #[tokio::test]
    async fn one_shot_action_is_not_returned_when_runtime_commit_fails() {
        let rule = view_to_domain_rule(one_shot_delay_rule()).expect("rule");
        let rules = Arc::new(RejectingCommitRules {
            snapshot: Mutex::new(RuleRuntimeSnapshot::new(vec![rule])),
            reject_commit: AtomicBool::new(true),
        });
        let pipeline = RuntimePipelineAdapter::new(
            test_product_hooks(),
            rules.clone(),
            Arc::new(InMemorySessionStore::new(10, 64 * 1024 * 1024)),
            Arc::new(BreakpointCoordinator::default()),
            Arc::new(EventHub::new(128)),
            Arc::new(CaptureRepositoryAdapter::default()),
        );
        let epoch = Uuid::new_v4();
        let context = test_context(epoch, Uuid::new_v4(), transaction_channel());
        pipeline.connection_opened(&context).await;
        let mut message = request_message(r#"{"amount":100}"#);

        let error = pipeline
            .request(&context, &mut message)
            .await
            .expect_err("commit failure must fail closed before returning actions");
        assert_eq!(error.code, "REVISION_CONFLICT");
        let persisted = rules.snapshot.lock();
        assert!(persisted.rules[0].enabled);
        assert_eq!(persisted.rules[0].hit_count, 0);
        assert!(pipeline.events.replay_after(0).events.iter().any(|event| {
            matches!(
                event.payload,
                UiEventPayload::OperationFailed(ref error)
                    if error.code == "REVISION_CONFLICT"
            )
        }));
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 4)]
    async fn concurrent_rule_hits_commit_without_lost_updates() {
        let rule = view_to_domain_rule({
            let mut view = one_shot_delay_rule();
            view.draft.one_shot = false;
            view
        })
        .expect("rule");
        let rules = Arc::new(StaticRules {
            snapshot: Mutex::new(RuleRuntimeSnapshot::new(vec![rule])),
        });
        let pipeline = Arc::new(RuntimePipelineAdapter::new(
            test_product_hooks(),
            rules.clone(),
            Arc::new(InMemorySessionStore::new(32, 64 * 1024 * 1024)),
            Arc::new(BreakpointCoordinator::default()),
            Arc::new(EventHub::new(512)),
            Arc::new(CaptureRepositoryAdapter::default()),
        ));
        let epoch = Uuid::new_v4();
        let mut tasks = Vec::new();
        for index in 0..20_u128 {
            let pipeline = pipeline.clone();
            let context = test_context(epoch, Uuid::from_u128(index + 1), transaction_channel());
            pipeline.connection_opened(&context).await;
            tasks.push(tokio::spawn(async move {
                let mut message = request_message(r#"{"amount":100}"#);
                pipeline.request(&context, &mut message).await
            }));
        }
        for task in tasks {
            let actions = task.await.expect("task").expect("request");
            assert_eq!(actions, vec![FaultAction::Delay(Duration::from_millis(25))]);
        }
        assert_eq!(
            rules.snapshot.lock().rules[0].hit_count,
            20,
            "serialized evaluate+commit preserves every concurrent hit"
        );
    }

    #[test]
    fn nth_hit_retry_preserves_prior_count_without_double_counting_current_message() {
        let rule = view_to_domain_rule({
            let mut view = one_shot_delay_rule();
            view.draft.conditions =
                vec![gmofg_proxy_application::RuleCondition::NthHit { count: 2 }];
            view.draft.one_shot = false;
            view
        })
        .expect("rule");
        let rules = Arc::new(ConflictOnceRules {
            snapshot: Mutex::new(RuleRuntimeSnapshot::new(vec![rule])),
            conflict_once: AtomicBool::new(true),
            commit_attempts: AtomicUsize::new(0),
        });
        let pipeline = RuntimePipelineAdapter::new(
            test_product_hooks(),
            rules.clone(),
            Arc::new(InMemorySessionStore::new(10, 64 * 1024 * 1024)),
            Arc::new(BreakpointCoordinator::default()),
            Arc::new(EventHub::new(128)),
            Arc::new(CaptureRepositoryAdapter::default()),
        );
        let epoch = Uuid::new_v4();
        let context = test_context(epoch, Uuid::new_v4(), transaction_channel());
        let message = request_message(r#"{"amount":100}"#);

        let first = pipeline
            .evaluate(&context, DomainMessageStage::Request, Some(&message))
            .expect("first evaluation");
        assert!(
            first.actions.is_empty(),
            "the first NthHit(2) evaluation only advances the in-memory counter"
        );
        assert_eq!(rules.commit_attempts.load(AtomicOrdering::Acquire), 0);

        let second = pipeline
            .evaluate(&context, DomainMessageStage::Request, Some(&message))
            .expect("second evaluation retries after the injected conflict");
        assert_eq!(
            second.actions,
            vec![RuleAction::Delay { milliseconds: 25 }],
            "the same second request must still hit after rollback and re-evaluation"
        );
        assert_eq!(
            rules.commit_attempts.load(AtomicOrdering::Acquire),
            2,
            "one conflicting CAS and one successful retry are expected"
        );
        {
            let persisted = rules.snapshot.lock();
            assert_eq!(persisted.rules[0].hit_count, 1);
            assert_eq!(
                persisted.collection_revision, 2,
                "the external advance and successful retry each advance once"
            );
        }

        let third = pipeline
            .evaluate(&context, DomainMessageStage::Request, Some(&message))
            .expect("third evaluation");
        assert!(
            third.actions.is_empty(),
            "the retry must not count the second request twice"
        );
        assert_eq!(
            rules.snapshot.lock().rules[0].hit_count,
            1,
            "only the exact second hit executes and persists"
        );
    }

    #[test]
    fn tls_handshake_policy_matches_the_peer_under_current_verification() {
        let fingerprint = "11:22:33:44";
        let pipeline = adapter(vec![tls_fingerprint_reject_rule(fingerprint)], 10);
        let epoch = Uuid::new_v4();
        let mut context = test_context(epoch, Uuid::new_v4(), transaction_channel());
        context.tls_peer = None;
        let matching_peer = TlsPeerIdentity {
            sha256_fingerprint: fingerprint.into(),
            subject_summary: "CN=blocked".into(),
        };
        assert!(
            pipeline
                .reject_tls_handshake(&context, &matching_peer)
                .expect("policy")
        );

        let other_peer = TlsPeerIdentity {
            sha256_fingerprint: "AA:BB".into(),
            subject_summary: "CN=allowed".into(),
        };
        assert!(
            !pipeline
                .reject_tls_handshake(&context, &other_peer)
                .expect("policy")
        );
    }

    #[test]
    fn rule_mutations_use_injected_codec_and_preserve_action_order() {
        let body_codec = test_body_codec();
        let mut message = request_message(r#"{"payment":{"amount":100}}"#);
        let actions = vec![
            RuleAction::SetJsonField {
                path: "$.payment.amount".into(),
                value: json!(200),
            },
            RuleAction::SetHeader {
                name: "x-test".into(),
                value: "yes".into(),
            },
            RuleAction::Delay { milliseconds: 25 },
            RuleAction::Pause,
        ];
        let (faults, pause) =
            apply_rule_actions(body_codec.as_ref(), &mut message, &actions, 42).expect("apply");
        assert!(pause);
        assert_eq!(faults, vec![FaultAction::Delay(Duration::from_millis(25))]);
        assert_eq!(
            decode_json(body_codec.as_ref(), &message.body).expect("json")["payment"]["amount"],
            200
        );
        assert_eq!(message.declared_content_length(), Some(message.body.len()));
        assert_eq!(header_value(&message, "x-test").as_deref(), Some("yes"));

        let mock = map_terminal_action(&TerminalAction::MockResponse {
            status: 503,
            headers: vec![("x-mock".into(), "enabled".into())],
            body_bytes: br#"{"mock":true}"#.to_vec(),
        })
        .expect("mock");
        let FaultAction::MockResponse {
            status,
            headers,
            body,
        } = mock
        else {
            panic!("expected mock action");
        };
        assert_eq!(status.as_u16(), 503);
        assert_eq!(headers["x-mock"], "enabled");
        assert_eq!(body, Bytes::from_static(br#"{"mock":true}"#));
    }

    // RULE-008~009, ACTION-012~013, MESSAGE-004~006, TEST-RULE:
    // later body/header mutations win and Rust rebuilds Content-Length exactly once.
    #[test]
    fn body_replacement_and_repeated_header_updates_preserve_action_order() {
        let body_codec = test_body_codec();
        let mut message = request_message(r#"{"original":true}"#);
        message
            .headers
            .push(RawHeader::new(b"x-test".to_vec(), b"old".to_vec()));
        let actions = vec![
            RuleAction::ReplaceBodyText("最初".into()),
            RuleAction::ReplaceBodyText("最終".into()),
            RuleAction::SetHeader {
                name: "x-test".into(),
                value: "first".into(),
            },
            RuleAction::SetHeader {
                name: "x-test".into(),
                value: "last".into(),
            },
        ];

        let (faults, pause) = apply_rule_actions(body_codec.as_ref(), &mut message, &actions, 42)
            .expect("apply mutations");
        assert!(faults.is_empty());
        assert!(!pause);
        assert_eq!(
            decode_body(body_codec.as_ref(), &message.body).expect("decode"),
            "最終"
        );
        assert_eq!(message.declared_content_length(), Some(message.body.len()));
        assert_eq!(header_value(&message, "x-test").as_deref(), Some("last"));
        assert_eq!(
            message
                .headers
                .iter()
                .filter(|header| header.name.eq_ignore_ascii_case(b"x-test"))
                .count(),
            1,
            "SetHeader replaces all earlier values for the same header"
        );
    }

    // ACTION-001~011, TEST-FAULT:
    // all terminal domain actions map to one explicit transport disposition.
    #[test]
    fn every_terminal_action_maps_to_the_expected_transport_fault() {
        let cases = vec![
            (TerminalAction::RejectTlsHandshake, FaultAction::RejectTls),
            (
                TerminalAction::DisconnectBeforeUpstream,
                FaultAction::DisconnectBeforeUpstream,
            ),
            (
                TerminalAction::UpstreamConnectTimeout { milliseconds: 1 },
                FaultAction::UpstreamConnectTimeout(Duration::from_millis(1)),
            ),
            (
                TerminalAction::UpstreamWriteTimeout { milliseconds: 1 },
                FaultAction::UpstreamWriteTimeout(Duration::from_millis(1)),
            ),
            (
                TerminalAction::UpstreamReadTimeout { milliseconds: 1 },
                FaultAction::UpstreamReadTimeout(Duration::from_millis(1)),
            ),
            (
                TerminalAction::DropUpstreamResponse {
                    mode: DropResponseMode::ReadCompleteResponse,
                },
                FaultAction::DropResponse {
                    read_upstream: true,
                },
            ),
            (
                TerminalAction::DropUpstreamResponse {
                    mode: DropResponseMode::CloseAfterRequestWrite,
                },
                FaultAction::DropResponse {
                    read_upstream: false,
                },
            ),
            (
                TerminalAction::InvalidJson {
                    body_bytes: b"{".to_vec(),
                },
                FaultAction::ReplaceBody {
                    body: Bytes::from_static(b"{"),
                },
            ),
            (
                TerminalAction::IncorrectContentLength { delta: -1 },
                FaultAction::ContentLengthOffset(-1),
            ),
            (
                TerminalAction::TruncateResponse { bytes: 2 },
                FaultAction::TruncateResponse(2),
            ),
        ];
        for (domain, expected) in cases {
            assert_eq!(
                map_terminal_action(&domain).expect("map terminal action"),
                expected,
                "unexpected transport mapping for {domain:?}"
            );
        }

        let mock = map_terminal_action(&TerminalAction::MockResponse {
            status: 202,
            headers: vec![("x-mock".into(), "yes".into())],
            body_bytes: br#"{"mock":true}"#.to_vec(),
        })
        .expect("map mock response");
        let FaultAction::MockResponse {
            status,
            headers,
            body,
        } = mock
        else {
            panic!("expected mock response transport fault");
        };
        assert_eq!(status.as_u16(), 202);
        assert_eq!(headers["x-mock"], "yes");
        assert_eq!(body, Bytes::from_static(br#"{"mock":true}"#));
    }
}
