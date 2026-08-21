//! 外部软件包驱动的 `LocalResponder` request-response 数据面。

use std::{
    collections::HashMap,
    sync::{Arc, Weak},
};

use async_trait::async_trait;
use bytes::Bytes;
use chrono::{DateTime, Utc};
use intercept_proxy_application::{
    ExternalPackageCallStage, SocketCaptureDocument, SocketCapturePayload, SocketCaptureSchemaRef,
    SocketDisplayDiagnostic, SocketDisplayFallbackReason, SocketDisplayResult, SocketExchangeId,
    SocketLocalExchangeCapture, SocketLocalExchangeFailureCapture, SocketLocalExchangeFailureStage,
};
use intercept_proxy_domain::{
    Document, ExternalDecodeRequest, ExternalDisplayRequest, ExternalDocumentWire,
    ExternalEncodeRequest, ExternalFrameRequest, ExternalFrameResult, ExternalPackageDirection,
    ExternalPackageMethodNamespace, ProtocolDirection, ProtocolRuleStage,
};
use intercept_proxy_runtime::{
    FrameBoundary, LocalResponderDiagnostics, LocalResponderProcessorFactory,
    SocketConnectionIdentity, SocketFrameProcessor, SocketProcessingFailure,
    SocketProcessingFailureKind,
};
use parking_lot::Mutex;
use uuid::Uuid;

use super::{
    ProtocolDocumentRuleConnection,
    external_relay::{ExternalSocketRuntimeSnapshot, OrderedTaskLane, trace_external_rpc_failure},
    local_responder::preview::publish_external_request_parsed,
    socket_capture_publisher::SocketCaptureContext,
};

pub(super) struct ExternalLocalResponderProcessorFactoryAdapter {
    snapshot: Arc<ExternalSocketRuntimeSnapshot>,
    capture: SocketCaptureContext,
    display_lanes: Mutex<HashMap<BusinessConnectionKey, Weak<OrderedTaskLane>>>,
}

impl ExternalLocalResponderProcessorFactoryAdapter {
    pub(super) fn new(
        snapshot: Arc<ExternalSocketRuntimeSnapshot>,
        capture: SocketCaptureContext,
    ) -> Self {
        Self {
            snapshot,
            capture,
            display_lanes: Mutex::new(HashMap::new()),
        }
    }

    fn display_lane(&self, connection: &SocketConnectionIdentity) -> Arc<OrderedTaskLane> {
        let key = BusinessConnectionKey {
            runtime_epoch: connection.runtime_epoch,
            connection_id: connection.connection_id,
        };
        let mut lanes = self.display_lanes.lock();
        // Factory 只保留弱引用：业务连接及其最后一个后台 capture 完成后，lane 可立即释放。
        lanes.retain(|_, lane| lane.strong_count() > 0);
        if let Some(lane) = lanes.get(&key).and_then(Weak::upgrade) {
            return lane;
        }
        let lane = Arc::new(OrderedTaskLane::default());
        lanes.insert(key, Arc::downgrade(&lane));
        lane
    }
}

#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
struct BusinessConnectionKey {
    runtime_epoch: Uuid,
    connection_id: Uuid,
}

impl LocalResponderProcessorFactory for ExternalLocalResponderProcessorFactoryAdapter {
    fn create_exchange(
        &self,
        connection: SocketConnectionIdentity,
    ) -> Box<dyn SocketFrameProcessor> {
        let display_lane = self.display_lane(&connection);
        let registration = self.snapshot.binding.registration();
        let upstream_hooks = registration.hooks().upstream();
        let downstream_hooks = registration.hooks().downstream();
        let methods = ExternalLocalMethods {
            frame: upstream_hooks.frame().qualified(
                ExternalPackageMethodNamespace::Hooks,
                ExternalPackageDirection::Upstream,
            ),
            decode: upstream_hooks.decode().qualified(
                ExternalPackageMethodNamespace::Hooks,
                ExternalPackageDirection::Upstream,
            ),
            encode: downstream_hooks.encode().qualified(
                ExternalPackageMethodNamespace::Hooks,
                ExternalPackageDirection::Downstream,
            ),
            request_display: registration.document().upstream().display().qualified(
                ExternalPackageMethodNamespace::Document,
                ExternalPackageDirection::Upstream,
            ),
            response_display: registration.document().downstream().display().qualified(
                ExternalPackageMethodNamespace::Document,
                ExternalPackageDirection::Downstream,
            ),
        };
        Box::new(ExternalLocalResponderProcessor {
            rpc: Arc::clone(&self.snapshot.binding.rpc),
            package: registration.package().identity().clone(),
            request_schema: registration.document().upstream().schema().clone(),
            response_schema: registration.document().downstream().schema().clone(),
            request_rules: self
                .snapshot
                .rules
                .connection(connection.clone(), ProtocolRuleStage::AppToProxy),
            response_rules: self
                .snapshot
                .rules
                .connection(connection.clone(), ProtocolRuleStage::ProxyToApp),
            methods,
            connection,
            capture: self.capture.clone(),
            diagnostics: None,
            pending: None,
            display_lane,
        })
    }
}

struct ExternalLocalMethods {
    frame: String,
    decode: String,
    encode: String,
    request_display: String,
    response_display: String,
}

struct PendingLocalCapture {
    exchange_id: SocketExchangeId,
    origin: Vec<u8>,
    request_document: SocketCaptureDocument,
    response_document: SocketCaptureDocument,
    request_wire: ExternalDocumentWire,
    response_wire: ExternalDocumentWire,
    matched_request_rule_ids: Vec<intercept_proxy_domain::ProtocolDocumentRuleId>,
    matched_response_rule_ids: Vec<intercept_proxy_domain::ProtocolDocumentRuleId>,
    written: Vec<u8>,
    occurred_at: DateTime<Utc>,
}

struct ExternalLocalResponderProcessor {
    rpc: Arc<dyn super::external_relay::contract::ExternalPackageRpc>,
    package: intercept_proxy_domain::ProtocolPackageRef,
    request_schema: intercept_proxy_domain::DocumentSchema,
    response_schema: intercept_proxy_domain::DocumentSchema,
    request_rules: ProtocolDocumentRuleConnection,
    response_rules: ProtocolDocumentRuleConnection,
    methods: ExternalLocalMethods,
    connection: SocketConnectionIdentity,
    capture: SocketCaptureContext,
    diagnostics: Option<LocalResponderDiagnostics>,
    pending: Option<PendingLocalCapture>,
    display_lane: Arc<OrderedTaskLane>,
}

#[async_trait]
impl SocketFrameProcessor for ExternalLocalResponderProcessor {
    async fn inspect(&mut self, buffered: Bytes) -> Result<FrameBoundary, SocketProcessingFailure> {
        let result = self
            .rpc
            .frame(
                &self.methods.frame,
                &ExternalFrameRequest::from_bytes(&buffered),
            )
            .await
            .map_err(|error| {
                self.rpc_failure(
                    ProtocolDirection::Upstream,
                    ExternalPackageCallStage::Frame,
                    &self.methods.frame,
                    &error,
                    SocketProcessingFailureKind::ProcessingFailed,
                )
            })?;
        Ok(match result {
            ExternalFrameResult::NeedMore => FrameBoundary::NeedMoreUnknown,
            ExternalFrameResult::Complete { consumed_bytes } => FrameBoundary::Complete {
                bytes: consumed_bytes,
            },
        })
    }

    async fn process(&mut self, origin: Bytes) -> Result<Bytes, SocketProcessingFailure> {
        if self.pending.is_some() {
            return Err(failure(SocketProcessingFailureKind::ProcessingFailed));
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
                self.rpc_failure(
                    ProtocolDirection::Upstream,
                    ExternalPackageCallStage::Decode,
                    &self.methods.decode,
                    &error,
                    SocketProcessingFailureKind::DecodeFailed,
                )
            })?;
        let request = decoded
            .document
            .into_document(&self.request_schema)
            .map_err(|_| failure(SocketProcessingFailureKind::DecodeFailed))?;
        let request = self
            .request_rules
            .execute(self.request_rules.bind_document(request))
            .map_err(|_| failure(SocketProcessingFailureKind::RuleFailed))?;
        let (request, matched_request_rule_ids) = request.into_parts();
        let exchange_id = SocketExchangeId::new();
        publish_external_request_parsed(
            self.diagnostics.as_ref(),
            exchange_id.as_uuid(),
            &origin,
            &request,
        );
        let response = self
            .response_rules
            .execute(
                self.response_rules
                    .bind_document(Document::new(self.response_schema.clone())),
            )
            .map_err(|_| failure(SocketProcessingFailureKind::RuleFailed))?;
        let (response, matched_response_rule_ids) = response.into_parts();
        let response_wire = ExternalDocumentWire::from_document(&response);
        let encoded = self
            .rpc
            .encode(
                &self.methods.encode,
                &ExternalEncodeRequest {
                    document: response_wire.clone(),
                },
            )
            .await
            .map_err(|error| {
                self.rpc_failure(
                    ProtocolDirection::Downstream,
                    ExternalPackageCallStage::Encode,
                    &self.methods.encode,
                    &error,
                    SocketProcessingFailureKind::EncodeFailed,
                )
            })?;
        let written = encoded
            .bytes()
            .map_err(|_| failure(SocketProcessingFailureKind::EncodeFailed))?;
        self.pending = Some(PendingLocalCapture {
            exchange_id,
            origin: origin.to_vec(),
            request_document: SocketCaptureDocument::from_document(&request),
            response_document: SocketCaptureDocument::from_document(&response),
            request_wire: ExternalDocumentWire::from_document(&request),
            response_wire,
            matched_request_rule_ids,
            matched_response_rule_ids,
            written: written.clone(),
            occurred_at,
        });
        Ok(Bytes::from(written))
    }

    fn set_local_diagnostics(&mut self, diagnostics: LocalResponderDiagnostics) {
        self.diagnostics = Some(diagnostics);
    }

    fn output_committed(&mut self) {
        let Some(pending) = self.pending.take() else {
            return;
        };
        self.spawn_capture(pending, None);
    }

    fn output_failed(&mut self, failure: &SocketProcessingFailure, written_bytes: usize) {
        let Some(pending) = self.pending.take() else {
            return;
        };
        self.spawn_capture(pending, Some((failure.kind, written_bytes)));
    }
}

impl ExternalLocalResponderProcessor {
    fn spawn_capture(
        &self,
        pending: PendingLocalCapture,
        output_failure: Option<(SocketProcessingFailureKind, usize)>,
    ) {
        let rpc = Arc::clone(&self.rpc);
        let request_method = self.methods.request_display.clone();
        let response_method = self.methods.response_display.clone();
        let capture = self.capture.clone();
        let ticket = capture.ticket();
        let connection = self.connection.clone();
        let package = self.package.clone();
        let request_schema = schema_ref(&self.request_schema);
        let response_schema = schema_ref(&self.response_schema);
        self.display_lane.spawn(async move {
            let request_display = display(
                &*rpc,
                &package,
                &connection,
                ProtocolDirection::Upstream,
                &request_method,
                pending.request_wire,
                &capture,
            )
            .await;
            let completed_at = Utc::now();
            let payload = if let Some((kind, written_bytes)) = output_failure {
                let stage = failure_stage(kind);
                SocketCapturePayload::LocalExchangeFailure(Box::new(
                    SocketLocalExchangeFailureCapture {
                        exchange_id: pending.exchange_id,
                        package,
                        request_schema,
                        response_schema,
                        request_origin: pending.origin,
                        request_document: pending.request_document,
                        request_display,
                        matched_request_rule_ids: pending.matched_request_rule_ids,
                        matched_response_rule_ids: pending.matched_response_rule_ids,
                        response_document: Some(pending.response_document),
                        failure_stage: stage,
                        failure_code: kind.as_str().to_owned(),
                        failure_message: stage.stable_message().to_owned(),
                        written_response_prefix: pending.written
                            [..written_bytes.min(pending.written.len())]
                            .to_vec(),
                    },
                ))
            } else {
                let response_display = display(
                    &*rpc,
                    &package,
                    &connection,
                    ProtocolDirection::Downstream,
                    &response_method,
                    pending.response_wire,
                    &capture,
                )
                .await;
                SocketCapturePayload::LocalExchange(Box::new(SocketLocalExchangeCapture {
                    exchange_id: pending.exchange_id,
                    package,
                    request_schema,
                    response_schema,
                    request_origin: pending.origin,
                    request_document: pending.request_document,
                    request_display,
                    response_document: pending.response_document,
                    matched_request_rule_ids: pending.matched_request_rule_ids,
                    matched_response_rule_ids: pending.matched_response_rule_ids,
                    written_response: pending.written,
                    response_display,
                }))
            };
            capture.record(
                ticket,
                &connection,
                pending.occurred_at,
                completed_at,
                payload,
            );
        });
    }

    fn rpc_failure(
        &self,
        direction: ProtocolDirection,
        stage: ExternalPackageCallStage,
        method: &str,
        error: &crate::adapters::external_packages::ExternalPackageConnectionError,
        fallback_kind: SocketProcessingFailureKind,
    ) -> SocketProcessingFailure {
        trace_external_rpc_failure(
            &self.package,
            &self.connection,
            direction,
            stage,
            method,
            error,
            &self.capture,
        );
        let kind = if matches!(
            error,
            crate::adapters::external_packages::ExternalPackageConnectionError::Timeout { .. }
        ) {
            SocketProcessingFailureKind::ProcessingTimeout
        } else {
            fallback_kind
        };
        failure(kind)
    }
}

async fn display(
    rpc: &dyn super::external_relay::contract::ExternalPackageRpc,
    package: &intercept_proxy_domain::ProtocolPackageRef,
    connection: &SocketConnectionIdentity,
    direction: ProtocolDirection,
    method: &str,
    document: ExternalDocumentWire,
    capture: &SocketCaptureContext,
) -> SocketDisplayResult {
    match rpc
        .display(method, &ExternalDisplayRequest { document })
        .await
    {
        Ok(result) => SocketDisplayResult::UntrustedHtml { html: result.html },
        Err(error) => {
            let diagnostic = trace_external_rpc_failure(
                package,
                connection,
                direction,
                ExternalPackageCallStage::Display,
                method,
                &error,
                capture,
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
    }
}

fn schema_ref(schema: &intercept_proxy_domain::DocumentSchema) -> SocketCaptureSchemaRef {
    SocketCaptureSchemaRef {
        id: schema.id().clone(),
        version: schema.version(),
    }
}
fn failure(kind: SocketProcessingFailureKind) -> SocketProcessingFailure {
    SocketProcessingFailure::new(kind, "external local responder processing failed")
}
const fn failure_stage(kind: SocketProcessingFailureKind) -> SocketLocalExchangeFailureStage {
    match kind {
        SocketProcessingFailureKind::WriteFailed
        | SocketProcessingFailureKind::WriteTimeout
        | SocketProcessingFailureKind::Cancelled => SocketLocalExchangeFailureStage::ResponseWrite,
        SocketProcessingFailureKind::RuleFailed => SocketLocalExchangeFailureStage::ResponseRule,
        _ => SocketLocalExchangeFailureStage::ResponseEncode,
    }
}

#[cfg(test)]
mod tests;
