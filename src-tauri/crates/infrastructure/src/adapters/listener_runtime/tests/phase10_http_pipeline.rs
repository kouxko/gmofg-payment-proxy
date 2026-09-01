use std::{
    collections::BTreeMap,
    net::SocketAddr,
    sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    },
    time::SystemTime,
};

use async_trait::async_trait;
use bytes::Bytes;
use intercept_proxy_domain::{
    BodyCodecKind, Condition, Document, DocumentMutation, DocumentPredicate, DocumentValue,
    ErrorCode as PackageErrorCode, HttpBodyProcessing, HttpListenerSettings, HttpRuleContent,
    JsonPointer, ListenerDataPlane, ProtocolDirection, ProtocolPackageId, ProtocolPackageRef,
    ProtocolPackageVersion, ProxyListener, ProxyWorkspace, RuleContent, RuleDefinition,
    RuleDefinitionDraft, RuleId, RuleRuntimeSnapshot, RuleStage, StringOperator, StringPredicate,
    UnifiedAction,
};
use intercept_proxy_exchange::{ExternalPackageCallStage, HttpContext};
use intercept_proxy_package_contract::{PackageManifest, PackageRpcError};
use intercept_proxy_product_api::{
    BodyCodec, ClassifiedRequest, ProductError, ProductMessageContext, RequestClassifier,
};
use intercept_proxy_runtime::{
    ConnectionContext, HttpConnectionIdentity, HttpProtocolCapabilityFactory, Message,
    PipelinePorts,
};
use parking_lot::Mutex;
use serde_json::json;
use uuid::Uuid;

use crate::adapters::{
    CaptureRepositoryAdapter, PackageTransportError, RuntimePipelineAdapter,
    RuntimePipelineProductHooks, pipeline::RuntimeRuleRepository,
};

use super::super::{
    external_relay::{
        ExternalPackageRpc, ExternalSocketPackageProvider, RuntimeExternalSocketPackageBinding,
    },
    http_protocol_pipeline::{
        HttpProtocolRuntimeSnapshot, JointDocumentEvaluation, decode_http_body_for_package,
    },
};
use super::{SqliteStore, test_listener_runtime};

fn set_string_rule(
    listener: &ProxyListener,
    id: RuleId,
    stage: RuleStage,
    created_order: u64,
    field: &str,
    mut conditions: Vec<Condition>,
    value: &str,
) -> RuleDefinition {
    if conditions.is_empty() {
        let decoded_field = match stage {
            RuleStage::ProxyToUpstream => "route",
            RuleStage::ProxyToApp => "result",
        };
        conditions.push(Condition::Document {
            path: JsonPointer::property(decoded_field),
            predicate: DocumentPredicate::String(StringPredicate {
                operator: StringOperator::Equal,
                value: "decoded".into(),
            }),
        });
    }
    RuleDefinition::restore(
        id,
        RuleDefinitionDraft {
            name: format!("{stage:?}"),
            enabled: true,
            priority: 10,
            listener_id: listener.id,
            stage,
            one_shot: false,
            content: RuleContent::Http(HttpRuleContent {
                description: String::new(),
                conditions,
                actions: vec![UnifiedAction::Document(DocumentMutation::Set {
                    path: JsonPointer::property(field),
                    value: DocumentValue::String(value.into()),
                })],
            }),
        },
        intercept_proxy_domain::RuleDefinitionRestoreSnapshot {
            revision: intercept_proxy_domain::Revision::INITIAL,
            created_order,
            lifecycle: intercept_proxy_domain::RuleLifecycle::default(),
        },
    )
    .unwrap()
}

fn workspace_with_http_rules(
    listener: &ProxyListener,
    rules: Vec<RuleDefinition>,
) -> ProxyWorkspace {
    let high_water = rules
        .iter()
        .map(RuleDefinition::created_order)
        .max()
        .unwrap_or(0);
    let mut workspace = ProxyWorkspace {
        listeners: vec![listener.clone()],
        rule_created_order_high_water: high_water,
        ..ProxyWorkspace::default()
    };
    workspace.rule_definitions = rules;
    workspace
}

#[path = "phase10_http_pipeline/mixed_rule_ownership.rs"]
mod mixed_rule_ownership;
#[path = "phase10_http_pipeline/production_shape.rs"]
mod production_shape;
#[path = "phase10_http_pipeline/request_metadata.rs"]
mod request_metadata;
#[path = "phase10_http_pipeline/unified_working_exchange.rs"]
mod unified_working_exchange;

#[test]
fn strict_http_package_codec_reads_original_utf8_and_shift_jis_wire_bytes() {
    let utf8 = HttpContext {
        header: "POST / HTTP/1.1\r\nContent-Type: application/json; charset=UTF-8\r\n\r\n".into(),
        body: "売上".into(),
        body_is_utf8: true,
        wire_body: "売上".as_bytes().to_vec(),
    };
    assert_eq!(
        decode_http_body_for_package(BodyCodecKind::Auto, &utf8).unwrap(),
        "売上"
    );

    let (shift_jis, _, had_errors) = encoding_rs::SHIFT_JIS.encode("売上");
    assert!(!had_errors);
    let shift_jis = HttpContext {
        header: "HTTP/1.1 200 OK\r\nContent-Type: text/plain; charset=Windows-31J\r\n\r\n".into(),
        body: String::from_utf8_lossy(&shift_jis).into_owned(),
        body_is_utf8: false,
        wire_body: shift_jis.to_vec(),
    };
    assert_eq!(
        decode_http_body_for_package(BodyCodecKind::Auto, &shift_jis).unwrap(),
        "売上"
    );
}

#[derive(Debug)]
struct RecordingHttpRpc {
    encode_calls: AtomicUsize,
    fail_encode: bool,
    fail_decode: bool,
    fail_display: bool,
}

impl RecordingHttpRpc {
    fn new(fail_encode: bool) -> Self {
        Self {
            encode_calls: AtomicUsize::new(0),
            fail_encode,
            fail_decode: false,
            fail_display: false,
        }
    }

    fn failing(stage: ExternalPackageCallStage) -> Self {
        Self {
            encode_calls: AtomicUsize::new(0),
            fail_encode: stage == ExternalPackageCallStage::Encode,
            fail_decode: stage == ExternalPackageCallStage::Decode,
            fail_display: stage == ExternalPackageCallStage::Display,
        }
    }
}

#[async_trait]
impl ExternalPackageRpc for RecordingHttpRpc {
    async fn decode_http(
        &self,
        _direction: ProtocolDirection,
        input: String,
    ) -> Result<Document, PackageTransportError> {
        if self.fail_decode {
            return Err(remote_failure(
                "phase10-decode-5",
                "hooks.upstream.decode",
                -32_408,
                PackageErrorCode::BodyDecodeFailed,
            ));
        }
        Document::parse_json(&input).map_err(|error| PackageTransportError::Package { error })
    }

    async fn encode_http(
        &self,
        _direction: ProtocolDirection,
        _original_input: String,
        document: Document,
    ) -> Result<String, PackageTransportError> {
        self.encode_calls.fetch_add(1, Ordering::SeqCst);
        if self.fail_encode {
            return Err(PackageTransportError::Remote {
                request_id: "phase10-encode-7".into(),
                method: "hooks.upstream.encode",
                error: PackageRpcError::new(
                    -32_410,
                    "encode rejected",
                    PackageErrorCode::BodyEncodeFailed,
                ),
            });
        }
        document
            .to_json()
            .map_err(|error| PackageTransportError::Package { error })
    }

    async fn display(
        &self,
        _direction: ProtocolDirection,
        document: Document,
    ) -> Result<String, PackageTransportError> {
        if self.fail_display {
            return Err(remote_failure(
                "phase10-display-6",
                "document.upstream.display",
                -32_409,
                PackageErrorCode::InternalError,
            ));
        }
        document
            .to_json()
            .map_err(|error| PackageTransportError::Package { error })
    }
}

fn remote_failure(
    request_id: &str,
    method: &'static str,
    code: i64,
    stable_code: PackageErrorCode,
) -> PackageTransportError {
    PackageTransportError::Remote {
        request_id: request_id.into(),
        method,
        error: PackageRpcError::new(code, "remote rejected", stable_code),
    }
}

#[derive(Debug)]
struct StrictTestUtf8;

impl BodyCodec for StrictTestUtf8 {
    fn id(&self) -> &'static str {
        "phase10-test-utf8"
    }

    fn name(&self) -> &'static str {
        "Phase10 test UTF-8"
    }

    fn decode(&self, bytes: &[u8]) -> Result<String, ProductError> {
        String::from_utf8(bytes.to_vec())
            .map_err(|error| ProductError::new("BODY_DECODE_FAILED", error.to_string()))
    }

    fn encode(&self, text: &str) -> Result<Vec<u8>, ProductError> {
        Ok(text.as_bytes().to_vec())
    }
}

fn external_evaluation(
    rpc: Arc<RecordingHttpRpc>,
    original: Document,
    current: Document,
) -> JointDocumentEvaluation {
    JointDocumentEvaluation::new_external(
        current,
        original,
        "original-input".into(),
        rpc,
        ProtocolDirection::Upstream,
        Arc::new(StrictTestUtf8),
        ProtocolPackageRef {
            id: ProtocolPackageId::new("phase10.http").unwrap(),
            version: ProtocolPackageVersion::new("1.0.0").unwrap(),
        },
        std::iter::empty(),
    )
}

fn wire_message(body: &'static [u8]) -> Message {
    Message::from_raw_http1_head(b"POST / HTTP/1.1\r\n\r\n", Bytes::from_static(body)).unwrap()
}

fn http_registration() -> PackageManifest {
    serde_json::from_value(json!({
        "api": 1,
        "kind": "http",
        "package": {
            "id": "http-pipeline-test",
            "name": "Phase10 HTTP",
            "version": "1.0.0",
            "description": "test"
        },
        "document": {
            "upstream": {"schema": {"type": "object", "properties": {"value": {"type": "string"}}}},
            "downstream": {"schema": {"type": "object", "properties": {"value": {"type": "string"}}}}
        }
    }))
    .unwrap()
}

fn http_registration_without_schema() -> PackageManifest {
    let mut value = serde_json::to_value(http_registration()).expect("manifest value");
    for direction in ["upstream", "downstream"] {
        value["document"][direction]
            .as_object_mut()
            .expect("direction object")
            .remove("schema");
    }
    serde_json::from_value(value).expect("schema-free HTTP manifest")
}

async fn prepared_external_snapshot(
    rpc: Arc<RecordingHttpRpc>,
) -> Arc<HttpProtocolRuntimeSnapshot> {
    let listener = phase10_listener();
    let workspace = ProxyWorkspace::default();
    production_shape::prepared_external_snapshot_for(rpc, &workspace, &listener).await
}

async fn prepared_plain_snapshot(
    workspace: &ProxyWorkspace,
    listener: &ProxyListener,
) -> Arc<HttpProtocolRuntimeSnapshot> {
    let adapter = test_listener_runtime(Arc::new(SqliteStore::in_memory().unwrap()));
    HttpProtocolRuntimeSnapshot::prepare_async(&adapter, workspace, listener)
        .await
        .unwrap()
        .unwrap()
}

fn phase10_listener() -> ProxyListener {
    let package = http_registration().package().identity().clone();
    ProxyListener {
        data_plane: ListenerDataPlane::Http(HttpListenerSettings {
            body_processing: HttpBodyProcessing::Protocol { package },
            ..HttpListenerSettings::default()
        }),
        ..ProxyListener::default()
    }
}

#[tokio::test]
async fn remote_decode_and_display_failures_keep_typed_json_rpc_identity() {
    let context = HttpContext {
        header: "POST / HTTP/1.1\r\nContent-Type: application/json; charset=utf-8\r\n\r\n".into(),
        body: r#"{"value":"old"}"#.into(),
        body_is_utf8: true,
        wire_body: br#"{"value":"old"}"#.to_vec(),
    };
    let identity = HttpConnectionIdentity {
        runtime_epoch: Uuid::from_u128(911),
        connection_id: Uuid::from_u128(912),
        peer: "127.0.0.1:1912".into(),
    };
    let decode_snapshot = prepared_external_snapshot(Arc::new(RecordingHttpRpc::failing(
        ExternalPackageCallStage::Decode,
    )))
    .await;
    let mut decode = decode_snapshot.create_upstream(identity.clone()).unwrap();
    let decode_error = decode.decode.decode(&context).await.unwrap_err();
    let decode_failure = decode_error.external_package_call.unwrap();
    assert_eq!(decode_failure.stage, ExternalPackageCallStage::Decode);
    assert_eq!(decode_failure.method, "hooks.upstream.decode");
    assert_eq!(
        decode_failure.request_id.as_deref(),
        Some("phase10-decode-5")
    );
    assert_eq!(decode_failure.remote_code, Some(-32_408));

    let display_snapshot = prepared_external_snapshot(Arc::new(RecordingHttpRpc::failing(
        ExternalPackageCallStage::Display,
    )))
    .await;
    let mut display = display_snapshot.create_upstream(identity).unwrap();
    let document = display.decode.decode(&context).await.unwrap();
    let display_error = display.display.display(&document).await.unwrap_err();
    let display_failure = display_error.external_package_call.unwrap();
    assert_eq!(display_failure.stage, ExternalPackageCallStage::Display);
    assert_eq!(display_failure.method, "document.upstream.display");
    assert_eq!(
        display_failure.request_id.as_deref(),
        Some("phase10-display-6")
    );
    assert_eq!(display_failure.remote_code, Some(-32_409));
}

#[tokio::test]
async fn unchanged_external_document_forwards_original_wire_bytes_without_encode_rpc() {
    let rpc = Arc::new(RecordingHttpRpc::new(false));
    let original = Document::parse_json(r#"{"value":"old"}"#).unwrap();
    let mut message = wire_message(b"original-wire-bytes");

    external_evaluation(Arc::clone(&rpc), original.clone(), original)
        .encode_into(&mut message)
        .await
        .unwrap();

    assert_eq!(message.body, Bytes::from_static(b"original-wire-bytes"));
    assert_eq!(rpc.encode_calls.load(Ordering::SeqCst), 0);
}

#[tokio::test]
async fn changed_external_document_uses_encode_rpc_and_encode_failure_fails_closed() {
    let original = Document::parse_json(r#"{"value":"old"}"#).unwrap();
    let changed = Document::parse_json(r#"{"value":"new"}"#).unwrap();
    let rpc = Arc::new(RecordingHttpRpc::new(false));
    let mut message = wire_message(b"original-wire-bytes");

    external_evaluation(Arc::clone(&rpc), original.clone(), changed.clone())
        .encode_into(&mut message)
        .await
        .unwrap();
    assert_eq!(message.body, Bytes::from_static(br#"{"value":"new"}"#));
    assert_eq!(rpc.encode_calls.load(Ordering::SeqCst), 1);

    let failing = Arc::new(RecordingHttpRpc::new(true));
    let error = external_evaluation(failing, original, changed)
        .encode_into(&mut wire_message(b"original-wire-bytes"))
        .await
        .unwrap_err();
    let failure = error.external_package_call.expect("typed remote failure");
    assert_eq!(failure.stage, ExternalPackageCallStage::Encode);
    assert_eq!(failure.method, "hooks.upstream.encode");
    assert_eq!(failure.request_id.as_deref(), Some("phase10-encode-7"));
    assert_eq!(failure.remote_code, Some(-32_410));
    assert_eq!(failure.stable_code.as_deref(), Some("BODY_ENCODE_FAILED"));
}

#[test]
fn http_package_codec_rejects_unknown_charset_and_non_identity_content_encoding() {
    for (header, code) in [
        (
            "POST / HTTP/1.1\r\nContent-Type: text/plain; charset=iso-8859-1\r\n\r\n",
            "HTTP_BODY_CHARSET_UNSUPPORTED",
        ),
        (
            "POST / HTTP/1.1\r\nContent-Type: text/plain; charset=utf-8\r\nContent-Encoding: gzip\r\n\r\n",
            "HTTP_CONTENT_ENCODING_UNSUPPORTED",
        ),
    ] {
        let context = HttpContext {
            header: header.into(),
            body: "wire".into(),
            body_is_utf8: true,
            wire_body: b"wire".to_vec(),
        };
        let error = decode_http_body_for_package(BodyCodecKind::Auto, &context).unwrap_err();
        assert!(error.message.starts_with(code), "{}", error.message);
    }
}
