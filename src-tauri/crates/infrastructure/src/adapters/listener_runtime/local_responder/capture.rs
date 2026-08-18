//! `LocalResponder` response 在 write + flush 后的原子 exchange capture。

use chrono::{DateTime, Utc};
use intercept_proxy_application::{
    SocketCaptureDocument, SocketCapturePayload, SocketCaptureSchemaRef, SocketExchangeId,
    SocketLocalExchangeCapture,
};
use intercept_proxy_domain::{ProtocolDocumentRuleId, ProtocolPackageRef};
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
    pub(super) matched_request_rule_ids: Vec<ProtocolDocumentRuleId>,
    pub(super) matched_response_rule_ids: Vec<ProtocolDocumentRuleId>,
    occurred_at: DateTime<Utc>,
}

pub(super) struct LocalCaptureCommit<'a> {
    pub(super) ticket: Option<SocketCapturePublishTicket>,
    pub(super) capture: &'a SocketCaptureContext,
    pub(super) connection: &'a SocketConnectionIdentity,
    pub(super) completed_at: DateTime<Utc>,
    pub(super) package: ProtocolPackageRef,
    pub(super) request_schema: SocketCaptureSchemaRef,
    pub(super) response_schema: SocketCaptureSchemaRef,
    pub(super) render_display: bool,
}

impl PendingLocalCapture {
    pub(super) fn new(
        response: LocalResponseOutput,
        exchange_id: SocketExchangeId,
        request: LocalRequestOutput,
        matched_request_rule_ids: Vec<ProtocolDocumentRuleId>,
        matched_response_rule_ids: Vec<ProtocolDocumentRuleId>,
        occurred_at: DateTime<Utc>,
    ) -> Self {
        Self {
            response,
            exchange_id,
            request,
            matched_request_rule_ids,
            matched_response_rule_ids,
            occurred_at,
        }
    }
}

pub(super) fn commit(
    coordinator: &mut LocalResponderCoordinator,
    pending: PendingLocalCapture,
    commit: LocalCaptureCommit<'_>,
) {
    #[cfg(test)]
    commit.capture.wait_before_display();
    let request_display = capture_display(
        std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            coordinator.render_request_display(&pending.request)
        }))
        .ok()
        .and_then(Result::ok)
        .unwrap_or_else(entry_fallback),
    );
    let handle = coordinator.response_committed(&pending.response).ok();
    let display = if commit.render_display {
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
    commit.capture.record(
        commit.ticket,
        commit.connection,
        pending.occurred_at,
        commit.completed_at,
        SocketCapturePayload::LocalExchange(Box::new(SocketLocalExchangeCapture {
            exchange_id: pending.exchange_id,
            package: commit.package,
            request_schema: commit.request_schema,
            response_schema: commit.response_schema,
            request_origin: pending.request.origin().to_vec(),
            request_document: SocketCaptureDocument::from_document(pending.request.document()),
            request_display,
            response_document: SocketCaptureDocument::from_document(
                pending.response.response_document(),
            ),
            matched_request_rule_ids: pending.matched_request_rule_ids,
            matched_response_rule_ids: pending.matched_response_rule_ids,
            written_response: pending.response.written().to_vec(),
            response_display: display,
        })),
    );
}

fn entry_fallback() -> ProtocolDisplayResult {
    ProtocolDisplayResult::HexFallback(DisplayFallbackReason::EntryPointFailed)
}
