//! 断点领域模型。
//!
//! “断点”表示代理已截获报文，并暂停网络流程等待操作者决定放行、修改、Mock 或终止。
//! 这里仅描述断点状态和合法变化；异步等待、事件通知等工作属于 application/runtime 层。

use crate::{
    BreakpointId, DomainError, ErrorCode, MessageId, MessageStage, Revision, RuntimeEpoch,
    SessionId,
};
use serde::{Deserialize, Serialize};
use specta::Type;

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize, Type)]
/// 断点生命周期。
///
/// 除 `Pending` 外均为终态；终态断点不能再次处理。
pub enum BreakpointState {
    Pending,
    Resolved,
    ClientDisconnected,
    ProxyStopped,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize, Type)]
/// 操作者对暂停报文作出的决定。
///
/// 不是每个决定都适用于请求和响应，例如响应阶段不能再创建 Mock 响应，具体兼容性
/// 由 `Breakpoint::resolve` 在领域层统一校验。
pub enum BreakpointDecision {
    ForwardOriginal,
    ForwardModified,
    MockResponse,
    Delay { milliseconds: u64 },
    DisconnectBeforeUpstream,
    CustomHttpStatus { status: u16 },
    InvalidJson,
    IncorrectContentLength { delta: i64 },
    Truncate { bytes: u64 },
    DropResponse,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize, Type)]
/// 一个可被并发安全处理的领域断点。
///
/// `revision` 用于拒绝界面基于旧数据提交的决定，`decision` 只在成功解决后出现。
pub struct Breakpoint {
    pub id: BreakpointId,
    pub runtime_epoch: RuntimeEpoch,
    pub session_id: SessionId,
    pub message_id: MessageId,
    pub stage: MessageStage,
    revision: Revision,
    state: BreakpointState,
    decision: Option<BreakpointDecision>,
}

impl Breakpoint {
    #[must_use]
    pub fn new(
        runtime_epoch: RuntimeEpoch,
        session_id: SessionId,
        message_id: MessageId,
        stage: MessageStage,
    ) -> Self {
        Self {
            id: BreakpointId::new(),
            runtime_epoch,
            session_id,
            message_id,
            stage,
            revision: Revision::INITIAL,
            state: BreakpointState::Pending,
            decision: None,
        }
    }

    pub fn resolve(
        &mut self,
        expected_revision: Revision,
        decision: BreakpointDecision,
    ) -> Result<Revision, DomainError> {
        // 先检查版本，再检查状态和动作兼容性。顺序很重要：旧界面提交必须得到版本冲突，
        // 不能覆盖或模糊掉另一个操作者已经完成的处理结果。
        self.revision.verify(expected_revision)?;
        if self.state != BreakpointState::Pending {
            return Err(self.resolved_error());
        }
        self.validate_decision(&decision)?;
        self.state = BreakpointState::Resolved;
        self.decision = Some(decision);
        self.revision = self.revision.next();
        Ok(self.revision)
    }

    #[must_use]
    pub const fn revision(&self) -> Revision {
        self.revision
    }

    #[must_use]
    pub const fn state(&self) -> BreakpointState {
        self.state
    }

    #[must_use]
    pub const fn decision(&self) -> Option<&BreakpointDecision> {
        self.decision.as_ref()
    }

    pub fn client_disconnected(&mut self) -> Result<Revision, DomainError> {
        self.cancel(BreakpointState::ClientDisconnected)
    }

    pub fn proxy_stopped(&mut self) -> Result<Revision, DomainError> {
        self.cancel(BreakpointState::ProxyStopped)
    }

    fn cancel(&mut self, state: BreakpointState) -> Result<Revision, DomainError> {
        if self.state != BreakpointState::Pending {
            return Err(self.resolved_error());
        }
        self.state = state;
        self.revision = self.revision.next();
        Ok(self.revision)
    }

    fn resolved_error(&self) -> DomainError {
        let code = match self.state {
            BreakpointState::ClientDisconnected => ErrorCode::BreakpointClientDisconnected,
            BreakpointState::ProxyStopped => ErrorCode::BreakpointProxyStopped,
            BreakpointState::Pending | BreakpointState::Resolved => {
                ErrorCode::BreakpointAlreadyResolved
            }
        };
        DomainError::new(code, "断点已结束，不能再次处理")
            .with_field_error("breakpoint", code.as_str())
    }

    fn validate_decision(&self, decision: &BreakpointDecision) -> Result<(), DomainError> {
        let compatible = match self.stage {
            MessageStage::Request => matches!(
                decision,
                BreakpointDecision::ForwardOriginal
                    | BreakpointDecision::ForwardModified
                    | BreakpointDecision::MockResponse
                    | BreakpointDecision::Delay { .. }
                    | BreakpointDecision::DisconnectBeforeUpstream
            ),
            MessageStage::Response => !matches!(
                decision,
                BreakpointDecision::MockResponse | BreakpointDecision::DisconnectBeforeUpstream
            ),
            MessageStage::TlsHandshake => false,
        };
        compatible.then_some(()).ok_or_else(|| {
            DomainError::new(ErrorCode::RuleInvalid, "断点决策与报文阶段不兼容")
                .with_field_error("decision", "当前阶段不支持此操作")
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pending(stage: MessageStage) -> Breakpoint {
        Breakpoint::new(
            RuntimeEpoch::new(),
            SessionId::new(),
            MessageId::new(),
            stage,
        )
    }

    // DATA-004, DATA-005, TEST-BREAKPOINT
    #[test]
    fn breakpoint_resolves_exactly_once_with_revision_check() {
        let mut breakpoint = pending(MessageStage::Request);
        assert_eq!(
            breakpoint
                .resolve(Revision::INITIAL, BreakpointDecision::ForwardOriginal)
                .unwrap(),
            Revision::new(2)
        );
        assert_eq!(
            breakpoint
                .resolve(Revision::new(2), BreakpointDecision::ForwardOriginal)
                .unwrap_err()
                .code,
            ErrorCode::BreakpointAlreadyResolved
        );
    }

    // BREAKPOINT-013, BREAKPOINT-015
    #[test]
    fn cancellation_causes_have_stable_errors() {
        let mut disconnected = pending(MessageStage::Response);
        disconnected.client_disconnected().unwrap();
        assert_eq!(
            disconnected
                .resolve(Revision::new(2), BreakpointDecision::DropResponse)
                .unwrap_err()
                .code,
            ErrorCode::BreakpointClientDisconnected
        );
        let mut stopped = pending(MessageStage::Response);
        stopped.proxy_stopped().unwrap();
        assert_eq!(
            stopped
                .resolve(Revision::new(2), BreakpointDecision::DropResponse)
                .unwrap_err()
                .code,
            ErrorCode::BreakpointProxyStopped
        );
    }
}
