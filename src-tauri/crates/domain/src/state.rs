//! 核心状态机。
//!
//! 代理、通道、报文和会话只能沿显式允许的路径迁移。例如运行中的代理必须先进入
//! `Stopping` 才能停止。非法跳转会立即返回领域错误，避免各适配器各自猜测状态。

use crate::{DomainError, ErrorCode};
use serde::{Deserialize, Serialize};
use specta::Type;

fn invalid_transition(from: impl std::fmt::Debug, to: impl std::fmt::Debug) -> DomainError {
    DomainError::new(
        ErrorCode::InvalidStateTransition,
        format!("非法状态转换：{from:?} -> {to:?}"),
    )
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize, Type)]
/// 整个代理进程的生命周期状态。
///
/// `Faulted` 不是“仍在正常运行”，而是记录启动或运行失败；它可以再次尝试启动，
/// 也可以进入停止流程释放残余资源。
pub enum ProxyState {
    Stopped,
    Starting,
    Running,
    Stopping,
    Faulted,
}

impl ProxyState {
    /// 验证并执行一次状态迁移。
    ///
    /// 返回 `Ok(next)` 只代表这条状态边合法，真正的端口绑定、任务取消等副作用由
    /// runtime 层完成。这里不能偷偷执行 I/O。
    pub fn transition(self, next: Self) -> Result<Self, DomainError> {
        let legal = matches!(
            (self, next),
            (Self::Stopped | Self::Faulted, Self::Starting)
                | (
                    Self::Starting,
                    Self::Running | Self::Faulted | Self::Stopping
                )
                | (Self::Running, Self::Stopping | Self::Faulted)
                | (Self::Stopping, Self::Stopped | Self::Faulted)
                | (Self::Faulted, Self::Stopping | Self::Stopped)
        );
        legal
            .then_some(next)
            .ok_or_else(|| invalid_transition(self, next))
    }
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize, Type)]
/// 单个监听通道（例如交易或 DLL）的生命周期。
pub enum ChannelState {
    Disabled,
    Stopped,
    Starting,
    Listening,
    Stopping,
    Faulted,
}

impl ChannelState {
    pub fn transition(self, next: Self) -> Result<Self, DomainError> {
        let legal = matches!(
            (self, next),
            (Self::Disabled, Self::Stopped)
                | (Self::Stopped, Self::Disabled | Self::Starting)
                | (
                    Self::Faulted,
                    Self::Starting | Self::Stopping | Self::Stopped | Self::Disabled
                )
                | (
                    Self::Starting,
                    Self::Listening | Self::Faulted | Self::Stopping
                )
                | (Self::Listening, Self::Stopping | Self::Faulted)
                | (Self::Stopping, Self::Stopped | Self::Faulted)
        );
        legal
            .then_some(next)
            .ok_or_else(|| invalid_transition(self, next))
    }
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize, Type)]
/// 单条请求或响应在规则管线中的处理状态。
///
/// `TerminalActionApplied` 表示规则已经用 Mock/断开等动作终结了正常转发路径。
pub enum MessageState {
    Captured,
    RulesEvaluated,
    Ready,
    Forwarded,
    TerminalActionApplied,
    Cancelled,
}

impl MessageState {
    pub fn transition(self, next: Self) -> Result<Self, DomainError> {
        let legal = matches!(
            (self, next),
            (Self::Captured, Self::RulesEvaluated | Self::Cancelled)
                | (
                    Self::RulesEvaluated,
                    Self::Ready | Self::TerminalActionApplied | Self::Cancelled
                )
                | (
                    Self::Ready,
                    Self::Forwarded | Self::TerminalActionApplied | Self::Cancelled
                )
        );
        legal
            .then_some(next)
            .ok_or_else(|| invalid_transition(self, next))
    }

    #[must_use]
    pub const fn is_terminal(self) -> bool {
        matches!(
            self,
            Self::Forwarded | Self::TerminalActionApplied | Self::Cancelled
        )
    }
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize, Type)]
/// 会话从接收请求到结束的阶段。
///
/// 客户端断开、代理停止和失败属于“可从任意活动阶段发生”的中断终态。
pub enum SessionState {
    ReceivingRequest,
    ProcessingRequest,
    WaitingForUpstream,
    ProcessingResponse,
    Completed,
    ClientDisconnected,
    ProxyStopped,
    Failed,
}

impl SessionState {
    pub fn transition(self, next: Self) -> Result<Self, DomainError> {
        // 中断不是正常业务步骤，但网络连接可能在任意活动阶段消失，因此统一允许所有
        // 非终态进入三种中断终态。已经终结的会话仍禁止再次迁移。
        let interrupted = matches!(
            next,
            Self::ClientDisconnected | Self::ProxyStopped | Self::Failed
        ) && !self.is_terminal();
        let legal = interrupted
            || matches!(
                (self, next),
                (Self::ReceivingRequest, Self::ProcessingRequest)
                    | (
                        Self::ProcessingRequest,
                        Self::WaitingForUpstream | Self::Completed
                    )
                    | (Self::WaitingForUpstream, Self::ProcessingResponse)
                    | (Self::ProcessingResponse, Self::Completed)
            );
        legal
            .then_some(next)
            .ok_or_else(|| invalid_transition(self, next))
    }

    #[must_use]
    pub const fn is_terminal(self) -> bool {
        matches!(
            self,
            Self::Completed | Self::ClientDisconnected | Self::ProxyStopped | Self::Failed
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // STATE-001, STATE-002, TEST-STATE
    #[test]
    fn runtime_states_allow_only_lifecycle_transitions() {
        assert_eq!(
            ProxyState::Stopped
                .transition(ProxyState::Starting)
                .unwrap(),
            ProxyState::Starting
        );
        assert!(ProxyState::Stopped.transition(ProxyState::Running).is_err());
        assert!(
            ChannelState::Listening
                .transition(ChannelState::Disabled)
                .is_err()
        );
    }

    // DATA-006, TEST-DOMAIN
    #[test]
    fn session_interruptions_are_terminal_from_any_active_phase() {
        assert_eq!(
            SessionState::WaitingForUpstream
                .transition(SessionState::ClientDisconnected)
                .unwrap(),
            SessionState::ClientDisconnected
        );
        assert!(
            SessionState::Completed
                .transition(SessionState::Failed)
                .is_err()
        );
    }
}
