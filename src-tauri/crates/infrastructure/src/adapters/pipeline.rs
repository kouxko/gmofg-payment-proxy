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
use chrono::{DateTime, Utc};
use gmofg_proxy_application::{
    AppError, AppResult, BreakpointCoordinator, BreakpointDecision, BreakpointDecisionKind,
    BreakpointDetailViewModel, BreakpointOutcome, BreakpointState, BreakpointSummaryViewModel,
    CaptureRowViewModel, ChannelKind as AppChannelKind, DisabledReason, EventHub,
    InMemorySessionStore, MessageContentViewModel, MessageStage as AppMessageStage,
    RuleSummaryViewModel, SessionDetailViewModel, SessionRecord, SessionStore,
    SessionSummaryViewModel, UiEventPayload, UiTone,
};
use gmofg_proxy_domain::{
    ChannelKind, DropResponseMode, MatchContext, MessageStage as DomainMessageStage, Rule,
    RuleAction, RuleEngine, RuleRuntimeSnapshot, RuntimeEpoch, TerminalAction, TerminalIdentity,
};
use gmofg_proxy_runtime::{
    Channel, ChannelRuntimeMetrics, ConnectionContext, ErrorCode, FaultAction, HandshakePolicy,
    Message, PipelinePorts, ProxyError, RawHeader, Result as ProxyResult, RuntimeMetricsProvider,
    RuntimeMetricsSnapshot, TlsPeerIdentity,
};
use parking_lot::Mutex;
use serde_json::Value;
use uuid::Uuid;

use super::{CaptureRepositoryAdapter, RuleRepositoryAdapter};

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
    ) -> AppResult<()>;
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
    ) -> AppResult<()> {
        RuleRepositoryAdapter::commit_runtime_snapshot(self, snapshot, evaluated_rules)
    }

    fn reset_runtime_hit_metadata(&self) -> AppResult<()> {
        RuleRepositoryAdapter::reset_runtime_hit_metadata(self)
    }
}

/// One adapter instance is shared by both listeners for the lifetime of the app.
#[derive(Debug)]
pub struct RuntimePipelineAdapter {
    rules: Arc<dyn RuntimeRuleRepository>,
    sessions: Arc<InMemorySessionStore>,
    breakpoints: Arc<BreakpointCoordinator>,
    events: Arc<EventHub>,
    captures: Arc<CaptureRepositoryAdapter>,
    capture_cursor: AtomicU64,
    rule_epoch: Mutex<Option<Uuid>>,
    state: Mutex<PipelineState>,
}

#[derive(Debug, Default)]
struct PipelineState {
    connections: HashMap<Uuid, ConnectionRuntime>,
    live_sessions: HashMap<Uuid, LiveSession>,
    rule_runtime: Option<RuleRuntime>,
    metrics_epoch: Option<Uuid>,
    channels: BTreeMap<Channel, ChannelRuntimeMetrics>,
}

#[derive(Debug)]
struct ConnectionRuntime {
    channel: Channel,
    session_id: Option<Uuid>,
    pending_breakpoints: Vec<Uuid>,
}

#[derive(Debug, Clone)]
struct LiveSession {
    started_at: DateTime<Utc>,
    runtime_epoch: Uuid,
}

#[derive(Debug)]
struct RuleRuntime {
    epoch: Uuid,
    snapshot: RuleRuntimeSnapshot,
    engine: RuleEngine,
}

#[derive(Debug)]
struct EvaluatedRules {
    actions: Vec<RuleAction>,
    traces: Vec<String>,
    matched_ids: Vec<Uuid>,
    hit_rules: Vec<RuleSummaryViewModel>,
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
        rules: Arc<dyn RuntimeRuleRepository>,
        sessions: Arc<InMemorySessionStore>,
        breakpoints: Arc<BreakpointCoordinator>,
        events: Arc<EventHub>,
        captures: Arc<CaptureRepositoryAdapter>,
    ) -> Self {
        Self {
            rules,
            sessions,
            breakpoints,
            events,
            captures,
            capture_cursor: AtomicU64::new(0),
            rule_epoch: Mutex::new(None),
            state: Mutex::new(PipelineState::default()),
        }
    }

    fn evaluate(
        &self,
        context: &ConnectionContext,
        stage: DomainMessageStage,
        message: Option<&Message>,
    ) -> ProxyResult<EvaluatedRules> {
        self.ensure_rule_epoch(context.runtime_epoch)?;
        let terminal = TerminalIdentity {
            source_ip: context.peer_addr.ip().to_string(),
            certificate_sha256: context
                .tls_peer
                .as_ref()
                .map_or_else(String::new, |identity| identity.sha256_fingerprint.clone()),
        };
        let json = message.and_then(|message| message.parse_shift_jis_json().ok());
        let target = message.and_then(|message| message_target(&message.start_line));
        let runtime_epoch = RuntimeEpoch::from_uuid(context.runtime_epoch);
        // Evaluation and its durable runtime metadata commit are one serialized
        // operation. Actions are never returned to the transport until the
        // corresponding hit count / one-shot disable transaction commits.
        let mut pipeline_state = self.state.lock();
        let snapshot = self.rules.runtime_snapshot().map_err(app_to_proxy)?;
        if pipeline_state
            .rule_runtime
            .as_ref()
            .is_none_or(|runtime| runtime.epoch != context.runtime_epoch)
        {
            pipeline_state.rule_runtime = Some(RuleRuntime {
                epoch: context.runtime_epoch,
                engine: RuleEngine::new(runtime_epoch, snapshot.rules.clone()),
                snapshot,
            });
        } else if let Some(runtime) = pipeline_state.rule_runtime.as_mut()
            && runtime.snapshot.signature != snapshot.signature
        {
            runtime.engine.reconcile(snapshot.rules.clone());
            runtime.snapshot = snapshot;
        }
        let runtime = pipeline_state
            .rule_runtime
            .as_mut()
            .expect("rule runtime was initialized");
        let evaluation = runtime.engine.evaluate(
            &MatchContext {
                runtime_epoch,
                channel: domain_channel(context.channel),
                stage,
                terminal: &terminal,
                path_or_request_type: target,
                json_body: json.as_ref(),
            },
            Utc::now(),
        );
        let hit_rules = matched_rule_summaries(&evaluation, runtime.engine.rules());
        let matched = evaluation.traces.iter().any(|trace| trace.matched);
        if matched {
            let base_snapshot = runtime.snapshot.clone();
            let evaluated_rules = runtime.engine.rules().to_vec();
            if let Err(error) = self
                .rules
                .commit_runtime_snapshot(&base_snapshot, &evaluated_rules)
            {
                pipeline_state.rule_runtime = None;
                drop(pipeline_state);
                self.events.publish(
                    Some(context.runtime_epoch),
                    Utc::now(),
                    error.view_model.entity_id.clone(),
                    None,
                    UiEventPayload::OperationFailed((*error.view_model).clone()),
                );
                return Err(app_to_proxy(error));
            }
            runtime.snapshot = RuleRuntimeSnapshot::new(evaluated_rules);
        }

        let traces = rule_trace_text(&evaluation);
        let matched_ids = evaluation
            .traces
            .iter()
            .filter(|trace| trace.matched)
            .map(|trace| trace.rule_id.as_uuid())
            .collect::<Vec<_>>();
        drop(pipeline_state);
        Ok(EvaluatedRules {
            actions: evaluation.composed_actions,
            traces,
            matched_ids,
            hit_rules,
        })
    }

    fn ensure_rule_epoch(&self, epoch: Uuid) -> ProxyResult<()> {
        let mut current = self.rule_epoch.lock();
        if *current != Some(epoch) {
            self.rules
                .reset_runtime_hit_metadata()
                .map_err(app_to_proxy)?;
            self.state.lock().rule_runtime = None;
            *current = Some(epoch);
        }
        Ok(())
    }

    fn publish_rule_hits(&self, epoch: Uuid, rules: Vec<RuleSummaryViewModel>) {
        for rule in rules {
            self.events.publish(
                Some(epoch),
                Utc::now(),
                Some(rule.rule_id.to_string()),
                Some(rule.revision),
                UiEventPayload::RuleHit(rule),
            );
        }
        self.sync_event_capacity(epoch);
    }

    fn sync_event_capacity(&self, epoch: Uuid) {
        let first = self
            .sessions
            .set_pending_ui_event_bytes(self.events.logical_bytes());
        if let Err(error) = first {
            self.events.publish(
                Some(epoch),
                Utc::now(),
                None,
                None,
                UiEventPayload::ResourceWarning {
                    message: error.view_model.message.clone(),
                },
            );
            // The warning is itself retained UI-event data. Synchronize once
            // more without publishing recursively so the logical byte count
            // always equals the actual EventHub state.
            let _ = self
                .sessions
                .set_pending_ui_event_bytes(self.events.logical_bytes());
        }
    }

    fn begin_session(&self, context: &ConnectionContext, original: &Message) -> ProxyResult<Uuid> {
        let now = Utc::now();
        let session_id = Uuid::new_v4();
        let request = content_view(original);
        let request_id = request_id(original).unwrap_or_else(|| session_id.to_string());
        let terminal_ip = context.peer_addr.ip().to_string();
        let fingerprint = fingerprint(context);
        let target = message_target(&original.start_line)
            .unwrap_or_default()
            .to_owned();
        let method = message_method(&original.start_line)
            .unwrap_or_default()
            .to_owned();
        let summary = SessionSummaryViewModel {
            session_id,
            request_id: request_id.clone(),
            started_at: now,
            completed_at: None,
            terminal_ip: terminal_ip.clone(),
            channel: app_channel(context.channel),
            method,
            target,
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
            let metrics = state.channels.entry(context.channel).or_default();
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
            record.detail.request = Some(content_view(effective));
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
            record.detail.response = Some(content_view(effective));
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
        self.sync_event_capacity(context.runtime_epoch);
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
            context,
            stage,
            original,
            effective,
            self.session_id(context)?,
        );
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
        self.sync_event_capacity(context.runtime_epoch);

        let outcome = ticket
            .outcome
            .await
            .unwrap_or(BreakpointOutcome::ClientDisconnected);
        self.remove_pending_breakpoint(context.connection_id, breakpoint_id);
        match outcome {
            BreakpointOutcome::Decision(decision) => {
                let actions = apply_breakpoint_decision(stage, original, effective, &decision)?;
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
                "Payment App 已断开，断点已终止。",
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
                    record.detail.final_action = "响应已返回 Payment App".into();
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
        self.sync_event_capacity(context.runtime_epoch);
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
            channel: summary.channel,
            channel_text: summary.channel.display_zh().into(),
            stage: publication.stage,
            stage_text: publication.stage.display_zh().into(),
            method: summary.method.clone(),
            target: summary.target.clone(),
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
            let metrics = state.channels.entry(context.channel).or_default();
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
        self.publish_rule_hits(verified_context.runtime_epoch, evaluated.hit_rules);
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
        if let Err(error) = self.rules.reset_runtime_hit_metadata() {
            self.events.publish(
                Some(epoch),
                Utc::now(),
                error.view_model.entity_id.clone(),
                None,
                UiEventPayload::OperationFailed((*error.view_model).clone()),
            );
        }
        self.state.lock().rule_runtime = None;
        *self.rule_epoch.lock() = None;
        self.sync_event_capacity(epoch);
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
                channel: context.channel,
                session_id: None,
                pending_breakpoints: Vec::new(),
            },
        );
        let metrics = state.channels.entry(context.channel).or_default();
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
        let (mut actions, pause) = apply_rule_actions(message, &evaluated.actions)?;
        self.publish_rule_hits(context.runtime_epoch, evaluated.hit_rules.clone());
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
            let metrics = state.channels.entry(context.channel).or_default();
            metrics.upstream_response_count = metrics.upstream_response_count.saturating_add(1);
            metrics.last_upstream_error = None;
        }
        let original = message.clone();
        let evaluated = self.evaluate(context, DomainMessageStage::Response, Some(message))?;
        let (mut actions, pause) = apply_rule_actions(message, &evaluated.actions)?;
        self.publish_rule_hits(context.runtime_epoch, evaluated.hit_rules.clone());
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
                .map(|connection| connection.channel)
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

    async fn runtime_fault(&self, epoch: Uuid, channel: Channel, error: &ProxyError) {
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
        self.sync_event_capacity(epoch);
    }
}

fn matched_rule_summaries(
    evaluation: &gmofg_proxy_domain::RuleEvaluation,
    rules: &[Rule],
) -> Vec<RuleSummaryViewModel> {
    evaluation
        .traces
        .iter()
        .filter(|trace| trace.matched)
        .filter_map(|trace| {
            rules
                .iter()
                .find(|rule| rule.id == trace.rule_id)
                .map(rule_summary)
        })
        .collect()
}

fn rule_trace_text(evaluation: &gmofg_proxy_domain::RuleEvaluation) -> Vec<String> {
    evaluation
        .traces
        .iter()
        .map(|trace| {
            format!(
                "{} [{}] {}",
                trace.rule_id,
                if trace.matched { "命中" } else { "未命中" },
                trace.reason
            )
        })
        .collect()
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

fn apply_rule_actions(
    message: &mut Message,
    actions: &[RuleAction],
) -> ProxyResult<(Vec<FaultAction>, bool)> {
    let mut faults = Vec::new();
    let mut pause = false;
    for action in actions {
        match action {
            RuleAction::SetJsonField { path, value } => {
                let mut json = message.parse_shift_jis_json()?;
                set_json_path(&mut json, path, value.clone())?;
                message.replace_json(&json)?;
            }
            RuleAction::ReplaceBodyText(text) => message.replace_shift_jis_text(text)?,
            RuleAction::SetHeader { name, value } => {
                message.remove_header(name);
                message.headers.push(RawHeader {
                    name: name.as_bytes().to_vec().into(),
                    value: value.as_bytes().to_vec().into(),
                });
            }
            RuleAction::Delay { milliseconds } => {
                faults.push(FaultAction::Delay(Duration::from_millis(*milliseconds)));
            }
            RuleAction::Pause => pause = true,
            RuleAction::CustomHttpStatus { status } => {
                faults.push(FaultAction::CustomStatus(proxy_status!(*status)?));
            }
            RuleAction::Terminal(terminal) => faults.push(map_terminal_action(terminal)?),
        }
    }
    if message.body_modified {
        message.set_content_length(message.body.len());
    }
    Ok((faults, pause))
}

fn map_terminal_action(action: &TerminalAction) -> ProxyResult<FaultAction> {
    Ok(match action {
        TerminalAction::RejectTlsHandshake => FaultAction::RejectTls,
        TerminalAction::DisconnectBeforeUpstream => FaultAction::DisconnectBeforeUpstream,
        TerminalAction::UpstreamConnectTimeout { .. } => FaultAction::UpstreamConnectTimeout,
        TerminalAction::UpstreamWriteTimeout { .. } => FaultAction::UpstreamWriteTimeout,
        TerminalAction::UpstreamReadTimeout { .. } => FaultAction::UpstreamReadTimeout,
        TerminalAction::DropUpstreamResponse { mode } => FaultAction::DropResponse {
            read_upstream: *mode == DropResponseMode::ReadCompleteResponse,
        },
        TerminalAction::MockResponse {
            status,
            headers,
            shift_jis_body,
        } => FaultAction::MockResponse {
            status: proxy_status!(*status)?,
            headers: Message {
                start_line: String::new(),
                headers: headers
                    .iter()
                    .map(|(name, value)| RawHeader {
                        name: name.as_bytes().to_vec().into(),
                        value: value.as_bytes().to_vec().into(),
                    })
                    .collect(),
                body: Vec::new().into(),
                body_modified: false,
            }
            .header_map()?,
            shift_jis_body: decode_shift_jis_bytes(shift_jis_body)?,
        },
        TerminalAction::InvalidJson { shift_jis_body } => FaultAction::InvalidJson {
            shift_jis_text: decode_shift_jis_bytes(shift_jis_body)?,
        },
        TerminalAction::IncorrectContentLength { delta } => {
            FaultAction::ContentLengthOffset(*delta)
        }
        TerminalAction::TruncateResponse { bytes } => {
            FaultAction::TruncateResponse(usize::try_from(*bytes).map_err(|_| {
                ProxyError::new(
                    ErrorCode::ConfigInvalid,
                    "truncate size exceeds platform range",
                )
            })?)
        }
    })
}

fn apply_breakpoint_decision(
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
            );
        }
        BreakpointDecisionKind::MockResponse => {
            let message = decision.message.as_ref().ok_or_else(|| {
                ProxyError::new(ErrorCode::ConfigInvalid, "mock response is missing")
            })?;
            let mock = proxy_message(message, "HTTP/1.1 200 OK");
            actions.push(FaultAction::MockResponse {
                status: proxy_status!(decision.http_status.unwrap_or(200))?,
                headers: mock.header_map()?,
                shift_jis_body: message.body_text.clone().ok_or_else(|| {
                    ProxyError::new(ErrorCode::ShiftJisDecodeFailed, "mock body is not text")
                })?,
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
        BreakpointDecisionKind::InvalidJson => actions.push(FaultAction::InvalidJson {
            shift_jis_text: "{invalid-json".into(),
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

fn set_json_path(root: &mut Value, path: &str, value: Value) -> ProxyResult<()> {
    let path = path.strip_prefix("$.").unwrap_or(path);
    let mut segments = path.split('.').peekable();
    let mut current = root;
    while let Some(segment) = segments.next() {
        let (name, indexes) = parse_segment(segment)?;
        if segments.peek().is_none() && indexes.is_empty() {
            let object = current.as_object_mut().ok_or_else(|| {
                ProxyError::new(
                    ErrorCode::ConfigInvalid,
                    "JSON path parent is not an object",
                )
            })?;
            object.insert(name.to_owned(), value);
            return Ok(());
        }
        current = current.get_mut(name).ok_or_else(|| {
            ProxyError::new(
                ErrorCode::ConfigInvalid,
                format!("JSON path not found: {path}"),
            )
        })?;
        for (position, index) in indexes.iter().enumerate() {
            if segments.peek().is_none() && position + 1 == indexes.len() {
                let array = current.as_array_mut().ok_or_else(|| {
                    ProxyError::new(ErrorCode::ConfigInvalid, "JSON path parent is not an array")
                })?;
                let slot = array.get_mut(*index).ok_or_else(|| {
                    ProxyError::new(ErrorCode::ConfigInvalid, "JSON array index is out of range")
                })?;
                *slot = value;
                return Ok(());
            }
            current = current.get_mut(*index).ok_or_else(|| {
                ProxyError::new(ErrorCode::ConfigInvalid, "JSON array index is out of range")
            })?;
        }
    }
    Err(ProxyError::new(
        ErrorCode::ConfigInvalid,
        "JSON path cannot be empty",
    ))
}

fn parse_segment(segment: &str) -> ProxyResult<(&str, Vec<usize>)> {
    let name_end = segment.find('[').unwrap_or(segment.len());
    let name = &segment[..name_end];
    if name.is_empty() {
        return Err(ProxyError::new(
            ErrorCode::ConfigInvalid,
            "JSON path segment cannot be empty",
        ));
    }
    let mut indexes = Vec::new();
    let mut rest = &segment[name_end..];
    while let Some(index_text) = rest.strip_prefix('[') {
        let close = index_text.find(']').ok_or_else(|| {
            ProxyError::new(ErrorCode::ConfigInvalid, "JSON path index is not closed")
        })?;
        indexes.push(index_text[..close].parse().map_err(|_| {
            ProxyError::new(ErrorCode::ConfigInvalid, "JSON path index is invalid")
        })?);
        rest = &index_text[close + 1..];
    }
    if !rest.is_empty() {
        return Err(ProxyError::new(
            ErrorCode::ConfigInvalid,
            "JSON path segment is invalid",
        ));
    }
    Ok((name, indexes))
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
        channel: draft.channel.map(|channel| match channel {
            AppChannelKind::Transaction => ChannelKind::Transaction,
            AppChannelKind::Dll => ChannelKind::Dll,
        }),
        stage,
        conditions,
        actions,
        one_shot: draft.one_shot,
        hit_count: view.summary.hit_count,
        last_hit_at: view.summary.last_hit_at,
    })
}

fn breakpoint_detail(
    context: &ConnectionContext,
    stage: AppMessageStage,
    original: &Message,
    effective: &Message,
    session_id: Uuid,
) -> BreakpointDetailViewModel {
    let title = match stage {
        AppMessageStage::Request => "请求断点·发送至服务器前",
        AppMessageStage::Response => "响应断点·返回 App 前",
        AppMessageStage::TlsHandshake | AppMessageStage::Terminal => "终态",
    };
    BreakpointDetailViewModel {
        summary: BreakpointSummaryViewModel {
            breakpoint_id: Uuid::new_v4(),
            session_id,
            runtime_epoch: context.runtime_epoch,
            stage,
            title: title.into(),
            terminal_ip: context.peer_addr.ip().to_string(),
            channel: app_channel(context.channel),
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
        original: content_view(original),
        effective: content_view(effective),
        can_resolve: true,
        resolve_disabled_reason: None,
        available_actions: Vec::new(),
    }
}

fn rule_summary(rule: &Rule) -> RuleSummaryViewModel {
    RuleSummaryViewModel {
        rule_id: rule.id.as_uuid(),
        revision: rule.revision.get(),
        name: rule.name.clone(),
        enabled: rule.enabled,
        priority: i32::try_from(rule.priority).unwrap_or(i32::MAX),
        creation_order: rule.created_order,
        channel_text: rule.channel.map_or("全部".into(), |channel| match channel {
            ChannelKind::Transaction => "交易".into(),
            ChannelKind::Dll => "DLL".into(),
        }),
        stage_text: match rule.stage {
            DomainMessageStage::Request => "请求",
            DomainMessageStage::Response => "响应",
            DomainMessageStage::TlsHandshake => "TLS 握手",
        }
        .into(),
        match_summary: format!("{} 个条件", rule.conditions.len()),
        action_summary: format!("{} 个动作", rule.actions.len()),
        hit_count: rule.hit_count,
        last_hit_at: rule.last_hit_at,
        ui_tone: if rule.enabled {
            UiTone::Positive
        } else {
            UiTone::Neutral
        },
    }
}

fn content_view(message: &Message) -> MessageContentViewModel {
    let mut headers = BTreeMap::<String, Vec<String>>::new();
    for header in &message.headers {
        headers
            .entry(String::from_utf8_lossy(&header.name).into_owned())
            .or_default()
            .push(String::from_utf8_lossy(&header.value).into_owned());
    }
    let body_text = message.decoded_shift_jis().ok();
    let json = body_text
        .as_deref()
        .and_then(|text| serde_json::from_str(text).ok());
    MessageContentViewModel {
        headers,
        body_text,
        body_bytes: message.body.to_vec(),
        json,
        content_length: message.body.len(),
    }
}

fn proxy_message(view: &MessageContentViewModel, start_line: &str) -> Message {
    Message {
        start_line: start_line.to_owned(),
        headers: view
            .headers
            .iter()
            .flat_map(|(name, values)| {
                values.iter().map(|value| RawHeader {
                    name: name.as_bytes().to_vec().into(),
                    value: value.as_bytes().to_vec().into(),
                })
            })
            .collect(),
        body: view.body_bytes.clone().into(),
        body_modified: true,
    }
}

fn decode_shift_jis_bytes(bytes: &[u8]) -> ProxyResult<String> {
    Message {
        start_line: String::new(),
        headers: Vec::new(),
        body: bytes.to_vec().into(),
        body_modified: false,
    }
    .decoded_shift_jis()
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
    let code = match error.view_model.code.as_str() {
        "SHIFT_JIS_DECODE_FAILED" => ErrorCode::ShiftJisDecodeFailed,
        "SHIFT_JIS_ENCODE_FAILED" => ErrorCode::ShiftJisEncodeFailed,
        "JSON_INVALID" => ErrorCode::JsonInvalid,
        "CONFIG_INVALID" | "RULE_INVALID" => ErrorCode::ConfigInvalid,
        _ => ErrorCode::Internal,
    };
    ProxyError::new(code, error.view_model.message)
}

fn app_channel(channel: Channel) -> AppChannelKind {
    match channel {
        Channel::Transaction => AppChannelKind::Transaction,
        Channel::Dll => AppChannelKind::Dll,
    }
}

fn domain_channel(channel: Channel) -> ChannelKind {
    match channel {
        Channel::Transaction => ChannelKind::Transaction,
        Channel::Dll => ChannelKind::Dll,
    }
}

fn message_method(start_line: &str) -> Option<&str> {
    start_line.split_ascii_whitespace().next()
}

fn message_target(start_line: &str) -> Option<&str> {
    start_line.split_ascii_whitespace().nth(1)
}

fn header_value(message: &Message, name: &str) -> Option<String> {
    message
        .headers
        .iter()
        .find(|header| header.name.eq_ignore_ascii_case(name.as_bytes()))
        .map(|header| String::from_utf8_lossy(&header.value).into_owned())
}

fn request_id(message: &Message) -> Option<String> {
    ["x-request-id", "request-id", "x-correlation-id"]
        .into_iter()
        .find_map(|name| header_value(message, name))
        .or_else(|| {
            message.parse_shift_jis_json().ok().and_then(|json| {
                ["requestId", "request_id", "reqId"]
                    .into_iter()
                    .find_map(|name| json.get(name))
                    .and_then(Value::as_str)
                    .map(str::to_owned)
            })
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
        "BREAKPOINT_PROXY_STOPPED" => "Proxy 停止",
        "TLS_HANDSHAKE_FAILED" => "TLS 失败",
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
        sync::atomic::{AtomicBool, Ordering as AtomicOrdering},
        time::SystemTime,
    };

    use gmofg_proxy_application::{RuleDraft as AppRuleDraft, RuleViewModel};
    use gmofg_proxy_runtime::TlsPeerIdentity;
    use serde_json::json;

    use super::*;

    #[derive(Debug)]
    struct StaticRules {
        snapshot: Mutex<RuleRuntimeSnapshot>,
    }

    #[derive(Debug)]
    struct RejectingCommitRules {
        snapshot: Mutex<RuleRuntimeSnapshot>,
        reject_commit: AtomicBool,
    }

    impl RuntimeRuleRepository for RejectingCommitRules {
        fn runtime_snapshot(&self) -> AppResult<RuleRuntimeSnapshot> {
            Ok(self.snapshot.lock().clone())
        }

        fn commit_runtime_snapshot(&self, _: &RuleRuntimeSnapshot, _: &[Rule]) -> AppResult<()> {
            if self.reject_commit.load(AtomicOrdering::Acquire) {
                Err(AppError::new(
                    "REVISION_CONFLICT",
                    "模拟运行态事务提交失败。",
                ))
            } else {
                Ok(())
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
        ) -> AppResult<()> {
            let mut current = self.snapshot.lock();
            if current.signature != snapshot.signature {
                return Err(AppError::new("REVISION_CONFLICT", "规则测试快照已变化。"));
            }
            *current = RuleRuntimeSnapshot::new(evaluated_rules.to_vec());
            Ok(())
        }

        fn reset_runtime_hit_metadata(&self) -> AppResult<()> {
            let mut current = self.snapshot.lock();
            for rule in &mut current.rules {
                rule.hit_count = 0;
                rule.last_hit_at = None;
            }
            *current = RuleRuntimeSnapshot::new(current.rules.clone());
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
            Arc::new(StaticRules {
                snapshot: Mutex::new(RuleRuntimeSnapshot::new(rules)),
            }),
            Arc::new(InMemorySessionStore::new(max_sessions, 64 * 1024 * 1024)),
            Arc::new(BreakpointCoordinator::default()),
            Arc::new(EventHub::new(128)),
            Arc::new(CaptureRepositoryAdapter::default()),
        ))
    }

    fn test_context(epoch: Uuid, connection_id: Uuid, channel: Channel) -> ConnectionContext {
        ConnectionContext {
            runtime_epoch: epoch,
            connection_id,
            channel,
            peer_addr: "10.0.0.2:12345".parse::<SocketAddr>().expect("address"),
            accepted_at: SystemTime::now(),
            tls_peer: Some(TlsPeerIdentity {
                sha256_fingerprint: "AA:BB:CC:DD:EE:FF".into(),
                subject_summary: "CN=Payment App".into(),
            }),
        }
    }

    fn request_message(body: &str) -> Message {
        Message {
            start_line: "POST /payment HTTP/1.1".into(),
            headers: vec![
                RawHeader {
                    name: b"host".to_vec().into(),
                    value: b"example.test".to_vec().into(),
                },
                RawHeader {
                    name: b"x-request-id".to_vec().into(),
                    value: b"REQ-1".to_vec().into(),
                },
            ],
            body: body.as_bytes().to_vec().into(),
            body_modified: false,
        }
    }

    fn response_message() -> Message {
        Message {
            start_line: "HTTP/1.1 200 OK".into(),
            headers: Vec::new(),
            body: br#"{"result":"ok"}"#.to_vec().into(),
            body_modified: false,
        }
    }

    #[tokio::test]
    async fn records_request_response_terminal_events_and_real_metrics() {
        let pipeline = adapter(Vec::new(), 10);
        let epoch = Uuid::new_v4();
        let context = test_context(epoch, Uuid::new_v4(), Channel::Transaction);

        pipeline.connection_opened(&context).await;
        let opened = pipeline.snapshot(Some(epoch)).await.expect("metrics");
        assert_eq!(opened.channels[&Channel::Transaction].connected_clients, 1);

        let mut request = request_message(r#"{"amount":100}"#);
        assert!(
            pipeline
                .request(&context, &mut request)
                .await
                .expect("request")
                .is_empty()
        );
        let running = pipeline.snapshot(Some(epoch)).await.expect("metrics");
        assert_eq!(running.channels[&Channel::Transaction].request_count, 1);
        assert_eq!(running.active_sessions, 1);

        let mut response = response_message();
        assert!(
            pipeline
                .response(&context, &mut response)
                .await
                .expect("response")
                .is_empty()
        );
        pipeline.connection_closed(&context, &Ok(())).await;

        let closed = pipeline.snapshot(Some(epoch)).await.expect("metrics");
        assert_eq!(closed.channels[&Channel::Transaction].connected_clients, 0);
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

        let next_context = test_context(Uuid::new_v4(), Uuid::new_v4(), Channel::Transaction);
        pipeline.connection_opened(&next_context).await;
        let next_epoch = pipeline
            .snapshot(Some(next_context.runtime_epoch))
            .await
            .expect("next epoch metrics");
        assert_eq!(
            next_epoch.channels[&Channel::Transaction].request_count,
            0,
            "runtime counters reset for a new epoch"
        );
        pipeline.connection_closed(&next_context, &Ok(())).await;
    }

    #[tokio::test]
    async fn pending_breakpoints_are_never_evicted_and_stop_unblocks_waiters() {
        let pipeline = adapter(vec![pause_rule()], 1);
        let epoch = Uuid::new_v4();
        let first_context = test_context(epoch, Uuid::new_v4(), Channel::Transaction);
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

        let second_context = test_context(epoch, Uuid::new_v4(), Channel::Dll);
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
            rules.clone(),
            Arc::new(InMemorySessionStore::new(10, 64 * 1024 * 1024)),
            Arc::new(BreakpointCoordinator::default()),
            Arc::new(EventHub::new(128)),
            Arc::new(CaptureRepositoryAdapter::default()),
        );
        let epoch = Uuid::new_v4();
        let context = test_context(epoch, Uuid::new_v4(), Channel::Transaction);
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
            let context = test_context(epoch, Uuid::from_u128(index + 1), Channel::Transaction);
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
    fn tls_handshake_policy_matches_the_peer_under_current_verification() {
        let fingerprint = "11:22:33:44";
        let pipeline = adapter(vec![tls_fingerprint_reject_rule(fingerprint)], 10);
        let epoch = Uuid::new_v4();
        let mut context = test_context(epoch, Uuid::new_v4(), Channel::Transaction);
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
    fn rule_mutations_preserve_shift_jis_rebuild_and_action_order() {
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
        let (faults, pause) = apply_rule_actions(&mut message, &actions).expect("apply");
        assert!(pause);
        assert_eq!(faults, vec![FaultAction::Delay(Duration::from_millis(25))]);
        assert_eq!(
            message.parse_shift_jis_json().expect("json")["payment"]["amount"],
            200
        );
        assert_eq!(message.declared_content_length(), Some(message.body.len()));
        assert_eq!(header_value(&message, "x-test").as_deref(), Some("yes"));

        let mock = map_terminal_action(&TerminalAction::MockResponse {
            status: 503,
            headers: vec![("x-mock".into(), "enabled".into())],
            shift_jis_body: br#"{"mock":true}"#.to_vec(),
        })
        .expect("mock");
        let FaultAction::MockResponse {
            status,
            headers,
            shift_jis_body,
        } = mock
        else {
            panic!("expected mock action");
        };
        assert_eq!(status.as_u16(), 503);
        assert_eq!(headers["x-mock"], "enabled");
        assert_eq!(shift_jis_body, r#"{"mock":true}"#);
    }
}
