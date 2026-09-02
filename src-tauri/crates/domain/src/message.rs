//! 网络报文的领域表示。
//!
//! 报文同时保留原始字节、可读文本、JSON 和 HTTP 元数据。原始字节用于未修改时完整
//! 透传，文本/JSON 用于规则和人工编辑；二者不可随意混用，否则 Shift-JIS 下会丢数据。

use crate::{DomainError, ErrorCode, MessageId, MessageState, Revision, RuntimeEpoch, SessionId};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use specta::Type;

#[derive(Clone, Debug, Deserialize, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize, Type)]
#[serde(try_from = "String", into = "String")]
/// 产品通道的受校验稳定 ID。
pub struct ChannelId(String);

impl ChannelId {
    pub fn new(value: impl Into<String>) -> Result<Self, DomainError> {
        let value = value.into();
        let valid = !value.is_empty()
            && value.len() <= 64
            && value
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.'))
            && value
                .as_bytes()
                .first()
                .is_some_and(u8::is_ascii_alphanumeric)
            && value
                .as_bytes()
                .last()
                .is_some_and(u8::is_ascii_alphanumeric);
        if valid {
            Ok(Self(value))
        } else {
            Err(DomainError::new(
                ErrorCode::ConfigInvalid,
                "通道 ID 必须为 1 到 64 个 ASCII 字母、数字、连字符、下划线或点，且首尾为字母或数字",
            ))
        }
    }

    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl TryFrom<String> for ChannelId {
    type Error = DomainError;

    fn try_from(value: String) -> Result<Self, Self::Error> {
        Self::new(value)
    }
}

impl From<ChannelId> for String {
    fn from(value: ChannelId) -> Self {
        value.0
    }
}

impl std::str::FromStr for ChannelId {
    type Err = DomainError;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        Self::new(value)
    }
}

impl std::fmt::Display for ChannelId {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(self.as_str())
    }
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize, Type)]
/// 报文在代理管线中的方向/阶段。
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
/// 一份报文数据快照；`body` 始终是权威原始字节。
pub struct MessageData {
    pub start_line: String,
    pub headers: Vec<Header>,
    pub body: Vec<u8>,
    pub content_kind: ContentKind,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize, Type)]
pub enum MessageChangeSource {
    Rule,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize, Type)]
/// 对原始报文的不可变修改版本，用父版本形成审计链。
pub struct MessageVersion {
    pub revision: Revision,
    pub source: MessageChangeSource,
    pub parent_revision: Revision,
    pub data: MessageData,
    pub created_at: DateTime<Utc>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize, Type)]
/// 报文聚合根，保存原件、修改历史、状态和 revision。
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

    #[test]
    fn channel_id_rejects_invalid_deserialized_values() {
        for invalid in ["", "-alpha", "alpha-", "with space", "under__"] {
            assert!(
                serde_json::from_value::<ChannelId>(serde_json::json!(invalid)).is_err(),
                "{invalid:?} must be rejected"
            );
        }
        assert_eq!(
            serde_json::from_value::<ChannelId>(serde_json::json!("alpha-3"))
                .unwrap()
                .as_str(),
            "alpha-3"
        );
        for valid in ["alpha_2.v1", "A-channel", "x"] {
            assert_eq!(
                serde_json::from_value::<ChannelId>(serde_json::json!(valid))
                    .unwrap()
                    .as_str(),
                valid
            );
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
        let next = message
            .add_version(
                Revision::INITIAL,
                MessageChangeSource::Rule,
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
