//! 待处理断点的应用层协调器。
//!
//! runtime 注册断点并异步等待，展示适配器按 ID 查询和提交决定。协调器保证一个断点
//! 只能终结一次，并保留有限终态记录，让重复操作得到明确错误。

use std::collections::{HashMap, VecDeque};

use parking_lot::Mutex;
use tokio::sync::oneshot;

use crate::{
    AppError, AppResult, BreakpointActionOptionViewModel, BreakpointDecision,
    BreakpointDecisionKind, BreakpointDetailViewModel, BreakpointId, BreakpointState,
    BreakpointSummaryViewModel, DisabledReason, Revision, RuntimeEpoch, UiTone,
    breakpoint_validation::validate_breakpoint_decision_structure,
};

#[derive(Debug)]
/// runtime 注册断点后得到的“展示数据 + 等待句柄”。
pub struct BreakpointTicket {
    pub detail: BreakpointDetailViewModel,
    pub outcome: oneshot::Receiver<BreakpointOutcome>,
}

#[derive(Debug, Clone, PartialEq)]
/// 网络任务等待到的最终结果。
pub enum BreakpointOutcome {
    Decision(Box<BreakpointDecision>),
    ClientDisconnected,
    ProxyStopped,
}

#[derive(Debug, Default)]
/// 连接 runtime 等待方与用户操作方的并发协调器。
pub struct BreakpointCoordinator {
    state: Mutex<CoordinatorState>,
}

#[derive(Debug, Default)]
struct CoordinatorState {
    pending: HashMap<BreakpointId, PendingBreakpoint>,
    terminal: HashMap<BreakpointId, BreakpointState>,
    terminal_order: VecDeque<BreakpointId>,
}

#[derive(Debug)]
struct PendingBreakpoint {
    detail: BreakpointDetailViewModel,
    sender: Option<oneshot::Sender<BreakpointOutcome>>,
}

impl BreakpointCoordinator {
    const MAX_TERMINAL_TOMBSTONES: usize = 4_096;

    pub fn register(&self, mut detail: BreakpointDetailViewModel) -> AppResult<BreakpointTicket> {
        // 重复检查和插入必须在同一次加锁期间完成，防止两个网络任务为同一 ID 创建等待者。
        if detail.summary.state != BreakpointState::Pending {
            return Err(AppError::new(
                "BREAKPOINT_INVALID",
                "只有待处理断点可以加入断点队列。",
            ));
        }
        let id = detail.summary.breakpoint_id;
        let mut state = self.state.lock();
        if state.pending.contains_key(&id) || state.terminal.contains_key(&id) {
            return Err(
                AppError::new("BREAKPOINT_ALREADY_EXISTS", "断点标识已存在。")
                    .entity(id.to_string()),
            );
        }
        apply_breakpoint_display(&mut detail);
        let (sender, receiver) = oneshot::channel();
        state.pending.insert(
            id,
            PendingBreakpoint {
                detail: detail.clone(),
                sender: Some(sender),
            },
        );
        Ok(BreakpointTicket {
            detail,
            outcome: receiver,
        })
    }

    pub fn query(&self, epoch: Option<RuntimeEpoch>) -> Vec<BreakpointSummaryViewModel> {
        let disconnected = {
            let state = self.state.lock();
            state
                .pending
                .iter()
                .filter_map(|(id, item)| {
                    item.sender
                        .as_ref()
                        .is_some_and(tokio::sync::oneshot::Sender::is_closed)
                        .then_some(*id)
                })
                .collect::<Vec<_>>()
        };
        for id in disconnected {
            let _ = self.terminate(id, BreakpointState::ClientDisconnected);
        }
        let state = self.state.lock();
        let mut items = state
            .pending
            .values()
            .filter(|item| {
                epoch.is_none_or(|runtime_epoch| item.detail.summary.runtime_epoch == runtime_epoch)
            })
            .map(|item| item.detail.summary.clone())
            .collect::<Vec<_>>();
        items.sort_by(|left, right| {
            left.waiting_since
                .cmp(&right.waiting_since)
                .then_with(|| left.breakpoint_id.cmp(&right.breakpoint_id))
        });
        items
    }

    pub fn get(
        &self,
        id: BreakpointId,
        epoch: RuntimeEpoch,
    ) -> AppResult<BreakpointDetailViewModel> {
        let state = self.state.lock();
        let item = state
            .pending
            .get(&id)
            .ok_or_else(|| not_found_or_terminal(&state, id))?;
        if item.detail.summary.runtime_epoch != epoch {
            return Err(
                AppError::new("BREAKPOINT_NOT_FOUND", "断点不存在于当前运行周期。")
                    .entity(id.to_string())
                    .epoch(epoch),
            );
        }
        Ok(item.detail.clone())
    }

    pub fn resolve(
        &self,
        epoch: RuntimeEpoch,
        decision: BreakpointDecision,
    ) -> AppResult<BreakpointSummaryViewModel> {
        let id = decision.breakpoint_id;
        let mut state = self.state.lock();
        if !state.pending.contains_key(&id) {
            return Err(not_found_or_terminal(&state, id));
        }
        let item = state.pending.get_mut(&id).ok_or_else(|| {
            AppError::new(
                "BREAKPOINT_STATE_INCONSISTENT",
                "断点协调器状态不一致，请重新加载断点列表后重试。",
            )
            .entity(id.to_string())
            .retryable("如果问题持续出现，请停止并重新启动对应代理入口。")
        })?;
        if item.detail.summary.runtime_epoch != epoch {
            return Err(
                AppError::new("BREAKPOINT_NOT_FOUND", "断点不存在于当前运行周期。")
                    .entity(id.to_string())
                    .epoch(epoch),
            );
        }
        if item.detail.summary.revision != decision.expected_revision {
            return Err(AppError::new(
                "REVISION_CONFLICT",
                "断点已被其他操作更新，请重新加载后再处理。",
            )
            .entity(id.to_string())
            .epoch(epoch));
        }

        validate_decision(&item.detail, &decision)?;
        let sender = item.sender.take().ok_or_else(|| {
            AppError::new("BREAKPOINT_ALREADY_RESOLVED", "断点已经处理。").entity(id.to_string())
        })?;
        if sender
            .send(BreakpointOutcome::Decision(Box::new(decision)))
            .is_err()
        {
            let mut summary = item.detail.summary.clone();
            summary.state = BreakpointState::ClientDisconnected;
            summary.revision = summary.revision.saturating_add(1);
            apply_summary_display(&mut summary);
            state.pending.remove(&id);
            remember_terminal(&mut state, id, BreakpointState::ClientDisconnected);
            return Err(AppError::new(
                "BREAKPOINT_CLIENT_DISCONNECTED",
                "客户端已断开，不能继续处理该断点。",
            )
            .entity(id.to_string())
            .epoch(epoch));
        }

        let mut summary = item.detail.summary.clone();
        summary.state = BreakpointState::Resolved;
        summary.revision = summary.revision.saturating_add(1);
        apply_summary_display(&mut summary);
        state.pending.remove(&id);
        remember_terminal(&mut state, id, BreakpointState::Resolved);
        Ok(summary)
    }

    pub fn client_disconnected(&self, id: BreakpointId) -> AppResult<BreakpointSummaryViewModel> {
        self.terminate(id, BreakpointState::ClientDisconnected)
    }

    pub fn proxy_stopped(&self, epoch: RuntimeEpoch) -> Vec<BreakpointSummaryViewModel> {
        let ids = {
            let state = self.state.lock();
            state
                .pending
                .values()
                .filter(|item| item.detail.summary.runtime_epoch == epoch)
                .map(|item| item.detail.summary.breakpoint_id)
                .collect::<Vec<_>>()
        };
        ids.into_iter()
            .filter_map(|id| self.terminate(id, BreakpointState::ProxyStopped).ok())
            .collect()
    }

    fn terminate(
        &self,
        id: BreakpointId,
        terminal: BreakpointState,
    ) -> AppResult<BreakpointSummaryViewModel> {
        let mut state = self.state.lock();
        let item = state
            .pending
            .remove(&id)
            .ok_or_else(|| not_found_or_terminal(&state, id))?;
        if let Some(sender) = item.sender {
            let outcome = match terminal {
                BreakpointState::ClientDisconnected => BreakpointOutcome::ClientDisconnected,
                BreakpointState::ProxyStopped => BreakpointOutcome::ProxyStopped,
                BreakpointState::Resolved | BreakpointState::Pending => {
                    return Err(AppError::new(
                        "BREAKPOINT_INVALID",
                        "断点取消只能转换为客户端断开或 Proxy 停止。",
                    )
                    .entity(id.to_string()));
                }
            };
            let _ = sender.send(outcome);
        }
        let mut summary = item.detail.summary;
        summary.state = terminal;
        summary.revision = summary.revision.saturating_add(1);
        apply_summary_display(&mut summary);
        remember_terminal(&mut state, id, terminal);
        Ok(summary)
    }
}

fn remember_terminal(state: &mut CoordinatorState, id: BreakpointId, terminal: BreakpointState) {
    state.terminal.insert(id, terminal);
    state.terminal_order.push_back(id);
    while state.terminal_order.len() > BreakpointCoordinator::MAX_TERMINAL_TOMBSTONES {
        if let Some(expired) = state.terminal_order.pop_front() {
            state.terminal.remove(&expired);
        }
    }
}

fn not_found_or_terminal(state: &CoordinatorState, id: BreakpointId) -> AppError {
    match state.terminal.get(&id) {
        Some(BreakpointState::Resolved) => {
            AppError::new("BREAKPOINT_ALREADY_RESOLVED", "断点已经处理。").entity(id.to_string())
        }
        Some(BreakpointState::ClientDisconnected) => AppError::new(
            "BREAKPOINT_CLIENT_DISCONNECTED",
            "客户端已断开，不能继续处理该断点。",
        )
        .entity(id.to_string()),
        Some(BreakpointState::ProxyStopped) => AppError::new(
            "BREAKPOINT_PROXY_STOPPED",
            "Proxy 已停止，不能继续处理该断点。",
        )
        .entity(id.to_string()),
        _ => AppError::new("BREAKPOINT_NOT_FOUND", "断点不存在或已被移除。").entity(id.to_string()),
    }
}

fn validate_decision(
    detail: &BreakpointDetailViewModel,
    decision: &BreakpointDecision,
) -> AppResult<()> {
    let validation = validate_breakpoint_decision_structure(detail, decision);
    if validation.valid {
        Ok(())
    } else {
        Err(AppError::field(
            "CONFIG_INVALID",
            "断点决策校验失败。",
            validation.field_errors,
        ))
    }
}

fn apply_breakpoint_display(detail: &mut BreakpointDetailViewModel) {
    apply_summary_display(&mut detail.summary);
    detail.can_resolve = detail.summary.state == BreakpointState::Pending;
    detail.resolve_disabled_reason = (!detail.can_resolve).then(|| DisabledReason {
        code: "BREAKPOINT_NOT_PENDING".into(),
        message: "该断点已不处于待处理状态。".into(),
    });
    let disabled_reason = detail.resolve_disabled_reason.clone();
    detail.available_actions = [
        (BreakpointDecisionKind::ForwardModified, "应用修改后放行"),
        (BreakpointDecisionKind::ForwardOriginal, "原样放行"),
        (BreakpointDecisionKind::MockResponse, "直接 Mock 响应"),
        (BreakpointDecisionKind::Delay, "延迟后继续"),
        (
            BreakpointDecisionKind::DisconnectBeforeUpstream,
            "不连接上游并断开",
        ),
        (BreakpointDecisionKind::CustomHttpStatus, "自定义 HTTP 状态"),
        (BreakpointDecisionKind::InvalidJson, "构造非法 JSON"),
        (
            BreakpointDecisionKind::WrongContentLength,
            "错误 Content-Length",
        ),
        (BreakpointDecisionKind::Truncate, "截断报文"),
        (BreakpointDecisionKind::DropResponse, "丢弃响应"),
    ]
    .into_iter()
    .filter(|(kind, _)| {
        crate::breakpoint_validation::stage_supports_decision(detail.summary.stage, *kind)
    })
    .map(|(kind, label)| BreakpointActionOptionViewModel {
        kind,
        label: label.into(),
        enabled: detail.can_resolve,
        disabled_reason: disabled_reason.clone(),
        default_delay_ms: (kind == BreakpointDecisionKind::Delay).then_some(1_000),
        default_http_status: (kind == BreakpointDecisionKind::CustomHttpStatus).then_some(503),
        default_content_length_delta: (kind == BreakpointDecisionKind::WrongContentLength)
            .then_some(1),
        default_truncate_at: (kind == BreakpointDecisionKind::Truncate)
            .then_some(detail.effective.body_bytes.len().saturating_sub(1).min(1)),
    })
    .collect();
}

fn apply_summary_display(summary: &mut BreakpointSummaryViewModel) {
    let (text, tone) = match summary.state {
        BreakpointState::Pending => ("等待处理", UiTone::Warning),
        BreakpointState::Resolved => ("已处理", UiTone::Positive),
        BreakpointState::ClientDisconnected => ("客户端已断开", UiTone::Danger),
        BreakpointState::ProxyStopped => ("Proxy 已停止", UiTone::Danger),
    };
    summary.state_text = text.into();
    summary.ui_tone = tone;
}

#[allow(dead_code)]
fn _revision_is_contract(_: Revision) {}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use chrono::Utc;
    use uuid::Uuid;

    use super::*;
    use crate::{ChannelId, MessageContentViewModel, MessageStage};

    fn message(body: &[u8]) -> MessageContentViewModel {
        MessageContentViewModel {
            http_status: None,
            start_line_bytes: Vec::new(),
            raw_headers: Vec::new(),
            headers: BTreeMap::new(),
            body_text: None,
            body_bytes: body.to_vec(),
            json: None,
            content_length: body.len(),
            media_type: None,
            charset: None,
            content_kind: crate::MessageContentKind::Unknown,
            codec_id: None,
            decode_error: None,
            query_string: None,
            protocol: None,
            protocol_failure: None,
        }
    }

    fn detail(stage: MessageStage, effective_body: &[u8]) -> BreakpointDetailViewModel {
        let original = message(b"original");
        BreakpointDetailViewModel {
            summary: BreakpointSummaryViewModel {
                breakpoint_id: Uuid::new_v4(),
                session_id: Uuid::new_v4(),
                runtime_epoch: Uuid::new_v4(),
                stage,
                title: String::new(),
                terminal_ip: "127.0.0.1".into(),
                channel: ChannelId::new("alpha").unwrap(),
                channel_text: "Alpha".into(),
                method: "POST".into(),
                target: "/pay".into(),
                waiting_since: Utc::now(),
                certificate_fingerprint_suffix: "1234".into(),
                state: BreakpointState::Pending,
                state_text: String::new(),
                ui_tone: UiTone::Warning,
                revision: 1,
            },
            original,
            effective: message(effective_body),
            can_resolve: true,
            resolve_disabled_reason: None,
            available_actions: Vec::new(),
        }
    }

    fn decision(
        detail: &BreakpointDetailViewModel,
        kind: BreakpointDecisionKind,
    ) -> BreakpointDecision {
        BreakpointDecision {
            breakpoint_id: detail.summary.breakpoint_id,
            expected_revision: detail.summary.revision,
            kind,
            message: None,
            delay_ms: None,
            http_status: None,
            content_length_delta: None,
            truncate_at: None,
        }
    }

    #[test]
    fn coordinator_rejects_zero_and_over_limit_delay() {
        let coordinator = BreakpointCoordinator::default();
        let detail = detail(MessageStage::Request, b"body");
        let epoch = detail.summary.runtime_epoch;
        let _ticket = coordinator.register(detail.clone()).expect("register");

        for delay_ms in [0, 600_001] {
            let error = coordinator
                .resolve(
                    epoch,
                    BreakpointDecision {
                        delay_ms: Some(delay_ms),
                        ..decision(&detail, BreakpointDecisionKind::Delay)
                    },
                )
                .expect_err("invalid delay must be rejected");

            assert_eq!(error.view_model.code, "CONFIG_INVALID");
            assert!(error.view_model.field_errors.contains_key("delay_ms"));
        }
    }

    #[test]
    fn coordinator_validates_truncate_against_effective_message_length() {
        let coordinator = BreakpointCoordinator::default();
        let detail = detail(MessageStage::Response, b"abc");
        let epoch = detail.summary.runtime_epoch;
        let _ticket = coordinator.register(detail.clone()).expect("register");

        let error = coordinator
            .resolve(
                epoch,
                BreakpointDecision {
                    message: Some(message(b"much longer replacement")),
                    truncate_at: Some(3),
                    ..decision(&detail, BreakpointDecisionKind::Truncate)
                },
            )
            .expect_err("truncate at effective length must be rejected");

        assert_eq!(error.view_model.code, "CONFIG_INVALID");
        assert!(error.view_model.field_errors.contains_key("truncate_at"));
    }
}
