//! Relay Frame 在 write + flush 后的正式 capture 映射。

use chrono::{DateTime, Utc};
use intercept_proxy_application::{
    SocketCapturePayload, SocketCaptureSchemaRef, SocketRelayFrameCapture,
    SocketRelayRuleStageCapture,
};
use intercept_proxy_domain::{ProtocolDirection, ProtocolPackageRef};
use intercept_proxy_protocol_scripting::{DisplayFallbackReason, ProtocolDisplayResult};
use intercept_proxy_protocol_scripting::{ProtocolDirectionExecutor, ProtocolFrameOutput};
use intercept_proxy_runtime::SocketConnectionIdentity;

use super::super::socket_capture_publisher::{
    SocketCaptureContext, SocketCapturePublishTicket, capture_display, capture_resource_busy,
};

pub(super) struct PendingRelayCapture {
    pub(super) output: ProtocolFrameOutput,
    pub(super) stages: Vec<SocketRelayRuleStageCapture>,
    occurred_at: DateTime<Utc>,
}

pub(super) struct RelayCaptureCommit<'a> {
    pub(super) ticket: Option<SocketCapturePublishTicket>,
    pub(super) capture: &'a SocketCaptureContext,
    pub(super) connection: &'a SocketConnectionIdentity,
    pub(super) completed_at: DateTime<Utc>,
    pub(super) direction: ProtocolDirection,
    pub(super) package: ProtocolPackageRef,
    pub(super) schema: SocketCaptureSchemaRef,
}

impl PendingRelayCapture {
    pub(super) fn new(
        output: ProtocolFrameOutput,
        stages: Vec<SocketRelayRuleStageCapture>,
        occurred_at: DateTime<Utc>,
    ) -> Self {
        Self {
            output,
            stages,
            occurred_at,
        }
    }
}

pub(super) fn commit(
    executor: Option<&mut ProtocolDirectionExecutor>,
    pending: PendingRelayCapture,
    commit: RelayCaptureCommit<'_>,
) {
    #[cfg(test)]
    commit.capture.wait_before_display();
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
    commit.capture.record(
        commit.ticket,
        commit.connection,
        pending.occurred_at,
        commit.completed_at,
        SocketCapturePayload::RelayFrame(Box::new(SocketRelayFrameCapture {
            direction: commit.direction,
            package: commit.package,
            schema: commit.schema,
            origin: pending.output.origin().to_vec(),
            stages: pending.stages,
            written: pending.output.written().to_vec(),
            display,
        })),
    );
}
