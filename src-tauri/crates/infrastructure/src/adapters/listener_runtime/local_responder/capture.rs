//! `LocalResponder` response 在 write + flush 后的原子 exchange capture。

use chrono::{DateTime, Utc};
use intercept_proxy_application::{
    SocketCaptureDocument, SocketCapturePayload, SocketCaptureSchemaRef, SocketExchangeId,
    SocketLocalExchangeCapture, SocketWriteKind,
};
use intercept_proxy_domain::{ProtocolPackageRef, SocketDocumentRuleId};
use intercept_proxy_protocol_scripting::{
    DisplayFallbackReason, LocalRequestOutput, LocalResponderCoordinator, LocalResponseOutput,
    ProtocolDisplayResult,
};
use intercept_proxy_runtime::SocketConnectionIdentity;

use super::super::socket_capture_publisher::{
    SocketCaptureContext, SocketCapturePublishTicket, capture_display, capture_resource_busy,
};

pub(super) struct PendingLocalCapture {
    pub(super) response: LocalResponseOutput,
    pub(super) exchange_id: SocketExchangeId,
    pub(super) request: LocalRequestOutput,
    pub(super) matched_rule_ids: Vec<SocketDocumentRuleId>,
    occurred_at: DateTime<Utc>,
}

impl PendingLocalCapture {
    pub(super) fn new(
        response: LocalResponseOutput,
        exchange_id: SocketExchangeId,
        request: LocalRequestOutput,
        matched_rule_ids: Vec<SocketDocumentRuleId>,
        occurred_at: DateTime<Utc>,
    ) -> Self {
        Self {
            response,
            exchange_id,
            request,
            matched_rule_ids,
            occurred_at,
        }
    }
}

#[allow(clippy::too_many_arguments)]
pub(super) fn commit(
    coordinator: &mut LocalResponderCoordinator,
    pending: PendingLocalCapture,
    ticket: Option<SocketCapturePublishTicket>,
    capture: &SocketCaptureContext,
    connection: &SocketConnectionIdentity,
    completed_at: DateTime<Utc>,
    package: ProtocolPackageRef,
    schema: SocketCaptureSchemaRef,
    request_decode_enabled: bool,
    response_encode_enabled: bool,
    render_display: bool,
) {
    #[cfg(test)]
    capture.wait_before_display();
    let request_display = pending.request.document().map(|_| {
        capture_display(
            std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                coordinator.render_request_display(&pending.request)
            }))
            .ok()
            .and_then(Result::ok)
            .unwrap_or_else(entry_fallback),
        )
    });
    let handle = coordinator.response_committed(&pending.response).ok();
    let display = if render_display {
        capture_display(handle.as_ref().map_or_else(entry_fallback, |handle| {
            std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                coordinator.render_response_display(handle)
            }))
            .ok()
            .and_then(Result::ok)
            .unwrap_or_else(entry_fallback)
        }))
    } else {
        capture_resource_busy()
    };
    capture.record(
        ticket,
        connection,
        pending.occurred_at,
        completed_at,
        SocketCapturePayload::LocalExchange(SocketLocalExchangeCapture {
            exchange_id: pending.exchange_id,
            package,
            schema,
            request_decode_enabled,
            response_encode_enabled,
            request_origin: pending.request.origin().to_vec(),
            request_document: pending
                .request
                .document()
                .map(SocketCaptureDocument::from_document),
            request_display,
            response_document: SocketCaptureDocument::from_document(
                pending.response.response_document(),
            ),
            matched_downstream_rule_ids: pending.matched_rule_ids,
            written_response: pending.response.written().to_vec(),
            response_write_kind: if response_encode_enabled {
                SocketWriteKind::Encoded
            } else {
                SocketWriteKind::Original
            },
            response_display: display,
        }),
    );
}

fn entry_fallback() -> ProtocolDisplayResult {
    ProtocolDisplayResult::HexFallback(DisplayFallbackReason::EntryPointFailed)
}
