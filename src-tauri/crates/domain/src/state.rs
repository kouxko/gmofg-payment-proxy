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
pub enum ProxyState {
    Stopped,
    Starting,
    Running,
    Stopping,
    Faulted,
}

impl ProxyState {
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
pub enum MessageState {
    Captured,
    RulesEvaluated,
    BreakpointPending,
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
                    Self::BreakpointPending
                        | Self::Ready
                        | Self::TerminalActionApplied
                        | Self::Cancelled
                )
                | (
                    Self::BreakpointPending,
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
