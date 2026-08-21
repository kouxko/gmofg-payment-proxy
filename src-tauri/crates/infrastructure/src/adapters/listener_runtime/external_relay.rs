//! 外部软件包驱动的 Socket Relay 数据面。
//!
//! 本模块只替换协议入口（Frame、Decode、Encode、Display）的执行位置。四阶段 Document
//! 规则、Frame Pump、线路写入及 capture 提交语义继续复用既有实现，避免形成第二套规则引擎。

use std::{future::Future, sync::Arc};

use async_trait::async_trait;
use bytes::Bytes;
use chrono::{DateTime, Utc};
use intercept_proxy_application::{
    ExternalPackageCallStage, SocketCaptureDocument, SocketCapturePayload, SocketCaptureSchemaRef,
    SocketDisplayDiagnostic, SocketDisplayFallbackReason, SocketDisplayResult,
    SocketRelayFrameCapture, SocketRelayRuleStageCapture,
};
use intercept_proxy_domain::{
    ExternalDecodeRequest, ExternalDisplayRequest, ExternalDocumentWire, ExternalEncodeRequest,
    ExternalFrameRequest, ExternalFrameResult, ExternalPackageDirection,
    ExternalPackageMethodNamespace, ProtocolDirection, ProtocolPackageRef, ProtocolRuleStage,
};
use intercept_proxy_runtime::{
    FrameBoundary, ScriptedRelayProcessorFactory, SocketConnectionIdentity, SocketFrameProcessor,
    SocketPayloadDirection, SocketProcessingFailure, SocketProcessingFailureKind,
};
use parking_lot::Mutex;
use tokio::sync::oneshot;

use super::{
    ProtocolDocumentRuleConnection, ProtocolDocumentRuleConnectionFactory,
    socket_capture_publisher::SocketCaptureContext,
};
use crate::adapters::external_packages::ExternalPackageConnectionError;

pub(super) mod contract;
mod diagnostics;
pub(crate) use contract::ExternalSocketPackageBinding as RuntimeExternalSocketPackageBinding;
pub(crate) use contract::ExternalSocketPackageProvider;
pub(crate) use contract::ExternalSocketRuntimeSnapshot;
use contract::{ExternalPackageRpc, ExternalSocketPackageBinding};
#[cfg(test)]
use diagnostics::redacted_data_summary;
pub(super) use diagnostics::trace_external_rpc_failure;

/// 同一次 Listener 启动快照派生的外部双方向 processor factory。
pub(super) struct ExternalRelayProcessorFactoryAdapter {
    binding: ExternalSocketPackageBinding,
    rules: ProtocolDocumentRuleConnectionFactory,
    capture: SocketCaptureContext,
}

impl ExternalRelayProcessorFactoryAdapter {
    pub(super) fn new(
        snapshot: &ExternalSocketRuntimeSnapshot,
        capture: SocketCaptureContext,
    ) -> Self {
        Self {
            binding: snapshot.binding.clone(),
            rules: snapshot.rules.clone(),
            capture,
        }
    }

    fn build_processor(
        &self,
        connection: SocketConnectionIdentity,
        direction: SocketPayloadDirection,
    ) -> Result<ExternalRelayFrameProcessor, SocketProcessingFailure> {
        let registration = &self.binding.registration;
        let package = registration.package().identity().clone();
        let (external_direction, protocol_direction, first_stage, second_stage, hooks, document) =
            match direction {
                SocketPayloadDirection::AppToUpstream => (
                    ExternalPackageDirection::Upstream,
                    ProtocolDirection::Upstream,
                    ProtocolRuleStage::AppToProxy,
                    ProtocolRuleStage::ProxyToUpstream,
                    registration.hooks().upstream(),
                    registration.document().upstream(),
                ),
                SocketPayloadDirection::UpstreamToApp => (
                    ExternalPackageDirection::Downstream,
                    ProtocolDirection::Downstream,
                    ProtocolRuleStage::UpstreamToProxy,
                    ProtocolRuleStage::ProxyToApp,
                    registration.hooks().downstream(),
                    registration.document().downstream(),
                ),
                SocketPayloadDirection::LocalExchange => {
                    return Err(processing_failure(
                        "external package does not support LocalExchange",
                    ));
                }
            };
        Ok(ExternalRelayFrameProcessor {
            rpc: Arc::clone(&self.binding.rpc),
            package,
            schema: document.schema().clone(),
            methods: ExternalDirectionMethods {
                frame: hooks
                    .frame()
                    .qualified(ExternalPackageMethodNamespace::Hooks, external_direction),
                decode: hooks
                    .decode()
                    .qualified(ExternalPackageMethodNamespace::Hooks, external_direction),
                encode: hooks
                    .encode()
                    .qualified(ExternalPackageMethodNamespace::Hooks, external_direction),
                display: document
                    .display()
                    .qualified(ExternalPackageMethodNamespace::Document, external_direction),
            },
            first_rules: self.rules.connection(connection.clone(), first_stage),
            second_rules: self.rules.connection(connection.clone(), second_stage),
            pending: None,
            connection,
            direction: protocol_direction,
            capture: self.capture.clone(),
            display_lane: Arc::new(OrderedTaskLane::default()),
        })
    }
}

impl ScriptedRelayProcessorFactory for ExternalRelayProcessorFactoryAdapter {
    fn create_direction(
        &self,
        connection: SocketConnectionIdentity,
        direction: SocketPayloadDirection,
    ) -> Box<dyn SocketFrameProcessor> {
        match self.build_processor(connection, direction) {
            Ok(processor) => Box::new(processor),
            Err(failure) => Box::new(FailedExternalFrameProcessor { failure }),
        }
    }
}

struct ExternalDirectionMethods {
    frame: String,
    decode: String,
    encode: String,
    display: String,
}

struct PendingExternalCapture {
    origin: Vec<u8>,
    written: Vec<u8>,
    document: ExternalDocumentWire,
    stages: Vec<SocketRelayRuleStageCapture>,
    occurred_at: DateTime<Utc>,
}

struct ExternalRelayFrameProcessor {
    rpc: Arc<dyn ExternalPackageRpc>,
    package: ProtocolPackageRef,
    schema: intercept_proxy_domain::DocumentSchema,
    methods: ExternalDirectionMethods,
    first_rules: ProtocolDocumentRuleConnection,
    second_rules: ProtocolDocumentRuleConnection,
    pending: Option<PendingExternalCapture>,
    connection: SocketConnectionIdentity,
    direction: ProtocolDirection,
    capture: SocketCaptureContext,
    display_lane: Arc<OrderedTaskLane>,
}

/// 以同步入队点确定顺序、在后台逐项执行的连接级任务 lane。
///
/// `output_committed` 不能等待 Display，否则旁路渲染会反向阻塞数据面；但直接 `spawn`
/// 又会让后续帧越序。本类型用前驱完成信号把后台任务串成 FIFO 链，并由任务自身持有 lane，
/// 确保最后一个 processor 被释放后，已接纳的 capture 仍能完成。
#[derive(Default)]
pub(super) struct OrderedTaskLane {
    tail: Mutex<Option<oneshot::Receiver<()>>>,
}

impl OrderedTaskLane {
    pub(super) fn spawn<F>(self: &Arc<Self>, task: F)
    where
        F: Future<Output = ()> + Send + 'static,
    {
        let (completed_tx, completed_rx) = oneshot::channel();
        let predecessor = self.tail.lock().replace(completed_rx);
        let keep_alive = Arc::clone(self);
        std::mem::drop(tokio::spawn(async move {
            if let Some(predecessor) = predecessor {
                let _ = predecessor.await;
            }
            task.await;
            let _ = completed_tx.send(());
            drop(keep_alive);
        }));
    }
}

#[async_trait]
impl SocketFrameProcessor for ExternalRelayFrameProcessor {
    async fn inspect(&mut self, buffered: Bytes) -> Result<FrameBoundary, SocketProcessingFailure> {
        let result = self
            .rpc
            .frame(
                &self.methods.frame,
                &ExternalFrameRequest::from_bytes(&buffered),
            )
            .await
            .map_err(|error| {
                rpc_failure(
                    ExternalCallStage::Frame,
                    &self.methods.frame,
                    &error,
                    &self.package,
                    &self.connection,
                    self.direction,
                    &self.capture,
                )
            })?;
        Ok(external_frame_boundary(&result))
    }

    async fn process(&mut self, origin: Bytes) -> Result<Bytes, SocketProcessingFailure> {
        if self.pending.is_some() {
            return Err(processing_failure(
                "previous external frame output was not committed",
            ));
        }
        let occurred_at = Utc::now();
        let decoded = self
            .rpc
            .decode(
                &self.methods.decode,
                &ExternalDecodeRequest::from_bytes(&origin),
            )
            .await
            .map_err(|error| {
                rpc_failure(
                    ExternalCallStage::Decode,
                    &self.methods.decode,
                    &error,
                    &self.package,
                    &self.connection,
                    self.direction,
                    &self.capture,
                )
            })?;
        let document = decoded
            .document
            .into_document(&self.schema)
            .map_err(|_| phase_failure(SocketProcessingFailureKind::DecodeFailed))?;
        let mut stages = Vec::with_capacity(2);
        let first = self
            .first_rules
            .execute(self.first_rules.bind_document(document))
            .map_err(|_| phase_failure(SocketProcessingFailureKind::RuleFailed))?;
        let (document, first_ids) = first.into_parts();
        stages.push(SocketRelayRuleStageCapture {
            stage: self.first_rules.stage(),
            matched_rule_ids: first_ids,
            document: SocketCaptureDocument::from_document(&document),
        });
        let second = self
            .second_rules
            .execute(self.second_rules.bind_document(document))
            .map_err(|_| phase_failure(SocketProcessingFailureKind::RuleFailed))?;
        let (document, second_ids) = second.into_parts();
        stages.push(SocketRelayRuleStageCapture {
            stage: self.second_rules.stage(),
            matched_rule_ids: second_ids,
            document: SocketCaptureDocument::from_document(&document),
        });
        let document = ExternalDocumentWire::from_document(&document);
        let encoded = self
            .rpc
            .encode(
                &self.methods.encode,
                &ExternalEncodeRequest {
                    document: document.clone(),
                },
            )
            .await
            .map_err(|error| {
                rpc_failure(
                    ExternalCallStage::Encode,
                    &self.methods.encode,
                    &error,
                    &self.package,
                    &self.connection,
                    self.direction,
                    &self.capture,
                )
            })?;
        let written = encoded
            .bytes()
            .map_err(|_| phase_failure(SocketProcessingFailureKind::EncodeFailed))?;
        self.pending = Some(PendingExternalCapture {
            origin: origin.to_vec(),
            written: written.clone(),
            document,
            stages,
            occurred_at,
        });
        Ok(Bytes::from(written))
    }

    fn output_committed(&mut self) {
        let Some(pending) = self.pending.take() else {
            return;
        };
        let rpc = Arc::clone(&self.rpc);
        let display_method = self.methods.display.clone();
        let capture = self.capture.clone();
        let ticket = capture.ticket();
        let connection = self.connection.clone();
        let package = self.package.clone();
        let direction = self.direction;
        let schema = SocketCaptureSchemaRef {
            id: self.schema.id().clone(),
            version: self.schema.version(),
        };
        // Display 明确位于 write + flush 之后。它是旁路观察任务，失败只产生 Hex fallback，
        // 不得反写已提交线路，也不得阻塞下一帧的 Frame/Decode/Encode。
        self.display_lane.spawn(async move {
            let display = match rpc
                .display(
                    &display_method,
                    &ExternalDisplayRequest {
                        document: pending.document,
                    },
                )
                .await
            {
                Ok(result) => SocketDisplayResult::UntrustedHtml { html: result.html },
                Err(error) => {
                    let diagnostic = trace_external_rpc_failure(
                        &package,
                        &connection,
                        direction,
                        ExternalPackageCallStage::Display,
                        &display_method,
                        &error,
                        &capture,
                    );
                    SocketDisplayResult::HexFallback {
                        reason: SocketDisplayFallbackReason::EntryPointFailed,
                        diagnostic: Some(SocketDisplayDiagnostic {
                            code: "DISPLAY_ENTRY_FAILED".to_owned(),
                            message: "Display 执行失败，已回退 Hex。".to_owned(),
                            external_package_call: Some(diagnostic),
                        }),
                    }
                }
            };
            capture.record(
                ticket,
                &connection,
                pending.occurred_at,
                Utc::now(),
                SocketCapturePayload::RelayFrame(Box::new(SocketRelayFrameCapture {
                    direction,
                    package,
                    schema,
                    origin: pending.origin,
                    stages: pending.stages,
                    written: pending.written,
                    display,
                })),
            );
        });
    }
}

struct FailedExternalFrameProcessor {
    failure: SocketProcessingFailure,
}

#[async_trait]
impl SocketFrameProcessor for FailedExternalFrameProcessor {
    async fn inspect(
        &mut self,
        _buffered: Bytes,
    ) -> Result<FrameBoundary, SocketProcessingFailure> {
        Err(self.failure.clone())
    }

    async fn process(&mut self, _origin: Bytes) -> Result<Bytes, SocketProcessingFailure> {
        Err(self.failure.clone())
    }
}

#[derive(Clone, Copy)]
enum ExternalCallStage {
    Frame,
    Decode,
    Encode,
}

fn external_frame_boundary(result: &ExternalFrameResult) -> FrameBoundary {
    match result {
        ExternalFrameResult::NeedMore => FrameBoundary::NeedMoreUnknown,
        ExternalFrameResult::Complete { consumed_bytes } => FrameBoundary::Complete {
            bytes: *consumed_bytes,
        },
    }
}

fn rpc_failure(
    stage: ExternalCallStage,
    method: &str,
    error: &ExternalPackageConnectionError,
    package: &ProtocolPackageRef,
    connection: &SocketConnectionIdentity,
    direction: ProtocolDirection,
    capture: &SocketCaptureContext,
) -> SocketProcessingFailure {
    trace_external_rpc_failure(
        package,
        connection,
        direction,
        stage.diagnostic_stage(),
        method,
        error,
        capture,
    );
    let kind = match error {
        ExternalPackageConnectionError::Timeout { .. } => {
            SocketProcessingFailureKind::ProcessingTimeout
        }
        _ => match stage {
            ExternalCallStage::Decode => SocketProcessingFailureKind::DecodeFailed,
            ExternalCallStage::Encode => SocketProcessingFailureKind::EncodeFailed,
            ExternalCallStage::Frame => SocketProcessingFailureKind::ProcessingFailed,
        },
    };
    phase_failure(kind)
}

impl ExternalCallStage {
    const fn diagnostic_stage(self) -> ExternalPackageCallStage {
        match self {
            Self::Frame => ExternalPackageCallStage::Frame,
            Self::Decode => ExternalPackageCallStage::Decode,
            Self::Encode => ExternalPackageCallStage::Encode,
        }
    }
}

fn phase_failure(kind: SocketProcessingFailureKind) -> SocketProcessingFailure {
    SocketProcessingFailure::new(kind, "external package processing failed")
}

fn processing_failure(message: &'static str) -> SocketProcessingFailure {
    SocketProcessingFailure::new(SocketProcessingFailureKind::ProcessingFailed, message)
}

#[cfg(test)]
mod tests;
