//! HTTP Body 编码解析器。
//!
//! 新 Listener 使用 `Auto`，最终展示投影以每条消息的 Content-Type charset 为准。
//! 旧 Workspace 的 Raw、UTF-8 和 Shift-JIS 仍可加载，避免配置升级破坏运行现场。

use std::sync::Arc;

use encoding_rs::SHIFT_JIS;
use intercept_proxy_domain::MessageStage;
use intercept_proxy_product_api::{BodyCodec, ProductError};
use intercept_proxy_runtime::{
    ConnectionContext, ErrorCode, Message, ProxyError, Result as ProxyResult,
};

use crate::SqliteStore;

use super::{common::decode_workspace_record, pipeline::RuntimeBodyCodecResolver};

mod content_type;
pub use content_type::HeaderBodyCodecResolver;
pub(crate) use content_type::decode_message_body;
pub(crate) use content_type::resolve_message_codec;

#[derive(Debug)]
pub struct WorkspaceBodyCodecResolver {
    store: Arc<SqliteStore>,
}

impl WorkspaceBodyCodecResolver {
    #[must_use]
    pub fn new(store: Arc<SqliteStore>) -> Self {
        Self { store }
    }
}

impl RuntimeBodyCodecResolver for WorkspaceBodyCodecResolver {
    fn resolve(
        &self,
        context: &ConnectionContext,
        stage: MessageStage,
        message: &Message,
    ) -> ProxyResult<Option<Arc<dyn BodyCodec>>> {
        if matches!(stage, MessageStage::TlsHandshake) {
            return Ok(None);
        }
        let snapshot = self.store.load_workspaces().map_err(|error| {
            ProxyError::new(
                ErrorCode::Internal,
                format!("cannot load Workspace codec snapshot: {error}"),
            )
        })?;
        // 运行中的 Listener 以自身 ID 绑定启动时所属 Workspace。切换“当前 Workspace”
        // 只是 UI 编辑上下文，不得让已经建立的连接突然改用另一个 Workspace 的 Codec。
        // 正常写入路径会禁止修改含运行中 Listener 的 Workspace，因此按全局唯一 Listener
        // ID 定位既保持运行快照语义，也避免把 UI 选择状态带进数据面。
        let mut matches = Vec::new();
        for record in snapshot.records {
            let workspace = decode_workspace_record(record).map_err(|message| {
                ProxyError::new(
                    ErrorCode::Internal,
                    format!("Workspace persistence corrupt: {message}"),
                )
            })?;
            if workspace
                .listeners
                .iter()
                .any(|listener| listener.id.to_string() == context.channel.as_str())
            {
                matches.push(workspace);
            }
        }
        if matches.len() > 1 {
            return Err(ProxyError::new(
                ErrorCode::Internal,
                format!(
                    "Workspace persistence corrupt: listener {} belongs to multiple Workspaces",
                    context.channel.as_str()
                ),
            ));
        }
        let workspace = matches.pop();
        let Some(workspace) = workspace else {
            return Ok(None);
        };
        let Some(listener) = workspace
            .listeners
            .iter()
            .find(|listener| listener.id.to_string() == context.channel.as_str())
        else {
            // 旧 supervisor 通道当前没有 Listener 级 Codec 引用。
            return Ok(None);
        };
        let selected = match stage {
            MessageStage::Request => listener.request_body_codec,
            MessageStage::Response => listener.response_body_codec,
            MessageStage::TlsHandshake => return Ok(None),
        };
        Ok(Some(resolve_message_codec(selected, message)))
    }
}

#[derive(Debug)]
struct RawBodyCodec;

impl BodyCodec for RawBodyCodec {
    fn id(&self) -> &'static str {
        "raw"
    }

    fn name(&self) -> &'static str {
        "Raw"
    }

    fn decode(&self, _bytes: &[u8]) -> Result<String, ProductError> {
        Err(ProductError::new(
            "RAW_BODY_HAS_NO_TEXT",
            "Raw Body 不能作为文本或 JSON 解码",
        ))
    }

    fn encode(&self, _text: &str) -> Result<Vec<u8>, ProductError> {
        Err(ProductError::new(
            "RAW_BODY_HAS_NO_TEXT",
            "Raw Body 不能从文本重建",
        ))
    }
}

#[derive(Debug)]
struct Utf8BodyCodec;

impl BodyCodec for Utf8BodyCodec {
    fn id(&self) -> &'static str {
        "utf-8"
    }

    fn name(&self) -> &'static str {
        "UTF-8"
    }

    fn decode(&self, bytes: &[u8]) -> Result<String, ProductError> {
        String::from_utf8(bytes.to_vec())
            .map_err(|error| ProductError::new("UTF8_DECODE_FAILED", error.to_string()))
    }

    fn encode(&self, text: &str) -> Result<Vec<u8>, ProductError> {
        Ok(text.as_bytes().to_vec())
    }
}

#[derive(Debug)]
struct ShiftJisBodyCodec;

impl BodyCodec for ShiftJisBodyCodec {
    fn id(&self) -> &'static str {
        "shift-jis"
    }

    fn name(&self) -> &'static str {
        "Shift-JIS"
    }

    fn decode(&self, bytes: &[u8]) -> Result<String, ProductError> {
        let (decoded, had_errors) = SHIFT_JIS.decode_without_bom_handling(bytes);
        if had_errors {
            return Err(ProductError::new(
                "SHIFT_JIS_DECODE_FAILED",
                "invalid Shift-JIS byte sequence in Body",
            ));
        }
        Ok(decoded.into_owned())
    }

    fn encode(&self, text: &str) -> Result<Vec<u8>, ProductError> {
        let (encoded, _encoding, had_errors) = SHIFT_JIS.encode(text);
        if had_errors {
            return Err(ProductError::new(
                "SHIFT_JIS_ENCODE_FAILED",
                "文本包含 Shift-JIS 无法无损表示的字符",
            ));
        }
        Ok(encoded.into_owned())
    }
}

#[cfg(test)]
mod tests {
    use std::{collections::BTreeMap, net::SocketAddr, time::SystemTime};

    use chrono::Utc;
    use intercept_proxy_application::{
        BreakpointBodyCodecResolver, MessageContentKind, MessageContentViewModel,
    };
    use intercept_proxy_domain::{BodyCodecKind, ListenerId, ProxyListener, ProxyWorkspace};
    use intercept_proxy_runtime::ChannelId;
    use uuid::Uuid;

    use super::*;
    use crate::WorkspaceRecord;

    #[test]
    fn selected_workspace_resolves_shift_jis_per_listener_and_stage() {
        let listener_id = ListenerId::new();
        let workspace = ProxyWorkspace {
            listeners: vec![ProxyListener {
                id: listener_id,
                name: "Shift-JIS API".into(),
                enabled: false,
                bind_address: "127.0.0.1".into(),
                port: 18_443,
                request_body_codec: BodyCodecKind::Raw,
                response_body_codec: BodyCodecKind::ShiftJis,
                ..ProxyListener::default()
            }],
            ..ProxyWorkspace::default()
        };
        workspace.validate().unwrap();
        let store = Arc::new(SqliteStore::in_memory().unwrap());
        store
            .insert_workspace(&WorkspaceRecord {
                id: workspace.id.as_uuid(),
                revision: workspace.revision.get(),
                value: serde_json::to_value(&workspace).unwrap(),
                updated_at: Utc::now(),
            })
            .unwrap();
        let resolver = WorkspaceBodyCodecResolver::new(store);
        let context = ConnectionContext {
            runtime_epoch: Uuid::new_v4(),
            connection_id: Uuid::new_v4(),
            channel: ChannelId::new(listener_id.to_string()).unwrap(),
            peer_addr: "127.0.0.1:12345".parse::<SocketAddr>().unwrap(),
            accepted_at: SystemTime::now(),
            tls_peer: None,
        };
        let message = message_with_content_type("text/plain", b"body");

        let request_codec = resolver
            .resolve(&context, MessageStage::Request, &message)
            .unwrap()
            .expect("request Raw codec");
        assert_eq!(request_codec.id(), "raw");
        let codec = resolver
            .resolve(&context, MessageStage::Response, &message)
            .unwrap()
            .expect("response Shift-JIS codec");
        let encoded = codec.encode("結果D48").unwrap();
        assert_eq!(codec.decode(&encoded).unwrap(), "結果D48");
        assert!(codec.encode("😀").is_err());
    }

    #[test]
    fn declared_charset_aliases_are_canonicalized_without_lossy_decoding() {
        let (encoded, _, had_errors) = SHIFT_JIS.encode("結果D48");
        assert!(!had_errors);
        for alias in [
            "Shift_JIS",
            "shift-jis",
            "SJIS",
            "Windows-31J",
            "MS932",
            "CP932",
        ] {
            let message =
                message_with_content_type(&format!("text/plain; charset={alias}"), &encoded);
            let codec = resolve_message_codec(BodyCodecKind::Auto, &message);
            assert_eq!(codec.id(), "auto:shift-jis");
            assert_eq!(codec.decode(&encoded).unwrap(), "結果D48");
        }
        for alias in ["UTF-8", "utf8", "\"utf-8\""] {
            let message = message_with_content_type(
                &format!("text/plain; charset={alias}"),
                "成功".as_bytes(),
            );
            let codec = resolve_message_codec(BodyCodecKind::Auto, &message);
            assert_eq!(codec.id(), "auto:utf-8");
            assert_eq!(codec.decode("成功".as_bytes()).unwrap(), "成功");
        }
    }

    #[test]
    fn forced_codec_overrides_conflicting_content_type_charset() {
        let utf8_message =
            message_with_content_type("text/plain; charset=utf-8", "結果D48".as_bytes());
        let shift_jis = resolve_message_codec(BodyCodecKind::ShiftJis, &utf8_message);
        assert_eq!(shift_jis.id(), "shift-jis");
        let encoded = shift_jis.encode("結果D48").unwrap();
        assert_eq!(shift_jis.decode(&encoded).unwrap(), "結果D48");

        let shift_jis_message =
            message_with_content_type("text/plain; charset=shift_jis", "成功".as_bytes());
        let utf8 = resolve_message_codec(BodyCodecKind::Utf8, &shift_jis_message);
        assert_eq!(utf8.id(), "utf-8");
        assert_eq!(utf8.decode("成功".as_bytes()).unwrap(), "成功");
    }

    #[test]
    fn breakpoint_resolver_uses_edited_content_type_instead_of_stale_codec_id() {
        let resolver = HeaderBodyCodecResolver;
        let message = MessageContentViewModel {
            http_status: None,
            start_line_bytes: b"POST / HTTP/1.1".to_vec(),
            raw_headers: Vec::new(),
            headers: BTreeMap::from([(
                "Content-Type".into(),
                vec!["application/json; charset=windows-31j".into()],
            )]),
            body_text: Some(r#"{"result":"成功"}"#.into()),
            body_bytes: Vec::new(),
            json: None,
            content_length: 0,
            media_type: Some("application/json".into()),
            charset: Some("utf-8".into()),
            content_kind: MessageContentKind::Json,
            codec_id: Some("auto:utf-8".into()),
            decode_error: None,
            query_string: None,
        };

        let codec = resolver.resolve(&message);
        let encoded = codec.encode(message.body_text.as_deref().unwrap()).unwrap();

        assert_eq!(codec.id(), "auto:shift-jis");
        assert_eq!(SHIFT_JIS.decode(&encoded).0, message.body_text.unwrap());
    }

    #[test]
    fn breakpoint_resolver_preserves_forced_codec_when_header_is_edited() {
        let resolver = HeaderBodyCodecResolver;
        let message = MessageContentViewModel {
            http_status: None,
            start_line_bytes: b"POST / HTTP/1.1".to_vec(),
            raw_headers: Vec::new(),
            headers: BTreeMap::from([(
                "Content-Type".into(),
                vec!["text/plain; charset=utf-8".into()],
            )]),
            body_text: Some("結果D48".into()),
            body_bytes: Vec::new(),
            json: None,
            content_length: 0,
            media_type: Some("text/plain".into()),
            charset: Some("utf-8".into()),
            content_kind: MessageContentKind::Text,
            codec_id: Some("shift-jis".into()),
            decode_error: None,
            query_string: None,
        };

        let codec = resolver.resolve(&message);

        assert_eq!(codec.id(), "shift-jis");
        let encoded = codec.encode("結果D48").unwrap();
        assert_eq!(codec.decode(&encoded).unwrap(), "結果D48");
    }

    fn message_with_content_type(content_type: &str, body: &[u8]) -> Message {
        use bytes::Bytes;
        use intercept_proxy_runtime::RawHeader;

        Message {
            start_line: "POST / HTTP/1.1".into(),
            headers: vec![RawHeader::new(
                Bytes::from_static(b"Content-Type"),
                Bytes::copy_from_slice(content_type.as_bytes()),
            )],
            body: Bytes::copy_from_slice(body),
            body_modified: false,
        }
    }
}
