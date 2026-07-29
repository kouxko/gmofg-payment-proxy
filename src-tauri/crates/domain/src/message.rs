use crate::{DomainError, ErrorCode, MessageId, MessageState, Revision, RuntimeEpoch, SessionId};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use specta::Type;

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize, Type)]
pub enum ChannelKind {
    Transaction,
    Dll,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize, Type)]
pub enum MessageStage {
    Request,
    Response,
    TlsHandshake,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize, Type)]
pub enum ContentKind {
    Json,
    Text,
    Binary,
    Unknown,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize, Type)]
pub struct Header {
    pub name: String,
    pub value: String,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize, Type)]
pub struct MessageData {
    pub start_line: String,
    pub headers: Vec<Header>,
    pub body: Vec<u8>,
    pub content_kind: ContentKind,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize, Type)]
pub enum MessageChangeSource {
    Rule,
    Manual,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize, Type)]
pub struct MessageVersion {
    pub revision: Revision,
    pub source: MessageChangeSource,
    pub parent_revision: Revision,
    pub data: MessageData,
    pub created_at: DateTime<Utc>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize, Type)]
pub struct Message {
    pub id: MessageId,
    pub session_id: SessionId,
    pub runtime_epoch: RuntimeEpoch,
    pub stage: MessageStage,
    state: MessageState,
    original: MessageData,
    versions: Vec<MessageVersion>,
    revision: Revision,
}

impl Message {
    #[must_use]
    pub fn new(
        session_id: SessionId,
        runtime_epoch: RuntimeEpoch,
        stage: MessageStage,
        original: MessageData,
    ) -> Self {
        Self {
            id: MessageId::new(),
            session_id,
            runtime_epoch,
            stage,
            state: MessageState::Captured,
            original,
            versions: Vec::new(),
            revision: Revision::INITIAL,
        }
    }

    #[must_use]
    pub fn effective(&self) -> &MessageData {
        self.versions
            .last()
            .map_or(&self.original, |version| &version.data)
    }

    #[must_use]
    pub const fn state(&self) -> MessageState {
        self.state
    }

    #[must_use]
    pub const fn original(&self) -> &MessageData {
        &self.original
    }

    #[must_use]
    pub fn versions(&self) -> &[MessageVersion] {
        &self.versions
    }

    #[must_use]
    pub const fn revision(&self) -> Revision {
        self.revision
    }

    pub fn add_version(
        &mut self,
        expected_revision: Revision,
        source: MessageChangeSource,
        data: MessageData,
        created_at: DateTime<Utc>,
    ) -> Result<Revision, DomainError> {
        self.revision.verify(expected_revision)?;
        if source == MessageChangeSource::Manual && self.state != MessageState::BreakpointPending {
            return Err(DomainError::new(
                ErrorCode::InvalidStateTransition,
                "只有待处理断点允许人工修改报文",
            ));
        }
        let parent_revision = self.revision;
        self.revision = self.revision.next();
        self.versions.push(MessageVersion {
            revision: self.revision,
            source,
            parent_revision,
            data,
            created_at,
        });
        Ok(self.revision)
    }

    pub fn transition(&mut self, next: MessageState) -> Result<(), DomainError> {
        self.state = self.state.transition(next)?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn data(body: &[u8]) -> MessageData {
        MessageData {
            start_line: "POST / HTTP/1.1".into(),
            headers: Vec::new(),
            body: body.to_vec(),
            content_kind: ContentKind::Json,
        }
    }

    // MESSAGE-002, MESSAGE-008, MESSAGE-009, NFR-003
    #[test]
    fn original_bytes_remain_immutable_and_versions_form_lineage() {
        let mut message = Message::new(
            SessionId::new(),
            RuntimeEpoch::new(),
            MessageStage::Request,
            data(&[0x81, 0x40]),
        );
        message.transition(MessageState::RulesEvaluated).unwrap();
        message.transition(MessageState::BreakpointPending).unwrap();
        let next = message
            .add_version(
                Revision::INITIAL,
                MessageChangeSource::Manual,
                data(b"changed"),
                Utc::now(),
            )
            .unwrap();
        assert_eq!(message.original().body, [0x81, 0x40]);
        assert_eq!(message.effective().body, b"changed");
        assert_eq!(message.versions()[0].parent_revision, Revision::INITIAL);
        assert_eq!(next, Revision::new(2));
    }
}
