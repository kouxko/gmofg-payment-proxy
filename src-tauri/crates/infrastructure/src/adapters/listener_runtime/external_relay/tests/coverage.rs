//! 外部 Relay 数据面的失败语义与启动快照覆盖。

use std::{fmt, sync::Arc, time::Duration};

use intercept_proxy_domain::{
    ExternalDecodeResponse, ExternalDisplayResponse, ExternalEncodeResponse, ExternalFrameResult,
    ListenerDataPlane, ProxyListener, ProxyWorkspace, ScriptedSocketProcessing,
    SocketPayloadProcessing, SocketRelaySettings,
};

use super::*;

#[derive(Clone, Copy, Debug)]
enum RpcBehavior {
    Success,
    FrameFailure,
    DecodeFailure,
    EncodeFailure,
    InvalidDocument,
    InvalidBytes,
}

#[derive(Debug)]
struct BehaviorRpc(RpcBehavior);

#[async_trait]
impl ExternalPackageRpc for BehaviorRpc {
    async fn frame(
        &self,
        _: &str,
        request: &ExternalFrameRequest,
    ) -> Result<ExternalFrameResult, ExternalPackageConnectionError> {
        if matches!(self.0, RpcBehavior::FrameFailure) {
            return Err(ExternalPackageConnectionError::Disconnected);
        }
        Ok(ExternalFrameResult::Complete {
            consumed_bytes: request.bytes().expect("valid request bytes").len(),
        })
    }

    async fn decode(
        &self,
        method: &str,
        _: &ExternalDecodeRequest,
    ) -> Result<ExternalDecodeResponse, ExternalPackageConnectionError> {
        if matches!(self.0, RpcBehavior::DecodeFailure) {
            return Err(ExternalPackageConnectionError::Disconnected);
        }
        let document = if matches!(self.0, RpcBehavior::InvalidDocument) {
            json!({"unknown": {"type": "string", "value": "not in schema"}})
        } else if method.contains("downstream") {
            json!({"response_code": {"type": "string", "value": "00"}})
        } else {
            json!({"message_type": {"type": "string", "value": "0200"}})
        };
        Ok(ExternalDecodeResponse {
            document: serde_json::from_value(document).expect("wire document"),
        })
    }

    async fn encode(
        &self,
        _: &str,
        _: &ExternalEncodeRequest,
    ) -> Result<ExternalEncodeResponse, ExternalPackageConnectionError> {
        if matches!(self.0, RpcBehavior::EncodeFailure) {
            return Err(ExternalPackageConnectionError::Disconnected);
        }
        if matches!(self.0, RpcBehavior::InvalidBytes) {
            return Ok(serde_json::from_value(json!({"frame_base64": "%%%"}))
                .expect("syntactically valid response"));
        }
        Ok(ExternalEncodeResponse::from_bytes(b"encoded"))
    }

    async fn display(
        &self,
        _: &str,
        _: &ExternalDisplayRequest,
    ) -> Result<ExternalDisplayResponse, ExternalPackageConnectionError> {
        Ok(ExternalDisplayResponse {
            html: "ok".to_owned(),
        })
    }
}

#[tokio::test]
async fn local_exchange_direction_is_rejected_by_both_processor_operations() {
    let mut processor = factory(RpcBehavior::Success)
        .create_direction(connection(), SocketPayloadDirection::LocalExchange);

    let inspect = processor
        .inspect(Bytes::from_static(b"frame"))
        .await
        .expect_err("LocalExchange is not a relay direction");
    let process = processor
        .process(Bytes::from_static(b"frame"))
        .await
        .expect_err("LocalExchange is not a relay direction");

    assert_eq!(inspect.kind, SocketProcessingFailureKind::ProcessingFailed);
    assert_eq!(process.kind, SocketProcessingFailureKind::ProcessingFailed);
}

#[tokio::test]
async fn downstream_direction_uses_downstream_contract() {
    let mut processor = factory(RpcBehavior::Success)
        .create_direction(connection(), SocketPayloadDirection::UpstreamToApp);

    assert_eq!(
        processor
            .process(Bytes::from_static(b"response"))
            .await
            .expect("downstream frame"),
        Bytes::from_static(b"encoded")
    );
}

#[tokio::test]
async fn frame_transport_failure_is_processing_failed() {
    let mut processor = factory(RpcBehavior::FrameFailure)
        .create_direction(connection(), SocketPayloadDirection::AppToUpstream);

    let failure = processor
        .inspect(Bytes::from_static(b"frame"))
        .await
        .expect_err("disconnected frame RPC must fail closed");

    assert_eq!(failure.kind, SocketProcessingFailureKind::ProcessingFailed);
}

#[tokio::test]
async fn decode_transport_failure_is_decode_failed() {
    let failure = process_failure(RpcBehavior::DecodeFailure).await;

    assert_eq!(failure.kind, SocketProcessingFailureKind::DecodeFailed);
}

#[tokio::test]
async fn encode_transport_failure_is_encode_failed() {
    let failure = process_failure(RpcBehavior::EncodeFailure).await;

    assert_eq!(failure.kind, SocketProcessingFailureKind::EncodeFailed);
}

#[tokio::test]
async fn schema_incompatible_decode_document_is_decode_failed() {
    let failure = process_failure(RpcBehavior::InvalidDocument).await;

    assert_eq!(failure.kind, SocketProcessingFailureKind::DecodeFailed);
}

#[tokio::test]
async fn invalid_encoded_base64_is_encode_failed() {
    let failure = process_failure(RpcBehavior::InvalidBytes).await;

    assert_eq!(failure.kind, SocketProcessingFailureKind::EncodeFailed);
}

#[tokio::test]
async fn uncommitted_output_rejects_the_next_frame() {
    let factory = factory(RpcBehavior::Success);
    let mut processor =
        factory.create_direction(connection(), SocketPayloadDirection::AppToUpstream);
    processor
        .process(Bytes::from_static(b"first"))
        .await
        .expect("first frame");

    let failure = processor
        .process(Bytes::from_static(b"second"))
        .await
        .expect_err("pending output must be committed before another frame");

    assert_eq!(failure.kind, SocketProcessingFailureKind::ProcessingFailed);
}

#[test]
fn committing_without_pending_output_is_a_noop() {
    let factory = factory(RpcBehavior::Success);
    let mut processor =
        factory.create_direction(connection(), SocketPayloadDirection::AppToUpstream);

    processor.output_committed();
}

#[test]
fn binding_debug_redacts_rpc_and_preserves_package_identity() {
    let binding = ExternalSocketPackageBinding::with_limits(
        registration(),
        Arc::new(BehaviorRpc(RpcBehavior::Success)),
        4096,
        Duration::from_millis(250),
    );

    let debug = format!("{binding:?}");

    assert!(debug.contains("external-runtime-test"));
    assert!(!debug.contains("BehaviorRpc"));
}

#[test]
fn binding_exposes_the_configured_frame_and_timeout_limits() {
    let binding = ExternalSocketPackageBinding::with_limits(
        registration(),
        Arc::new(BehaviorRpc(RpcBehavior::Success)),
        4096,
        Duration::from_millis(250),
    );

    assert_eq!(binding.max_frame_bytes(), 4096);
    assert_eq!(binding.rpc_timeout(), Duration::from_millis(250));
}

#[test]
fn runtime_snapshot_debug_redacts_rule_programs() {
    let snapshot = snapshot(RpcBehavior::Success);

    let debug = format!("{snapshot:?}");

    assert!(debug.contains("ExternalSocketRuntimeSnapshot"));
    assert!(!debug.contains("set amount"));
}

#[test]
fn runtime_snapshot_replaces_rules_from_the_current_workspace() {
    let snapshot = snapshot(RpcBehavior::Success);
    let listener = external_listener();
    let workspace = ProxyWorkspace {
        listeners: vec![listener.clone()],
        ..ProxyWorkspace::default()
    };

    snapshot
        .replace_document_rules(&workspace, &listener)
        .expect("empty rule set compiles");
}

#[test]
fn redacted_data_summary_reports_every_json_shape_without_values() {
    let cases = [
        (None, "none"),
        (Some(json!(null)), "null"),
        (Some(json!(true)), "bool"),
        (Some(json!(42)), "number"),
        (Some(json!("secret")), "string(bytes=6)"),
        (Some(json!(["secret", 42])), "array(items=2)"),
    ];

    for (value, expected) in cases {
        assert_eq!(redacted_data_summary(value.as_ref()), expected);
    }
}

async fn process_failure(behavior: RpcBehavior) -> SocketProcessingFailure {
    let factory = factory(behavior);
    let mut processor =
        factory.create_direction(connection(), SocketPayloadDirection::AppToUpstream);
    processor
        .process(Bytes::from_static(b"frame"))
        .await
        .expect_err("configured failure")
}

fn factory(behavior: RpcBehavior) -> ExternalRelayProcessorFactoryAdapter {
    let snapshot = snapshot(behavior);
    ExternalRelayProcessorFactoryAdapter::new(
        &snapshot,
        SocketCaptureContext {
            workspace_id: intercept_proxy_domain::WorkspaceId::new(),
            listener_id: listener_id(),
            publisher: None,
        },
    )
}

fn snapshot(behavior: RpcBehavior) -> ExternalSocketRuntimeSnapshot {
    let registration = registration();
    ExternalSocketRuntimeSnapshot::new(
        ExternalSocketPackageBinding::new(registration.clone(), Arc::new(BehaviorRpc(behavior))),
        rules(&registration),
        SocketTopology::default(),
    )
}

fn external_listener() -> ProxyListener {
    ProxyListener {
        id: listener_id(),
        data_plane: ListenerDataPlane::Socket(SocketRelaySettings {
            topology: SocketTopology::default(),
            maximum_connections: 1,
            processing: SocketPayloadProcessing::Scripted(ScriptedSocketProcessing {
                package: registration().package().identity().clone(),
            }),
        }),
        ..ProxyListener::default()
    }
}

impl fmt::Display for RpcBehavior {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "{self:?}")
    }
}
