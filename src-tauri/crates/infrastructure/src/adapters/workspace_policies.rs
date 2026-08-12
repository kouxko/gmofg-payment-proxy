//! 当前 Workspace 的通用元数据提取器与响应断言执行器。
//!
//! 该适配器只读取所选 Workspace 的一致性快照。它不会改变网络报文：断言失败只会写入
//! 会话结果并供测试/界面判断，真实上游响应仍由代理按原始字节返回客户端。

use std::{collections::BTreeMap, sync::Arc};

use intercept_proxy_application::ResponseAssertionResultViewModel;
use intercept_proxy_domain::{
    JsonPath, MessageStage, MetadataExtractorSource, ProxyWorkspace, ResponseAssertionKind,
};
use intercept_proxy_product_api::BodyCodec;
use intercept_proxy_runtime::{ConnectionContext, ErrorCode, Message, ProxyError, Result};
use ring::digest::{SHA256, digest};
use serde_json::Value;

use crate::SqliteStore;

#[cfg(test)]
use super::common::encode_workspace_record;
use super::{
    common::decode_workspace_record,
    pipeline::{RuntimeWorkspacePolicyEvaluation, RuntimeWorkspacePolicyResolver},
};

#[derive(Debug)]
pub struct WorkspaceRuntimePolicyResolver {
    store: Arc<SqliteStore>,
}

impl WorkspaceRuntimePolicyResolver {
    #[must_use]
    pub fn new(store: Arc<SqliteStore>) -> Self {
        Self { store }
    }

    fn workspace_for_channel(&self, channel: &str) -> Result<Option<ProxyWorkspace>> {
        let snapshot = self.store.load_workspaces().map_err(|error| {
            ProxyError::new(
                ErrorCode::Internal,
                format!("cannot load Workspace policy snapshot: {error}"),
            )
        })?;
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
                .any(|listener| listener.id.to_string() == channel)
            {
                matches.push(workspace);
            }
        }
        if matches.len() > 1 {
            return Err(ProxyError::new(
                ErrorCode::Internal,
                format!(
                    "Workspace persistence corrupt: listener {channel} belongs to multiple Workspaces"
                ),
            ));
        }
        Ok(matches.pop())
    }
}

impl RuntimeWorkspacePolicyResolver for WorkspaceRuntimePolicyResolver {
    fn evaluate(
        &self,
        context: &ConnectionContext,
        stage: MessageStage,
        message: &Message,
        body_codec: &dyn BodyCodec,
    ) -> Result<RuntimeWorkspacePolicyEvaluation> {
        // UI 可在 Listener 运行时切换当前 Workspace；运行时策略必须继续按连接的
        // Listener ID 解析，不能跟随 UI 选择漂移。
        let Some(workspace) = self.workspace_for_channel(context.channel.as_str())? else {
            return Ok(RuntimeWorkspacePolicyEvaluation::default());
        };
        let listener_id = context.channel.as_str();
        let decoded = body_codec.decode(&message.body).ok();
        let json = decoded
            .as_deref()
            .and_then(|text| serde_json::from_str::<Value>(text).ok());

        let mut metadata = BTreeMap::new();
        for extractor in workspace.metadata_extractors.iter().filter(|extractor| {
            extractor.listener_ids.is_empty()
                || extractor
                    .listener_ids
                    .iter()
                    .any(|id| id.to_string() == listener_id)
        }) {
            let value = match &extractor.source {
                MetadataExtractorSource::Header { name } => {
                    header_values(message, name).into_iter().next()
                }
                MetadataExtractorSource::JsonPath { path } => json
                    .as_ref()
                    .and_then(|value| JsonPath::parse(path).ok()?.resolve(value))
                    .map(display_json_value),
                MetadataExtractorSource::BodyText => decoded.clone(),
                MetadataExtractorSource::FixedValue { value } => Some(value.clone()),
            };
            if let Some(value) = value {
                metadata.insert(extractor.name.clone(), value);
            }
        }

        let assertions = if stage == MessageStage::Response {
            workspace
                .response_assertions
                .iter()
                .filter(|assertion| {
                    assertion.enabled
                        && (assertion.listener_ids.is_empty()
                            || assertion
                                .listener_ids
                                .iter()
                                .any(|id| id.to_string() == listener_id))
                })
                .map(|assertion| {
                    let (passed, message_text) = evaluate_assertion(
                        &assertion.assertion,
                        message,
                        decoded.as_deref(),
                        json.as_ref(),
                    );
                    ResponseAssertionResultViewModel {
                        assertion_id: assertion.id,
                        name: assertion.name.clone(),
                        passed,
                        message: message_text,
                    }
                })
                .collect()
        } else {
            Vec::new()
        };

        Ok(RuntimeWorkspacePolicyEvaluation {
            metadata,
            assertions,
        })
    }
}

fn evaluate_assertion(
    assertion: &ResponseAssertionKind,
    message: &Message,
    decoded: Option<&str>,
    json: Option<&Value>,
) -> (bool, String) {
    match assertion {
        ResponseAssertionKind::HttpStatusEquals { expected } => compare(
            message.http_status() == Some(*expected),
            format!("期望 HTTP {expected}，实际 {:?}", message.http_status()),
        ),
        ResponseAssertionKind::HeaderEquals { name, expected } => {
            let actual = header_values(message, name);
            compare(
                actual.iter().any(|value| value == expected),
                format!("Header {name} 期望 {expected:?}，实际 {actual:?}"),
            )
        }
        ResponseAssertionKind::JsonPathEquals { path, expected } => {
            let actual = json
                .and_then(|value| JsonPath::parse(path).ok()?.resolve(value))
                .cloned();
            compare(
                actual.as_ref() == Some(expected),
                format!("JSONPath {path} 期望 {expected}，实际 {actual:?}"),
            )
        }
        ResponseAssertionKind::BodyTextContains { expected } => compare(
            decoded.is_some_and(|text| text.contains(expected)),
            decoded.map_or_else(
                || "Body 无法按 Listener Codec 解码".into(),
                |text| {
                    format!(
                        "Body 必须包含 {expected:?}，实际长度 {} 字符",
                        text.chars().count()
                    )
                },
            ),
        ),
        ResponseAssertionKind::BodyLengthEquals { expected } => compare(
            u64::try_from(message.body.len()).ok() == Some(*expected),
            format!(
                "Body 期望 {expected} 字节，实际 {} 字节",
                message.body.len()
            ),
        ),
        ResponseAssertionKind::BodySha256Equals { expected_hex } => {
            let actual = hex_lower(digest(&SHA256, &message.body).as_ref());
            compare(
                actual.eq_ignore_ascii_case(expected_hex),
                format!("Body SHA-256 期望 {expected_hex}，实际 {actual}"),
            )
        }
    }
}

fn compare(passed: bool, detail: String) -> (bool, String) {
    (passed, if passed { "通过".into() } else { detail })
}

fn header_values(message: &Message, name: &str) -> Vec<String> {
    message
        .headers
        .iter()
        .filter(|header| header.name.as_ref().eq_ignore_ascii_case(name.as_bytes()))
        .map(|header| String::from_utf8_lossy(&header.value).into_owned())
        .collect()
}

fn display_json_value(value: &Value) -> String {
    match value {
        Value::String(value) => value.clone(),
        other => other.to_string(),
    }
}

fn hex_lower(bytes: &[u8]) -> String {
    use std::fmt::Write;
    bytes.iter().fold(
        String::with_capacity(bytes.len() * 2),
        |mut output, byte| {
            let _ = write!(output, "{byte:02x}");
            output
        },
    )
}

#[cfg(test)]
mod tests {
    use std::{net::SocketAddr, time::SystemTime};

    use bytes::Bytes;
    use chrono::Utc;
    use intercept_proxy_domain::{
        FixedServerSettings, HttpListenerSettings, ListenerDataPlane, ListenerId,
        MetadataExtractor, MetadataExtractorId, ProxyListener, ResponseAssertion,
        ResponseAssertionId, UpstreamTlsSettings,
    };
    use intercept_proxy_product_api::ProductError;
    use intercept_proxy_runtime::{ChannelId, RawHeader};
    use uuid::Uuid;

    use super::*;
    use crate::WorkspaceRecord;

    #[derive(Debug)]
    struct Utf8;

    impl BodyCodec for Utf8 {
        fn id(&self) -> &'static str {
            "utf-8"
        }
        fn name(&self) -> &'static str {
            "UTF-8"
        }
        fn decode(&self, bytes: &[u8]) -> std::result::Result<String, ProductError> {
            String::from_utf8(bytes.to_vec())
                .map_err(|error| ProductError::new("UTF8", error.to_string()))
        }
        fn encode(&self, text: &str) -> std::result::Result<Vec<u8>, ProductError> {
            Ok(text.as_bytes().to_vec())
        }
    }

    #[test]
    fn selected_workspace_extracts_metadata_and_asserts_final_response_without_mutation() {
        let listener_id = ListenerId::new();
        let workspace = ProxyWorkspace {
            listeners: vec![ProxyListener {
                id: listener_id,
                name: "test".into(),
                enabled: false,
                bind_address: "127.0.0.1".into(),
                port: 18_443,
                data_plane: ListenerDataPlane::Http(HttpListenerSettings {
                    fixed_server: Some(FixedServerSettings {
                        upstream_url: "https://example.test".into(),
                        upstream_tls: UpstreamTlsSettings::default(),
                    }),
                    ..HttpListenerSettings::default()
                }),
                ..ProxyListener::default()
            }],
            metadata_extractors: vec![MetadataExtractor {
                id: MetadataExtractorId::new(),
                name: "business_result".into(),
                listener_ids: vec![listener_id],
                source: MetadataExtractorSource::JsonPath {
                    path: "$.result".into(),
                },
            }],
            response_assertions: vec![ResponseAssertion {
                id: ResponseAssertionId::new(),
                name: "configured business result".into(),
                listener_ids: vec![listener_id],
                enabled: true,
                assertion: ResponseAssertionKind::BodyTextContains {
                    expected: "EXPECTED".into(),
                },
            }],
            ..ProxyWorkspace::default()
        };
        let store = Arc::new(SqliteStore::in_memory().unwrap());
        store
            .insert_workspace(&WorkspaceRecord {
                id: workspace.id.as_uuid(),
                revision: workspace.revision.get(),
                value: encode_workspace_record(&workspace).unwrap(),
                updated_at: Utc::now(),
            })
            .unwrap();
        let resolver = WorkspaceRuntimePolicyResolver::new(store);
        let context = ConnectionContext {
            runtime_epoch: Uuid::new_v4(),
            connection_id: Uuid::new_v4(),
            channel: ChannelId::new(listener_id.to_string()).unwrap(),
            peer_addr: "127.0.0.1:12345".parse::<SocketAddr>().unwrap(),
            accepted_at: SystemTime::now(),
            tls_peer: None,
        };
        let original = Bytes::from_static(br#"{"result":"EXPECTED"}"#);
        let message = Message {
            start_line: "HTTP/1.1 200 OK".into(),
            headers: vec![RawHeader::new("Content-Type", "application/json")],
            body: original.clone(),
            body_modified: false,
        };
        let result = resolver
            .evaluate(&context, MessageStage::Response, &message, &Utf8)
            .unwrap();
        assert_eq!(
            result.metadata.get("business_result").map(String::as_str),
            Some("EXPECTED")
        );
        assert!(result.assertions.iter().all(|item| item.passed));
        assert_eq!(
            message.body, original,
            "assertions must never rewrite the response"
        );
    }
}
