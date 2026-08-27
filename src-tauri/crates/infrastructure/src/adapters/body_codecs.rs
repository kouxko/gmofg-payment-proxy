//! HTTP Body 编码解析器。
//!
//! 新 Listener 使用 `Auto`，最终展示投影以每条消息的 Content-Type charset 为准。
//! 旧 Workspace 的 Raw、UTF-8 和 Shift-JIS 仍可加载，避免配置升级破坏运行现场。

use std::{collections::HashMap, sync::Arc};

use encoding_rs::SHIFT_JIS;
use intercept_proxy_domain::{BodyCodecKind, MessageStage, ProxyListener};
use intercept_proxy_product_api::{BodyCodec, ProductError};
use intercept_proxy_runtime::{
    ConnectionContext, ErrorCode, Message, ProxyError, Result as ProxyResult,
};

use super::pipeline::RuntimeBodyCodecResolver;

mod content_type;
pub use content_type::HeaderBodyCodecResolver;
pub(crate) use content_type::decode_message_body;
pub(crate) use content_type::resolve_message_codec;

#[derive(Debug)]
pub struct WorkspaceBodyCodecResolver {
    snapshots: parking_lot::RwLock<HashMap<(uuid::Uuid, String), InstalledBodyCodecSnapshot>>,
}

impl WorkspaceBodyCodecResolver {
    #[must_use]
    pub fn new() -> Self {
        Self {
            snapshots: parking_lot::RwLock::new(HashMap::new()),
        }
    }

    pub(crate) fn install_listener(
        &self,
        runtime_epoch: uuid::Uuid,
        run_token: uuid::Uuid,
        listener: &ProxyListener,
    ) {
        let snapshot = listener
            .http()
            .map_or(ListenerBodyCodecSnapshot::ProductDefault, |http| {
                ListenerBodyCodecSnapshot::Http {
                    request: http.request_body_codec,
                    response: http.response_body_codec,
                }
            });
        self.snapshots.write().insert(
            (runtime_epoch, listener.id.to_string()),
            InstalledBodyCodecSnapshot {
                run_token,
                codec: snapshot,
            },
        );
    }

    pub(crate) fn remove_listener(
        &self,
        runtime_epoch: uuid::Uuid,
        listener_id: intercept_proxy_domain::ListenerId,
        run_token: uuid::Uuid,
    ) {
        let key = (runtime_epoch, listener_id.to_string());
        let mut snapshots = self.snapshots.write();
        if snapshots
            .get(&key)
            .is_some_and(|snapshot| snapshot.run_token == run_token)
        {
            snapshots.remove(&key);
        }
    }

    pub(crate) fn remove_epoch(&self, runtime_epoch: uuid::Uuid) {
        self.snapshots
            .write()
            .retain(|(epoch, _), _| *epoch != runtime_epoch);
    }
}

impl Default for WorkspaceBodyCodecResolver {
    fn default() -> Self {
        Self::new()
    }
}

#[derive(Clone, Copy, Debug)]
enum ListenerBodyCodecSnapshot {
    ProductDefault,
    Http {
        request: BodyCodecKind,
        response: BodyCodecKind,
    },
}

#[derive(Clone, Copy, Debug)]
struct InstalledBodyCodecSnapshot {
    run_token: uuid::Uuid,
    codec: ListenerBodyCodecSnapshot,
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
        let snapshot = self
            .snapshots
            .read()
            .get(&(context.runtime_epoch, context.channel.as_str().to_owned()))
            .map(|snapshot| snapshot.codec)
            .ok_or_else(|| {
                ProxyError::new(
                    ErrorCode::Internal,
                    format!(
                        "body codec runtime snapshot missing for epoch {} channel {}",
                        context.runtime_epoch,
                        context.channel.as_str()
                    ),
                )
            })?;
        let ListenerBodyCodecSnapshot::Http { request, response } = snapshot else {
            return Ok(None);
        };
        let selected = match stage {
            MessageStage::Request => request,
            MessageStage::Response => response,
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

    use intercept_proxy_application::{
        BreakpointBodyCodecResolver, MessageContentKind, MessageContentViewModel,
    };
    use intercept_proxy_domain::{
        BodyCodecKind, HttpListenerSettings, ListenerDataPlane, ListenerId, ProxyListener,
        ProxyWorkspace,
    };
    use intercept_proxy_runtime::ChannelId;
    use uuid::Uuid;

    use super::*;
    #[test]
    fn started_epoch_resolves_frozen_shift_jis_per_listener_and_stage() {
        let listener_id = ListenerId::new();
        let workspace = ProxyWorkspace {
            listeners: vec![ProxyListener {
                id: listener_id,
                name: "Shift-JIS API".into(),
                enabled: false,
                bind_address: "127.0.0.1".into(),
                port: 18_443,
                data_plane: ListenerDataPlane::Http(HttpListenerSettings {
                    request_body_codec: BodyCodecKind::Raw,
                    response_body_codec: BodyCodecKind::ShiftJis,
                    ..HttpListenerSettings::default()
                }),
                ..ProxyListener::default()
            }],
            ..ProxyWorkspace::default()
        };
        workspace.validate().unwrap();
        let resolver = WorkspaceBodyCodecResolver::new();
        let runtime_epoch = Uuid::new_v4();
        resolver.install_listener(runtime_epoch, Uuid::new_v4(), &workspace.listeners[0]);
        let context = ConnectionContext {
            runtime_epoch,
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

        resolver.remove_epoch(runtime_epoch);
        let stale = resolver
            .resolve(&context, MessageStage::Response, &message)
            .expect_err("stopped epoch cannot resolve a codec");
        assert_eq!(stale.code, ErrorCode::Internal.as_str());
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
            protocol: None,
            protocol_failure: None,
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
            protocol: None,
            protocol_failure: None,
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
