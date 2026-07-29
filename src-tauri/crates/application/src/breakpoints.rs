use std::collections::{HashMap, VecDeque};

use parking_lot::Mutex;
use tokio::sync::oneshot;

use crate::{
    AppError, AppResult, BreakpointActionOptionViewModel, BreakpointDecision,
    BreakpointDecisionKind, BreakpointDetailViewModel, BreakpointId, BreakpointState,
    BreakpointSummaryViewModel, DisabledReason, Revision, RuntimeEpoch, UiTone,
};

#[derive(Debug)]
pub struct BreakpointTicket {
    pub detail: BreakpointDetailViewModel,
    pub outcome: oneshot::Receiver<BreakpointOutcome>,
}

#[derive(Debug, Clone, PartialEq)]
pub enum BreakpointOutcome {
    Decision(BreakpointDecision),
    ClientDisconnected,
    ProxyStopped,
}

#[derive(Debug, Default)]
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
        let item = state
            .pending
            .get_mut(&id)
            .expect("presence was checked while holding the coordinator lock");
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
        if sender.send(BreakpointOutcome::Decision(decision)).is_err() {
            let mut summary = item.detail.summary.clone();
            summary.state = BreakpointState::ClientDisconnected;
            summary.revision = summary.revision.saturating_add(1);
            apply_summary_display(&mut summary);
            state.pending.remove(&id);
            remember_terminal(&mut state, id, BreakpointState::ClientDisconnected);
            return Err(AppError::new(
                "BREAKPOINT_CLIENT_DISCONNECTED",
                "Payment App 已断开，不能继续处理该断点。",
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
            "Payment App 已断开，不能继续处理该断点。",
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
    let stage_compatible = match detail.summary.stage {
        crate::MessageStage::Request => matches!(
            decision.kind,
            crate::BreakpointDecisionKind::ForwardOriginal
                | crate::BreakpointDecisionKind::ForwardModified
                | crate::BreakpointDecisionKind::MockResponse
                | crate::BreakpointDecisionKind::Delay
                | crate::BreakpointDecisionKind::DisconnectBeforeUpstream
        ),
        crate::MessageStage::Response => !matches!(
            decision.kind,
            crate::BreakpointDecisionKind::MockResponse
                | crate::BreakpointDecisionKind::DisconnectBeforeUpstream
        ),
        crate::MessageStage::TlsHandshake | crate::MessageStage::Terminal => false,
    };
    if !stage_compatible {
        return Err(AppError::new(
            "CONFIG_INVALID",
            "断点决策与当前报文阶段不兼容。",
        ));
    }
    if matches!(
        decision.kind,
        crate::BreakpointDecisionKind::ForwardModified
            | crate::BreakpointDecisionKind::MockResponse
    ) && decision.message.is_none()
    {
        return Err(AppError::new(
            "CONFIG_INVALID",
            "该断点操作必须提供有效报文。",
        ));
    }
    if decision.kind == crate::BreakpointDecisionKind::Delay && decision.delay_ms.is_none() {
        return Err(AppError::new(
            "CONFIG_INVALID",
            "延迟操作必须提供延迟毫秒数。",
        ));
    }
    if let Some(delay_ms) = decision.delay_ms
        && delay_ms > 600_000
    {
        return Err(AppError::new(
            "CONFIG_INVALID",
            "断点延迟不能超过 600000 毫秒。",
        ));
    }
    if decision.kind == crate::BreakpointDecisionKind::CustomHttpStatus
        && !decision
            .http_status
            .is_some_and(|status| (100..=599).contains(&status))
    {
        return Err(AppError::new(
            "CONFIG_INVALID",
            "自定义 HTTP 状态码必须位于 100 到 599 之间。",
        ));
    }
    if decision.kind == crate::BreakpointDecisionKind::WrongContentLength
        && decision.content_length_delta.is_none_or(|delta| delta == 0)
    {
        return Err(AppError::new(
            "CONFIG_INVALID",
            "错误 Content-Length 必须提供非零差值。",
        ));
    }
    if decision.kind == crate::BreakpointDecisionKind::Truncate && decision.truncate_at.is_none() {
        return Err(AppError::new(
            "CONFIG_INVALID",
            "截断操作必须提供截断位置。",
        ));
    }
    if let Some(at) = decision.truncate_at {
        let body_len = decision
            .message
            .as_ref()
            .unwrap_or(&detail.effective)
            .body_bytes
            .len();
        if body_len == 0 || at >= body_len {
            return Err(AppError::new(
                "CONFIG_INVALID",
                "截断位置必须位于 0 到 Body 长度减 1 之间。",
            ));
        }
    }
    Ok(())
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
