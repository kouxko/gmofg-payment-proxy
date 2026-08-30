//! 外部 Socket Relay processor 的合同测试。

use std::sync::Arc;

use async_trait::async_trait;
use intercept_proxy_domain::{
    Document, DocumentAction, DocumentValue, ErrorCode, JsonPointer, ListenerId,
    ProtocolDocumentRuleDefinition, ProtocolDocumentRuleId, ProtocolDocumentRuleProgram,
    SocketTopology,
};
use intercept_proxy_exchange::{FrameResult, SocketContext};
use intercept_proxy_package_contract::{
    DecodeParams, DisplayParams, EncodeParams, FrameParams, FrameResult as PackageFrameResult,
    PackageManifest, PackageRpcError,
};
use parking_lot::Mutex;
use serde_json::json;
use uuid::Uuid;

use super::*;
use crate::adapters::PackageTransportError;

#[test]
fn remote_data_diagnostic_is_shape_only() {
    let summary = redacted_data_summary(Some(&json!({
        "pan": "4111111111111111",
        "nested": {"secret": "must-not-leak"}
    })));

    assert_eq!(summary, "object(fields=2)");
    assert!(!summary.contains("4111111111111111"));
    assert!(!summary.contains("secret"));
}

#[test]
fn remote_data_diagnostic_summarizes_every_json_shape_without_values() {
    let cases = [
        (None, "none"),
        (Some(serde_json::Value::Null), "null"),
        (Some(json!(true)), "bool"),
        (Some(json!(42)), "number"),
        (Some(json!("secret")), "string(bytes=6)"),
        (Some(json!(["first", "second"])), "array(items=2)"),
        (Some(json!({"first": 1, "second": 2})), "object(fields=2)"),
    ];

    for (data, expected) in cases {
        assert_eq!(redacted_data_summary(data.as_ref()), expected);
    }
}

#[test]
fn remote_error_diagnostic_preserves_correlation_without_payload_values() {
    let package = registration().package().identity().clone();
    let error = PackageTransportError::Remote {
        request_id: "g7-c42".to_owned(),
        method: "hooks.upstream.decode",
        error: PackageRpcError::new(
            -32_001,
            "decoder rejected message".to_owned(),
            ErrorCode::BodyDecodeFailed,
        ),
    };
    let diagnostic = trace_external_rpc_failure(
        &package,
        &connection(),
        ProtocolDirection::Upstream,
        ExternalPackageCallStage::Decode,
        "hooks.upstream.decode",
        &error,
    );

    assert_eq!(diagnostic.request_id.as_deref(), Some("g7-c42"));
    assert_eq!(diagnostic.remote_code, Some(-32_001));
    assert_eq!(
        diagnostic.stable_code.as_deref(),
        Some("BODY_DECODE_FAILED")
    );
    assert_eq!(
        diagnostic.remote_message.as_deref(),
        Some("decoder rejected message")
    );
    assert_eq!(
        diagnostic.remote_data_summary.as_deref(),
        Some("object(fields=1)")
    );
    assert!(!format!("{diagnostic:?}").contains("4111111111111111"));
}

#[test]
fn non_rpc_error_diagnostic_is_uncorrelated_and_supports_downstream_direction() {
    let package = registration().package().identity().clone();

    let diagnostic = trace_external_rpc_failure(
        &package,
        &connection(),
        ProtocolDirection::Downstream,
        ExternalPackageCallStage::Encode,
        "hooks.downstream.encode_and_encrypt",
        &PackageTransportError::Disconnected,
    );

    assert_eq!(diagnostic.direction, ProtocolDirection::Downstream);
    assert_eq!(diagnostic.stage, ExternalPackageCallStage::Encode);
    assert_eq!(diagnostic.request_id, None);
    assert_eq!(diagnostic.remote_code, None);
    assert_eq!(diagnostic.remote_message, None);
    assert_eq!(diagnostic.remote_data_summary, None);
}

#[tokio::test]
async fn capabilities_run_frame_decode_display_rules_encode_in_order() {
    let registration = registration();
    let rpc = Arc::new(FakeExternalRpc::default());
    let snapshot = ExternalSocketRuntimeSnapshot::new(
        ExternalSocketPackageBinding::new(registration.clone(), rpc.clone()),
        rules(&registration),
        SocketTopology::default(),
    );
    let factory = ExternalSocketCapabilityFactoryAdapter::new(&snapshot, observation_metadata());
    let mut capabilities = factory.create_upstream(connection()).unwrap();

    assert_eq!(
        capabilities.frame.split(b"abc").await.unwrap(),
        FrameResult::Complete { consumed: 3 }
    );
    let context = SocketContext {
        data: b"abc".to_vec(),
    };
    let document = capabilities.decode.decode(&context).await.unwrap();
    assert_eq!(
        capabilities.display.display(&document).await.unwrap(),
        "<p>ok</p>"
    );
    let document = capabilities.rules.apply(document).await.unwrap();
    let encoded = capabilities
        .encode
        .encode(&context, &document)
        .await
        .unwrap();
    assert_eq!(encoded.data, b"encoded");

    assert_eq!(
        rpc.calls.lock().as_slice(),
        [
            "hooks.upstream.frame",
            "hooks.upstream.decode",
            "document.upstream.display",
            "hooks.upstream.encode",
        ]
    );
    let encoded_document = rpc.encoded_document.lock().clone().unwrap();
    assert_eq!(
        serde_json::to_value(encoded_document).unwrap()["amount"],
        json!(42.0)
    );
}

#[derive(Debug, Default)]
struct FakeExternalRpc {
    calls: Mutex<Vec<&'static str>>,
    encoded_document: Mutex<Option<Document>>,
}

impl FakeExternalRpc {
    fn record_method(&self, method: &'static str) {
        self.calls.lock().push(method);
    }
}

#[async_trait]
impl ExternalPackageRpc for FakeExternalRpc {
    async fn frame(
        &self,
        direction: ProtocolDirection,
        request: FrameParams,
    ) -> Result<PackageFrameResult, PackageTransportError> {
        self.record_method(match direction {
            ProtocolDirection::Upstream => "hooks.upstream.frame",
            ProtocolDirection::Downstream => "hooks.downstream.frame",
        });
        Ok(PackageFrameResult::complete(request.buffer.bytes().len()).unwrap())
    }

    async fn decode(
        &self,
        direction: ProtocolDirection,
        _request: DecodeParams,
    ) -> Result<Document, PackageTransportError> {
        self.record_method(match direction {
            ProtocolDirection::Upstream => "hooks.upstream.decode",
            ProtocolDirection::Downstream => "hooks.downstream.decode",
        });
        Ok(serde_json::from_value(json!({"message_type": "0200"})).unwrap())
    }

    async fn encode(
        &self,
        direction: ProtocolDirection,
        request: EncodeParams,
    ) -> Result<String, PackageTransportError> {
        self.record_method(match direction {
            ProtocolDirection::Upstream => "hooks.upstream.encode",
            ProtocolDirection::Downstream => "hooks.downstream.encode",
        });
        *self.encoded_document.lock() = Some(request.document.clone());
        Ok("ZW5jb2RlZA==".to_owned())
    }

    async fn display(
        &self,
        direction: ProtocolDirection,
        _request: DisplayParams,
    ) -> Result<String, PackageTransportError> {
        self.record_method(match direction {
            ProtocolDirection::Upstream => "document.upstream.display",
            ProtocolDirection::Downstream => "document.downstream.display",
        });
        Ok("<p>ok</p>".to_owned())
    }
}

fn registration() -> PackageManifest {
    serde_json::from_value(json!({
        "api": 1,
        "kind": "socket",
        "package": {
            "id": "external-runtime-test",
            "name": "External runtime test",
            "version": "1.0.0",
            "description": "test"
        },
        "document": {
            "upstream": {
                "schema": {
                    "type": "object",
                    "title": "Upstream",
                    "properties": {
                        "message_type": {"type": "string", "title": "MTI"},
                        "amount": {"type": "number", "title": "Amount"}
                    }
                }
            },
            "downstream": {
                "schema": {
                    "type": "object",
                    "title": "Downstream",
                    "properties": {
                        "response_code": {"type": "string", "title": "Response code"}
                    }
                }
            }
        }
    }))
    .unwrap()
}

fn observation_metadata() -> SocketObservationMetadata {
    SocketObservationMetadata {
        workspace_id: "test-workspace".to_owned(),
        listener_id: listener_id().to_string(),
    }
}

fn listener_id() -> ListenerId {
    ListenerId::from_uuid(Uuid::from_u128(10))
}

fn connection() -> SocketConnectionIdentity {
    SocketConnectionIdentity {
        runtime_epoch: Uuid::from_u128(1),
        connection_id: Uuid::from_u128(2),
        peer_addr: "127.0.0.1:12345".parse().unwrap(),
    }
}

fn rules(registration: &PackageManifest) -> ProtocolDocumentRuleConnectionFactory {
    let package = registration.package().identity().clone();
    let upstream = registration.document().upstream().schema().unwrap().clone();
    let downstream = registration
        .document()
        .downstream()
        .schema()
        .unwrap()
        .clone();
    let rule = ProtocolDocumentRuleDefinition::new_named_for_stage(
        ProtocolDocumentRuleId::new(),
        "set amount".to_owned(),
        true,
        10,
        1,
        listener_id(),
        package.clone(),
        ProtocolRuleStage::ProxyToUpstream,
        Vec::new(),
        vec![DocumentAction::SetField {
            field: JsonPointer::property("amount"),
            value: DocumentValue::integer(42).unwrap(),
        }],
    )
    .unwrap();
    let program = |stage, schema, rules| {
        Arc::new(
            ProtocolDocumentRuleProgram::new_for_stage(
                listener_id(),
                package.clone(),
                schema,
                stage,
                rules,
            )
            .unwrap(),
        )
    };
    ProtocolDocumentRuleConnectionFactory::new(
        program(ProtocolRuleStage::AppToProxy, upstream.clone(), Vec::new()),
        program(ProtocolRuleStage::ProxyToUpstream, upstream, vec![rule]),
        program(
            ProtocolRuleStage::UpstreamToProxy,
            downstream.clone(),
            Vec::new(),
        ),
        program(ProtocolRuleStage::ProxyToApp, downstream, Vec::new()),
    )
    .unwrap()
}
