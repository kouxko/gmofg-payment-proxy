//! 一次端到端代理会话的领域生命周期。
//!
//! `Session` 关联客户端连接、请求、可选响应及最终结果。它只维护合法状态和关联关系，
//! 不负责保存、分页、筛选或导出 Payload。

use crate::{
    ConnectionId, DomainError, MessageId, Revision, RuntimeEpoch, SessionId, SessionState,
};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use specta::Type;

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize, Type)]
/// 第 N 次命中和审计使用的终端身份。
///
/// 同一 IP 但证书指纹不同的客户端视为不同终端。
pub struct TerminalIdentity {
    pub source_ip: String,
    pub certificate_sha256: String,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize, Type)]
/// 一次客户端请求到最终结束的会话聚合根。
pub struct Session {
    pub id: SessionId,
    pub connection_id: ConnectionId,
    pub runtime_epoch: RuntimeEpoch,
    pub terminal: TerminalIdentity,
    state: SessionState,
    pub request_message_id: MessageId,
    response_message_id: Option<MessageId>,
    revision: Revision,
    pub created_at: DateTime<Utc>,
    completed_at: Option<DateTime<Utc>>,
}

impl Session {
    #[must_use]
    pub fn new(
        connection_id: ConnectionId,
        runtime_epoch: RuntimeEpoch,
        terminal: TerminalIdentity,
        request_message_id: MessageId,
        created_at: DateTime<Utc>,
    ) -> Self {
        Self {
            id: SessionId::new(),
            connection_id,
            runtime_epoch,
            terminal,
            state: SessionState::ReceivingRequest,
            request_message_id,
            response_message_id: None,
            revision: Revision::INITIAL,
            created_at,
            completed_at: None,
        }
    }

    pub fn transition(&mut self, next: SessionState, at: DateTime<Utc>) -> Result<(), DomainError> {
        // 只有状态机确认合法后才更新 revision 和完成时间，错误路径不会留下半迁移状态。
        self.state = self.state.transition(next)?;
        self.revision = self.revision.next();
        if next.is_terminal() {
            self.completed_at = Some(at);
        }
        Ok(())
    }

    #[must_use]
    pub const fn state(&self) -> SessionState {
        self.state
    }

    #[must_use]
    pub const fn response_message_id(&self) -> Option<MessageId> {
        self.response_message_id
    }

    #[must_use]
    pub const fn revision(&self) -> Revision {
        self.revision
    }

    #[must_use]
    pub const fn completed_at(&self) -> Option<DateTime<Utc>> {
        self.completed_at
    }

    pub fn attach_response(&mut self, message_id: MessageId) -> Result<(), DomainError> {
        if self.response_message_id.is_some() {
            return Err(crate::DomainError::new(
                crate::ErrorCode::InvalidStateTransition,
                "会话已关联响应报文",
            ));
        }
        self.response_message_id = Some(message_id);
        self.revision = self.revision.next();
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // DATA-002, DATA-003
    #[test]
    fn request_and_response_belong_to_one_session_and_connection() {
        let request = MessageId::new();
        let response = MessageId::new();
        let mut session = Session::new(
            ConnectionId::new(),
            RuntimeEpoch::new(),
            TerminalIdentity {
                source_ip: "10.0.0.2".into(),
                certificate_sha256: "abc".into(),
            },
            request,
            Utc::now(),
        );
        session.attach_response(response).unwrap();
        assert_eq!(session.request_message_id, request);
        assert_eq!(session.response_message_id(), Some(response));
    }
}
