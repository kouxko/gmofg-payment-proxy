//! `LocalResponder` response 在 write + flush 后的原子 exchange capture。

use chrono::{DateTime, Utc};
use intercept_proxy_application::{
    SocketCaptureDocument, SocketCapturePayload, SocketCaptureSchemaRef, SocketExchangeId,
    SocketLocalExchangeCapture, SocketLocalExchangeFailureCapture, SocketLocalExchangeFailureStage,
};
use intercept_proxy_domain::{ProtocolDocumentRuleId, ProtocolPackageRef};
use intercept_proxy_protocol_scripting::{
    DisplayFallbackReason, LocalRequestOutput, LocalResponderCoordinator, LocalResponseOutput,
    ProtocolDisplayResult,
};
use intercept_proxy_runtime::SocketConnectionIdentity;
use intercept_proxy_runtime::SocketProcessingFailureKind;

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

pub(super) struct LocalCaptureFailure<'a> {
    pub(super) ticket: Option<SocketCapturePublishTicket>,
    pub(super) capture: &'a SocketCaptureContext,
    pub(super) connection: &'a SocketConnectionIdentity,
    pub(super) completed_at: DateTime<Utc>,
    pub(super) package: ProtocolPackageRef,
    pub(super) request_schema: SocketCaptureSchemaRef,
    pub(super) response_schema: SocketCaptureSchemaRef,
    pub(super) failure_kind: SocketProcessingFailureKind,
    pub(super) written_bytes: usize,
    pub(super) render_display: bool,
}

pub(super) struct FailedResponseBuild {
    pub(super) exchange_id: SocketExchangeId,
    pub(super) request: LocalRequestOutput,
    pub(super) response_document: Option<SocketCaptureDocument>,
    pub(super) matched_request_rule_ids: Vec<ProtocolDocumentRuleId>,
    pub(super) matched_response_rule_ids: Vec<ProtocolDocumentRuleId>,
    pub(super) occurred_at: DateTime<Utc>,
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

pub(super) fn fail_response_build(
    coordinator: &mut LocalResponderCoordinator,
    failed: FailedResponseBuild,
    failure: LocalCaptureFailure<'_>,
) {
    record_failure(
        coordinator,
        failed.exchange_id,
        &failed.request,
        failed.response_document,
        failed.matched_request_rule_ids,
        failed.matched_response_rule_ids,
        failed.occurred_at,
        Vec::new(),
        failure,
    );
}

pub(super) fn fail_output(
    coordinator: &mut LocalResponderCoordinator,
    pending: PendingLocalCapture,
    failure: LocalCaptureFailure<'_>,
) {
    let prefix_length = failure.written_bytes.min(pending.response.written().len());
    let prefix = pending.response.written()[..prefix_length].to_vec();
    let response_document = Some(SocketCaptureDocument::from_document(
        pending.response.response_document(),
    ));
    record_failure(
        coordinator,
        pending.exchange_id,
        &pending.request,
        response_document,
        pending.matched_request_rule_ids,
        pending.matched_response_rule_ids,
        pending.occurred_at,
        prefix,
        failure,
    );
}

#[expect(
    clippy::too_many_arguments,
    reason = "capture evidence is explicit at the persistence boundary"
)]
fn record_failure(
    coordinator: &mut LocalResponderCoordinator,
    exchange_id: SocketExchangeId,
    request: &LocalRequestOutput,
    response_document: Option<SocketCaptureDocument>,
    matched_request_rule_ids: Vec<ProtocolDocumentRuleId>,
    matched_response_rule_ids: Vec<ProtocolDocumentRuleId>,
    occurred_at: DateTime<Utc>,
    written_response_prefix: Vec<u8>,
    failure: LocalCaptureFailure<'_>,
) {
    let request_display = if failure.render_display {
        capture_display(
            std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                coordinator.render_request_display(request)
            }))
            .ok()
            .and_then(Result::ok)
            .unwrap_or_else(entry_fallback),
        )
    } else {
        capture_resource_busy()
    };
    let failure_stage = failure_stage(failure.failure_kind);
    failure.capture.record(
        failure.ticket,
        failure.connection,
        occurred_at,
        failure.completed_at,
        SocketCapturePayload::LocalExchangeFailure(Box::new(SocketLocalExchangeFailureCapture {
            exchange_id,
            package: failure.package,
            request_schema: failure.request_schema,
            response_schema: failure.response_schema,
            request_origin: request.origin().to_vec(),
            request_document: SocketCaptureDocument::from_document(request.document()),
            request_display,
            matched_request_rule_ids,
            matched_response_rule_ids,
            response_document,
            failure_stage,
            failure_code: failure.failure_kind.as_str().to_owned(),
            failure_message: failure_stage.stable_message().to_owned(),
            written_response_prefix,
        })),
    );
}

const fn failure_stage(kind: SocketProcessingFailureKind) -> SocketLocalExchangeFailureStage {
    match kind {
        SocketProcessingFailureKind::RuleFailed => SocketLocalExchangeFailureStage::ResponseRule,
        SocketProcessingFailureKind::WriteFailed
        | SocketProcessingFailureKind::WriteTimeout
        | SocketProcessingFailureKind::Cancelled => SocketLocalExchangeFailureStage::ResponseWrite,
        _ => SocketLocalExchangeFailureStage::ResponseEncode,
    }
}

fn entry_fallback() -> ProtocolDisplayResult {
    ProtocolDisplayResult::HexFallback(DisplayFallbackReason::EntryPointFailed)
}
