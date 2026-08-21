//! 外部 `LocalResponder` 数据面的失败语义覆盖。

use std::sync::Arc;

use intercept_proxy_domain::{
    ExternalDecodeResponse, ExternalDisplayResponse, ExternalEncodeResponse, ExternalFrameResult,
};

use super::*;

#[derive(Clone, Copy, Debug)]
enum RpcBehavior {
    Success,
    NeedMore,
    FrameFailure,
    DecodeFailure,
    EncodeFailure,
    DecodeTimeout,
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
        match self.0 {
            RpcBehavior::FrameFailure => Err(ExternalPackageConnectionError::Disconnected),
            RpcBehavior::NeedMore => Ok(ExternalFrameResult::NeedMore),
            _ => Ok(ExternalFrameResult::Complete {
                consumed_bytes: request.bytes().expect("valid frame request").len(),
            }),
        }
    }

    async fn decode(
        &self,
        _: &str,
        _: &ExternalDecodeRequest,
    ) -> Result<ExternalDecodeResponse, ExternalPackageConnectionError> {
        if matches!(self.0, RpcBehavior::DecodeFailure) {
            return Err(ExternalPackageConnectionError::Disconnected);
        }
        if matches!(self.0, RpcBehavior::DecodeTimeout) {
            return Err(ExternalPackageConnectionError::Timeout {
                request_id: "g1-c1".to_owned(),
                method: "hooks.upstream.decode".to_owned(),
            });
        }
        let document = if matches!(self.0, RpcBehavior::InvalidDocument) {
            json!({"unknown": {"type": "string", "value": "not in schema"}})
        } else {
            json!({"request": {"type": "string", "value": "sale"}})
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
        Ok(ExternalEncodeResponse::from_bytes(b"approved"))
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
async fn inspect_maps_need_more_without_inventing_a_frame_length() {
    let mut processor = factory(RpcBehavior::NeedMore).create_exchange(connection());

    let boundary = processor
        .inspect(Bytes::from_static(b"partial"))
        .await
        .expect("need-more result");

    assert_eq!(boundary, FrameBoundary::NeedMoreUnknown);
}

#[tokio::test]
async fn inspect_maps_complete_to_the_external_consumed_length() {
    let mut processor = factory(RpcBehavior::Success).create_exchange(connection());

    let boundary = processor
        .inspect(Bytes::from_static(b"complete"))
        .await
        .expect("complete result");

    assert_eq!(boundary, FrameBoundary::Complete { bytes: 8 });
}

#[tokio::test]
async fn frame_transport_failure_is_processing_failed() {
    let mut processor = factory(RpcBehavior::FrameFailure).create_exchange(connection());

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
async fn decode_timeout_is_processing_timeout() {
    let failure = process_failure(RpcBehavior::DecodeTimeout).await;

    assert_eq!(failure.kind, SocketProcessingFailureKind::ProcessingTimeout);
}

#[tokio::test]
async fn encode_transport_failure_is_encode_failed() {
    let failure = process_failure(RpcBehavior::EncodeFailure).await;

    assert_eq!(failure.kind, SocketProcessingFailureKind::EncodeFailed);
}

#[tokio::test]
async fn schema_incompatible_request_is_decode_failed() {
    let failure = process_failure(RpcBehavior::InvalidDocument).await;

    assert_eq!(failure.kind, SocketProcessingFailureKind::DecodeFailed);
}

#[tokio::test]
async fn invalid_encoded_base64_is_encode_failed() {
    let failure = process_failure(RpcBehavior::InvalidBytes).await;

    assert_eq!(failure.kind, SocketProcessingFailureKind::EncodeFailed);
}

#[tokio::test]
async fn uncommitted_response_rejects_the_next_exchange() {
    let factory = factory(RpcBehavior::Success);
    let mut processor = factory.create_exchange(connection());
    processor
        .process(Bytes::from_static(b"first"))
        .await
        .expect("first exchange");

    let failure = processor
        .process(Bytes::from_static(b"second"))
        .await
        .expect_err("pending response must be committed before another exchange");

    assert_eq!(failure.kind, SocketProcessingFailureKind::ProcessingFailed);
}

#[test]
fn commit_without_pending_response_is_a_noop() {
    let factory = factory(RpcBehavior::Success);
    let mut processor = factory.create_exchange(connection());

    processor.output_committed();
}

#[test]
fn failed_output_without_pending_response_is_a_noop() {
    let factory = factory(RpcBehavior::Success);
    let mut processor = factory.create_exchange(connection());

    processor.output_failed(
        &SocketProcessingFailure::new(SocketProcessingFailureKind::WriteFailed, "write failed"),
        0,
    );
}

#[test]
fn output_failure_stage_classifies_write_failures() {
    for kind in [
        SocketProcessingFailureKind::WriteFailed,
        SocketProcessingFailureKind::WriteTimeout,
        SocketProcessingFailureKind::Cancelled,
    ] {
        assert_eq!(
            failure_stage(kind),
            SocketLocalExchangeFailureStage::ResponseWrite
        );
    }
}

#[test]
fn output_failure_stage_classifies_rule_failure() {
    assert_eq!(
        failure_stage(SocketProcessingFailureKind::RuleFailed),
        SocketLocalExchangeFailureStage::ResponseRule
    );
}

#[test]
fn output_failure_stage_defaults_to_encode_failure() {
    assert_eq!(
        failure_stage(SocketProcessingFailureKind::EncodeFailed),
        SocketLocalExchangeFailureStage::ResponseEncode
    );
}

async fn process_failure(behavior: RpcBehavior) -> SocketProcessingFailure {
    let factory = factory(behavior);
    let mut processor = factory.create_exchange(connection());
    processor
        .process(Bytes::from_static(b"sale"))
        .await
        .expect_err("configured failure")
}

fn factory(behavior: RpcBehavior) -> ExternalLocalResponderProcessorFactoryAdapter {
    let registration = registration();
    ExternalLocalResponderProcessorFactoryAdapter::new(
        Arc::new(ExternalSocketRuntimeSnapshot::new(
            ExternalSocketPackageBinding::new(
                registration.clone(),
                Arc::new(BehaviorRpc(behavior)),
            ),
            rules(&registration),
            SocketTopology::default(),
        )),
        SocketCaptureContext {
            workspace_id: intercept_proxy_domain::WorkspaceId::new(),
            listener_id: listener_id(),
            publisher: None,
        },
    )
}

fn connection() -> SocketConnectionIdentity {
    SocketConnectionIdentity {
        runtime_epoch: Uuid::from_u128(11),
        connection_id: Uuid::from_u128(12),
        peer_addr: "127.0.0.1:12345".parse().expect("socket address"),
    }
}
