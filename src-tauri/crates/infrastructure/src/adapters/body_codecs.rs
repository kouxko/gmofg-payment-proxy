//! 动态 Workspace Body Codec 解析器。
//!
//! Listener 只保存非敏感 Codec 引用；实际 Raw、UTF-8、Shift-JIS 行为由 Rust 在每个
//! 请求/响应阶段根据持久化 Workspace 快照决定。前端不解码、不猜测，也不重建长度。

use std::sync::Arc;

use encoding_rs::SHIFT_JIS;
use intercept_proxy_domain::{BodyCodecKind, MessageStage, ProxyListener};
use intercept_proxy_product_api::{BodyCodec, ProductError};
use intercept_proxy_runtime::{ConnectionContext, ErrorCode, ProxyError, Result as ProxyResult};

use crate::SqliteStore;

use super::{common::decode_workspace_record, pipeline::RuntimeBodyCodecResolver};

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
                .any(|listener| listener.id().to_string() == context.channel.as_str())
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
        let Some(ProxyListener::Reverse(listener)) = workspace
            .listeners
            .iter()
            .find(|listener| listener.id().to_string() == context.channel.as_str())
        else {
            // 旧 supervisor 通道或 Forward CONNECT 当前没有方向独立 Codec 引用。
            return Ok(None);
        };
        let policy_id = match stage {
            MessageStage::Request => listener.request_codec_policy,
            MessageStage::Response => listener.response_codec_policy,
            MessageStage::TlsHandshake => None,
        };
        let Some(policy_id) = policy_id else {
            return Ok(None);
        };
        let policy = workspace
            .body_codec_policies
            .iter()
            .find(|policy| policy.id == policy_id)
            .ok_or_else(|| {
                ProxyError::new(
                    ErrorCode::ConfigInvalid,
                    format!("Workspace Body Codec reference {policy_id} does not exist"),
                )
            })?;
        let codec: Arc<dyn BodyCodec> = match policy.codec {
            BodyCodecKind::Raw => Arc::new(RawBodyCodec),
            BodyCodecKind::Utf8 => Arc::new(Utf8BodyCodec),
            BodyCodecKind::ShiftJis => Arc::new(ShiftJisBodyCodec),
        };
        Ok(Some(codec))
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
                "Body 包含无效的 Shift-JIS 字节序列",
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
    use std::{net::SocketAddr, time::SystemTime};

    use chrono::Utc;
    use intercept_proxy_domain::{
        BodyCodecPolicy, BodyDirection, CodecPolicyId, DownstreamTlsSettings, ListenerId,
        ProxyListener, ProxyWorkspace, ReverseProxyListener, UpstreamTlsSettings,
    };
    use intercept_proxy_runtime::ChannelId;
    use uuid::Uuid;

    use super::*;
    use crate::WorkspaceRecord;

    #[test]
    fn selected_workspace_resolves_shift_jis_per_listener_and_stage() {
        let listener_id = ListenerId::new();
        let codec_id = CodecPolicyId::new();
        let workspace = ProxyWorkspace {
            listeners: vec![ProxyListener::Reverse(ReverseProxyListener {
                id: listener_id,
                name: "Shift-JIS API".into(),
                enabled: false,
                bind_address: "127.0.0.1".into(),
                port: 18_443,
                upstream_url: "https://example.test".into(),
                downstream_tls: DownstreamTlsSettings::default(),
                upstream_tls: UpstreamTlsSettings::default(),
                request_codec_policy: None,
                response_codec_policy: Some(codec_id),
            })],
            body_codec_policies: vec![BodyCodecPolicy {
                id: codec_id,
                name: "SJIS response".into(),
                listener_ids: vec![listener_id],
                direction: BodyDirection::Response,
                codec: BodyCodecKind::ShiftJis,
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

        assert!(
            resolver
                .resolve(&context, MessageStage::Request)
                .unwrap()
                .is_none()
        );
        let codec = resolver
            .resolve(&context, MessageStage::Response)
            .unwrap()
            .expect("response Shift-JIS codec");
        let encoded = codec.encode("結果D48").unwrap();
        assert_eq!(codec.decode(&encoded).unwrap(), "結果D48");
        assert!(codec.encode("😀").is_err());
    }
}
