//! 代理传输运行时与应用/领域服务之间的生产桥接层。
//!
//! 只有本模块同时理解“保留原始字节的代理消息”和应用/领域模型：请求按固定阶段经过抓包、
//! 断点、规则匹配和动作执行。每个 epoch 使用快照隔离；锁只包围共享元数据，不跨网络
//! `await` 长时间持有。Tauri 只装配本适配器，不执行 pipeline 策略。

use std::{
    collections::{BTreeMap, HashMap, HashSet},
    sync::{
        Arc,
        atomic::{AtomicU64, Ordering},
    },
    time::Duration,
};

use bytes::Bytes;
use chrono::{DateTime, Utc};
use intercept_proxy_application::{
    AppError, AppResult, BreakpointCoordinator, BreakpointDecision, BreakpointDecisionKind,
    BreakpointDetailViewModel, BreakpointOutcome, BreakpointState, BreakpointSummaryViewModel,
    CaptureRowViewModel, ChannelId as AppChannelId, DisabledReason, EventHub, InMemorySessionStore,
    MessageContentViewModel, MessageStage as AppMessageStage, SessionDetailViewModel,
    SessionRecord, SessionStore, SessionSummaryViewModel, UiEventPayload, UiTone,
};
use intercept_proxy_domain::{
    ChannelId as DomainChannelId, MessageStage as DomainMessageStage, Rule, RuleAction,
    RuleRuntimeSnapshot, RuntimeEpoch, TerminalAction,
};
use intercept_proxy_product_api::{BodyCodec, RequestClassifier};
use intercept_proxy_runtime::{
    ChannelId, ChannelRuntimeMetrics, ConnectionContext, ErrorCode, FaultAction, HandshakePolicy,
    Message, PipelinePorts, ProxyError, Result as ProxyResult, RuntimeMetricsProvider,
    RuntimeMetricsSnapshot, TlsPeerIdentity, UpstreamSecurityEvidence,
    fault::{mock_response, project_response_for_observation},
};
use parking_lot::Mutex;
use uuid::Uuid;

use super::{CaptureRepositoryAdapter, RuleRepositoryAdapter};
use message_projection::{
    breakpoint_content_view, classify_request, content_view, decode_json, encode_body,
    header_value, message_method, message_target, proxy_message,
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
mod upstream_security;

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
    fn runtime_snapshot(&self, channel: &ChannelId) -> AppResult<RuleRuntimeSnapshot>;
    fn commit_runtime_snapshot(
        &self,
        snapshot: &RuleRuntimeSnapshot,
        evaluated_rules: &[Rule],
    ) -> AppResult<u64>;
    fn reset_runtime_hit_metadata(&self, collection_id: Uuid) -> AppResult<()>;
}

impl RuntimeRuleRepository for RuleRepositoryAdapter {
    fn runtime_snapshot(&self, channel: &ChannelId) -> AppResult<RuleRuntimeSnapshot> {
        RuleRepositoryAdapter::runtime_snapshot(self, channel.as_str())
    }

    fn commit_runtime_snapshot(
        &self,
        snapshot: &RuleRuntimeSnapshot,
        evaluated_rules: &[Rule],
    ) -> AppResult<u64> {
        RuleRepositoryAdapter::commit_runtime_snapshot(self, snapshot, evaluated_rules)
    }

    fn reset_runtime_hit_metadata(&self, collection_id: Uuid) -> AppResult<()> {
        RuleRepositoryAdapter::reset_runtime_hit_metadata(self, collection_id)
    }
}

/// One adapter instance is shared by both listeners for the lifetime of the app.
#[derive(Debug)]
pub struct RuntimePipelineProductHooks {
    pub body_codec: Arc<dyn BodyCodec>,
    pub request_classifier: Arc<dyn RequestClassifier>,
    pub channel_labels: BTreeMap<String, String>,
}

/// 根据动态 Listener 和消息阶段选择 Body 编解码器。
///
/// Resolver 返回 `None` 时使用产品的通用 Body codec；动态 Listener 可结合当前消息
/// Content-Type 选择更精确的编码器。
pub trait RuntimeBodyCodecResolver: std::fmt::Debug + Send + Sync {
    fn resolve(
        &self,
        context: &ConnectionContext,
        stage: DomainMessageStage,
        message: &Message,
    ) -> ProxyResult<Option<Arc<dyn BodyCodec>>>;
}

/// One adapter instance is shared by both listeners for the lifetime of the app.
#[derive(Debug)]
pub struct RuntimePipelineAdapter {
    body_codec: Arc<dyn BodyCodec>,
    body_codec_resolver: Option<Arc<dyn RuntimeBodyCodecResolver>>,
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
    connections: HashMap<RuntimeEpoch, HashMap<Uuid, ConnectionRuntime>>,
    live_sessions: HashMap<Uuid, LiveSession>,
    channels: HashMap<RuntimeEpoch, BTreeMap<ChannelId, ChannelRuntimeMetrics>>,
    stopped_epochs: HashSet<RuntimeEpoch>,
}

impl PipelineState {
    fn connection(&self, context: &ConnectionContext) -> Option<&ConnectionRuntime> {
        self.connections
            .get(&RuntimeEpoch::from_uuid(context.runtime_epoch))?
            .get(&context.connection_id)
    }

    fn connection_mut(&mut self, context: &ConnectionContext) -> Option<&mut ConnectionRuntime> {
        self.connections
            .get_mut(&RuntimeEpoch::from_uuid(context.runtime_epoch))?
            .get_mut(&context.connection_id)
    }

    fn channel_metrics_mut(
        &mut self,
        context: &ConnectionContext,
    ) -> Option<&mut ChannelRuntimeMetrics> {
        self.channels
            .get_mut(&RuntimeEpoch::from_uuid(context.runtime_epoch))?
            .get_mut(&context.channel)
    }

    fn remove_connection(&mut self, context: &ConnectionContext) {
        let epoch = RuntimeEpoch::from_uuid(context.runtime_epoch);
        let remove_epoch = self.connections.get_mut(&epoch).is_some_and(|connections| {
            connections.remove(&context.connection_id);
            connections.is_empty()
        });
        if remove_epoch {
            self.connections.remove(&epoch);
        }
    }
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

mod breakpoints;
mod completion;
mod core;
mod mapping;
mod metrics;
mod ports;
mod session;

#[cfg(test)]
use mapping::view_to_domain_rule;
use mapping::{
    app_channel, app_to_proxy, apply_breakpoint_decision, breakpoint_detail, domain_channel,
    fingerprint, result_text, result_tone, tls_summary,
};

#[cfg(test)]
#[path = "pipeline/tests.rs"]
mod tests;
