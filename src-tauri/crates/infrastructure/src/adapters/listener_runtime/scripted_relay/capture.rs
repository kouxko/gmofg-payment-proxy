//! Relay Frame 在 write + flush 后的正式 capture 映射。

use chrono::{DateTime, Utc};
use intercept_proxy_application::{
    SocketCaptureDocument, SocketCapturePayload, SocketCaptureSchemaRef, SocketRelayFrameCapture,
    SocketWriteKind,
};
use intercept_proxy_domain::{ProtocolPackageRef, SocketDirection, SocketDocumentRuleId};
use intercept_proxy_protocol_scripting::{DisplayFallbackReason, ProtocolDisplayResult};
use intercept_proxy_protocol_scripting::{ProtocolDirectionExecutor, ProtocolFrameOutput};
use intercept_proxy_runtime::SocketConnectionIdentity;

use super::super::socket_capture_publisher::{
    SocketCaptureContext, SocketCapturePublishTicket, capture_display, capture_resource_busy,
};

pub(super) struct PendingRelayCapture {
    pub(super) output: ProtocolFrameOutput,
    pub(super) matched_rule_ids: Vec<SocketDocumentRuleId>,
    occurred_at: DateTime<Utc>,
}

impl PendingRelayCapture {
    pub(super) fn new(
        output: ProtocolFrameOutput,
        matched_rule_ids: Vec<SocketDocumentRuleId>,
        occurred_at: DateTime<Utc>,
    ) -> Self {
        Self {
            output,
            matched_rule_ids,
            occurred_at,
        }
    }
}

#[allow(clippy::too_many_arguments)]
pub(super) fn commit(
    executor: Option<&mut ProtocolDirectionExecutor>,
    pending: PendingRelayCapture,
    ticket: Option<SocketCapturePublishTicket>,
    capture: &SocketCaptureContext,
    connection: &SocketConnectionIdentity,
    completed_at: DateTime<Utc>,
    direction: SocketDirection,
    package: ProtocolPackageRef,
    schema: SocketCaptureSchemaRef,
    decode_enabled: bool,
    encode_enabled: bool,
) {
    #[cfg(test)]
    capture.wait_before_display();
    let display = executor.map_or_else(capture_resource_busy, |executor| {
        capture_display(
            std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                executor.render_display(&pending.output)
            }))
            .unwrap_or(ProtocolDisplayResult::HexFallback(
                DisplayFallbackReason::EntryPointFailed,
            )),
        )
    });
    let document = pending
        .output
        .decoded_document()
        .map(SocketCaptureDocument::from_document);
    capture.record(
        ticket,
        connection,
        pending.occurred_at,
        completed_at,
        SocketCapturePayload::RelayFrame(SocketRelayFrameCapture {
            direction,
            package,
            schema,
            decode_enabled,
            encode_enabled,
            origin: pending.output.origin().to_vec(),
            document,
            matched_rule_ids: pending.matched_rule_ids,
            written: pending.output.written().to_vec(),
            write_kind: if encode_enabled {
                SocketWriteKind::Encoded
            } else {
                SocketWriteKind::Original
            },
            display,
        }),
    );
}
