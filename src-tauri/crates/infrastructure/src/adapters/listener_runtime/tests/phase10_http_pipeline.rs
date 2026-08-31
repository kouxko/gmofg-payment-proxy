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
    BodyCodecKind, Document, DocumentValue, ErrorCode as PackageErrorCode, HttpBodyProcessing,
    HttpListenerSettings, HttpRuleContent, JsonPointer, ListenerDataPlane, ProtocolDirection,
    ProtocolDocumentOperation, ProtocolDocumentPredicate, ProtocolDocumentRuleDefinition,
    ProtocolDocumentRuleId, ProtocolPackageId, ProtocolPackageRef, ProtocolPackageVersion,
    ProtocolRuleStage, ProxyListener, ProxyWorkspace, RuleContent, RuleDefinition,
    RuleDefinitionDraft, RuleRuntimeSnapshot, SocketRuleContent,
};
use intercept_proxy_exchange::{ExternalPackageCallStage, HttpContext};
use intercept_proxy_package_contract::{
    DecodeParams, DisplayParams, EncodeParams, FrameParams, FrameResult, PackageManifest,
    PackageRpcError,
};
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

fn phase10_package() -> ProtocolPackageRef {
    ProtocolPackageRef {
        id: ProtocolPackageId::new("http-pipeline-test").unwrap(),
        version: ProtocolPackageVersion::new("1.0.0").unwrap(),
    }
}

fn set_string_rule(
    listener: &ProxyListener,
    id: ProtocolDocumentRuleId,
    stage: ProtocolRuleStage,
    created_order: u64,
    field: &str,
    mut conditions: Vec<ProtocolDocumentPredicate>,
    value: &str,
) -> ProtocolDocumentRuleDefinition {
    if conditions.is_empty() {
        let decoded_field = match stage {
            ProtocolRuleStage::ProxyToUpstream => "route",
            ProtocolRuleStage::ProxyToApp => "result",
        };
        conditions.push(ProtocolDocumentPredicate::Equals {
            field: JsonPointer::property(decoded_field),
            value: DocumentValue::String("decoded".into()),
        });
    }
    ProtocolDocumentRuleDefinition::new_named_for_stage(
        id,
        format!("{stage:?}"),
        true,
        10,
        created_order,
        listener.id,
        phase10_package(),
        stage,
        conditions,
        vec![ProtocolDocumentOperation::SetField {
            field: JsonPointer::property(field),
            value: DocumentValue::String(value.into()),
        }],
    )
    .unwrap()
}

fn workspace_with_http_rules(
    listener: &ProxyListener,
    rules: Vec<ProtocolDocumentRuleDefinition>,
) -> ProxyWorkspace {
    let high_water = rules
        .iter()
        .map(ProtocolDocumentRuleDefinition::created_order)
        .max()
        .unwrap_or(0);
    let mut workspace = ProxyWorkspace {
        listeners: vec![listener.clone()],
        rule_created_order_high_water: high_water,
        ..ProxyWorkspace::default()
    };
    workspace.replace_document_runtime_rules(rules).unwrap();
    workspace.rule_definitions = workspace
        .rule_definitions
        .iter()
        .map(|definition| {
            let RuleContent::Socket(SocketRuleContent {
                package,
                condition,
                actions,
            }) = definition.content()
            else {
                return definition.clone();
            };
            RuleDefinition::restore(
                definition.rule_id(),
                RuleDefinitionDraft {
                    name: definition.name().to_owned(),
                    enabled: definition.enabled(),
                    priority: definition.priority(),
                    listener_id: definition.listener_id(),
                    stage: definition.stage(),
                    one_shot: definition.one_shot(),
                    content: RuleContent::Http(HttpRuleContent {
                        description: String::new(),
                        condition: condition.clone(),
                        actions: actions.clone(),
                        document: Some(intercept_proxy_domain::HttpDocumentRuleContent {
                            package: package.clone(),
                        }),
                    }),
                },
                intercept_proxy_domain::RuleDefinitionRestoreSnapshot {
                    revision: definition.revision(),
                    created_order: definition.created_order(),
                    lifecycle: definition.lifecycle().clone(),
                },
            )
            .unwrap()
        })
        .collect();
    workspace
}

#[path = "phase10_http_pipeline/mixed_rule_ownership.rs"]
mod mixed_rule_ownership;
#[path = "phase10_http_pipeline/production_shape.rs"]
mod production_shape;

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
    async fn frame(
        &self,
        _direction: ProtocolDirection,
        _request: FrameParams,
    ) -> Result<FrameResult, PackageTransportError> {
        unreachable!("HTTP pipeline does not call Socket framing")
    }

    async fn decode(
        &self,
        _direction: ProtocolDirection,
        request: DecodeParams,
    ) -> Result<Document, PackageTransportError> {
        if self.fail_decode {
            return Err(remote_failure(
                "phase10-decode-5",
                "hooks.upstream.decode",
                -32_408,
                PackageErrorCode::BodyDecodeFailed,
            ));
        }
        Document::parse_json(&request.input)
            .map_err(|error| PackageTransportError::Package { error })
    }

    async fn encode(
        &self,
        _direction: ProtocolDirection,
        request: EncodeParams,
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
        request
            .document
            .to_json()
            .map_err(|error| PackageTransportError::Package { error })
    }

    async fn display(
        &self,
        _direction: ProtocolDirection,
        request: DisplayParams,
    ) -> Result<String, PackageTransportError> {
        if self.fail_display {
            return Err(remote_failure(
                "phase10-display-6",
                "document.upstream.display",
                -32_409,
                PackageErrorCode::InternalError,
            ));
        }
        request
            .document
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

async fn prepared_external_snapshot(
    rpc: Arc<RecordingHttpRpc>,
) -> Arc<HttpProtocolRuntimeSnapshot> {
    let listener = phase10_listener();
    let workspace = ProxyWorkspace::default();
    production_shape::prepared_external_snapshot_for(rpc, &workspace, &listener).await
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
