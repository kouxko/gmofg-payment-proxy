//! 冻结协议包到 Proxy Scripted Relay processor 的连接级适配。

use std::{panic::AssertUnwindSafe, time::Duration};

use async_trait::async_trait;
use bytes::Bytes;
use intercept_proxy_domain::SocketDirection;
use intercept_proxy_protocol_scripting::{
    DirectionExecutionPlan, ProtocolDirection, ProtocolDirectionExecutor,
    ProtocolExecutionCancellation, ProtocolFrameInspection, ProtocolFrameInspector,
    ProtocolFrameOutput, ProtocolFramingError, ProtocolFramingLimits, ProtocolRuntimeError,
    ProtocolRuntimeLimits,
};
use intercept_proxy_runtime::{
    FrameBoundary, ScriptedRelayProcessorFactory, SocketConnectionIdentity, SocketFrameProcessor,
    SocketFramePumpLimits, SocketPayloadDirection, SocketProcessingFailure,
    SocketProcessingFailureKind,
};
use tokio::sync::{mpsc, oneshot};

use super::{
    SocketDocumentRuleConnection, SocketDocumentRuleConnectionFactory,
    scripted_snapshot::ScriptedSocketRuntimeSnapshot,
};
use crate::adapters::protocol_packages::runtime_snapshot::RuntimeProtocolPackageSnapshot;

use limiter::{BlockingCommandSlots, acquire_command_permits};

pub(super) mod limiter;

/// 同一次 Listener 启动快照派生的双方向 processor factory。
pub(super) struct ScriptedRelayProcessorFactoryAdapter {
    package: RuntimeProtocolPackageSnapshot,
    upstream: DirectionExecutionPlan,
    downstream: DirectionExecutionPlan,
    rules: SocketDocumentRuleConnectionFactory,
    listener_id: String,
    runtime_limits: ProtocolRuntimeLimits,
    framing_limits: ProtocolFramingLimits,
    blocking_slots: BlockingCommandSlots,
}

impl ScriptedRelayProcessorFactoryAdapter {
    pub(super) fn new(
        snapshot: &ScriptedSocketRuntimeSnapshot,
        listener_id: String,
        framing_limits: ProtocolFramingLimits,
    ) -> Self {
        Self {
            package: snapshot.package().clone(),
            upstream: snapshot.upstream(),
            downstream: snapshot.downstream(),
            rules: snapshot.rule_connections().clone(),
            listener_id,
            runtime_limits: snapshot.runtime_limits(),
            framing_limits,
            blocking_slots: BlockingCommandSlots::new_relay(snapshot.maximum_connections()),
        }
    }

    fn build_processor(
        &self,
        connection: SocketConnectionIdentity,
        direction: SocketPayloadDirection,
    ) -> Result<ScriptedRelayFrameProcessor, SocketProcessingFailure> {
        let (protocol_direction, rule_direction, plan) = match direction {
            SocketPayloadDirection::AppToUpstream => (
                ProtocolDirection::Upstream,
                SocketDirection::Upstream,
                self.upstream,
            ),
            SocketPayloadDirection::UpstreamToApp => (
                ProtocolDirection::Downstream,
                SocketDirection::Downstream,
                self.downstream,
            ),
            SocketPayloadDirection::LocalExchange => {
                return Err(processing_failure(
                    "LocalExchange cannot use the Scripted Relay factory",
                ));
            }
        };
        let connection_context =
            format!("{}:{}", connection.runtime_epoch, connection.connection_id);
        let cancellation = ProtocolExecutionCancellation::new();
        let inspector = ProtocolFrameInspector::new_with_cancellation(
            self.package.compiled(),
            protocol_direction,
            connection_context.clone(),
            self.listener_id.clone(),
            self.runtime_limits,
            self.framing_limits,
            cancellation.clone(),
        );
        let executor = ProtocolDirectionExecutor::new_with_cancellation(
            self.package.compiled(),
            plan,
            connection_context,
            self.listener_id.clone(),
            self.runtime_limits,
            cancellation.clone(),
        )
        .map_err(|_| processing_failure("protocol direction executor construction failed"))?;
        let rules = self.rules.connection(connection, rule_direction);
        Ok(ScriptedRelayFrameProcessor::spawn(
            DirectionWorkerState {
                inspector,
                executor,
                rules,
                package: self.package.compiled().package().clone(),
                pending_output: None,
                cancellation: cancellation.clone(),
            },
            self.blocking_slots.clone(),
            cancellation,
        ))
    }
}

pub(super) fn frame_pump_limits(
    runtime: ProtocolRuntimeLimits,
    framing: ProtocolFramingLimits,
    upstream: DirectionExecutionPlan,
    downstream: DirectionExecutionPlan,
) -> Result<SocketFramePumpLimits, SocketProcessingFailure> {
    let entry_calls = [upstream, downstream]
        .into_iter()
        .map(|plan| u64::from(plan.decode_enabled()) + u64::from(plan.encode_enabled()))
        .max()
        .unwrap_or(0)
        .max(1);
    frame_pump_limits_for_entry_calls(runtime, framing, entry_calls)
}

pub(super) fn frame_pump_limits_for_entry_calls(
    runtime: ProtocolRuntimeLimits,
    framing: ProtocolFramingLimits,
    entry_calls: u64,
) -> Result<SocketFramePumpLimits, SocketProcessingFailure> {
    const READ_CHUNK_BYTES: usize = 16 * 1024;

    let processing_ms =
        processing_budget_ms(runtime.max_wall_time_ms(), entry_calls).ok_or_else(invalid_limits)?;
    let max_buffer_bytes =
        usize::try_from(framing.max_fifo_bytes()).map_err(|_| invalid_limits())?;
    let max_output_bytes = usize::try_from(framing.max_frame_bytes().max(runtime.max_blob_bytes()))
        .map_err(|_| invalid_limits())?;
    SocketFramePumpLimits::new(
        max_buffer_bytes,
        max_output_bytes,
        READ_CHUNK_BYTES.min(max_buffer_bytes),
        Duration::from_millis(processing_ms),
    )
}

fn processing_budget_ms(max_wall_time_ms: u64, entry_calls: u64) -> Option<u64> {
    const RULE_AND_SCHEDULING_BUDGET_MS: u64 = 250;

    let frame_process_ms = max_wall_time_ms
        .checked_mul(entry_calls.max(1))?
        .checked_add(RULE_AND_SCHEDULING_BUDGET_MS)?;
    // Display 在上一 Frame 完整 write + flush 后才排入同一方向 worker。下一次 inspect 的
    // timeout 从 mailbox send 开始，因此必须同时覆盖“上一 Display + 当前 Frame”两次 Rhai
    // 墙钟上限；否则 encode-only 配置中的合法慢 Display 会把下一 Frame 误判为超时。
    let frame_after_display_ms = max_wall_time_ms
        .checked_mul(2)?
        .checked_add(RULE_AND_SCHEDULING_BUDGET_MS)?;
    Some(frame_process_ms.max(frame_after_display_ms))
}

impl ScriptedRelayProcessorFactory for ScriptedRelayProcessorFactoryAdapter {
    fn create_direction(
        &self,
        connection: SocketConnectionIdentity,
        direction: SocketPayloadDirection,
    ) -> Box<dyn SocketFrameProcessor> {
        match self.build_processor(connection, direction) {
            Ok(processor) => Box::new(processor),
            Err(failure) => Box::new(FailedFrameProcessor { failure }),
        }
    }
}

struct ScriptedRelayFrameProcessor {
    commands: mpsc::Sender<DirectionCommand>,
    cancellation: ProtocolExecutionCancellation,
}

impl ScriptedRelayFrameProcessor {
    fn spawn(
        state: DirectionWorkerState,
        blocking_slots: BlockingCommandSlots,
        cancellation: ProtocolExecutionCancellation,
    ) -> Self {
        let (commands, receiver) = mpsc::channel(1);
        // 每方向一个轻量 async mailbox；真正的 Rhai 调用通过共享 blocking pool 执行。
        // Sender 全部释放后，receiver 会先排空已经提交的 Display，再自然退出。
        std::mem::drop(tokio::spawn(run_direction_worker(
            receiver,
            state,
            blocking_slots,
        )));
        Self {
            commands,
            cancellation,
        }
    }

    async fn send<T>(
        &self,
        build: impl FnOnce(oneshot::Sender<T>) -> DirectionCommand,
    ) -> Result<T, SocketProcessingFailure> {
        let mut cancel_on_drop = CancelOnDrop::new(self.cancellation.clone());
        let (reply, receive) = oneshot::channel();
        self.commands
            .send(build(reply))
            .await
            .map_err(|_| worker_failure())?;
        let result = receive.await.map_err(|_| worker_failure());
        cancel_on_drop.disarm();
        result
    }
}

struct CancelOnDrop {
    cancellation: ProtocolExecutionCancellation,
    armed: bool,
}

impl CancelOnDrop {
    fn new(cancellation: ProtocolExecutionCancellation) -> Self {
        Self {
            cancellation,
            armed: true,
        }
    }

    fn disarm(&mut self) {
        self.armed = false;
    }
}

impl Drop for CancelOnDrop {
    fn drop(&mut self) {
        if self.armed {
            self.cancellation.cancel();
        }
    }
}

#[async_trait]
impl SocketFrameProcessor for ScriptedRelayFrameProcessor {
    async fn inspect(&mut self, buffered: Bytes) -> Result<FrameBoundary, SocketProcessingFailure> {
        self.send(|reply| DirectionCommand::Inspect { buffered, reply })
            .await?
    }

    async fn process(&mut self, origin: Bytes) -> Result<Bytes, SocketProcessingFailure> {
        self.send(|reply| DirectionCommand::Process { origin, reply })
            .await?
    }

    fn output_committed(&mut self) {
        // Pump 保证该方法紧跟当前 processor 的 Process/write/flush，且在下一次 inspect 之前调用；
        // 因此容量 1 mailbox 此刻必有空间。异常只丢弃旁路 Display，不能反写已提交线路。
        let _ = self.commands.try_send(DirectionCommand::CommitDisplay);
    }
}

struct FailedFrameProcessor {
    failure: SocketProcessingFailure,
}

#[async_trait]
impl SocketFrameProcessor for FailedFrameProcessor {
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

enum DirectionCommand {
    Inspect {
        buffered: Bytes,
        reply: oneshot::Sender<Result<FrameBoundary, SocketProcessingFailure>>,
    },
    Process {
        origin: Bytes,
        reply: oneshot::Sender<Result<Bytes, SocketProcessingFailure>>,
    },
    CommitDisplay,
}

struct DirectionWorkerState {
    inspector: ProtocolFrameInspector,
    executor: ProtocolDirectionExecutor,
    rules: SocketDocumentRuleConnection,
    package: intercept_proxy_domain::ProtocolPackageRef,
    pending_output: Option<ProtocolFrameOutput>,
    cancellation: ProtocolExecutionCancellation,
}

async fn run_direction_worker(
    mut commands: mpsc::Receiver<DirectionCommand>,
    mut state: DirectionWorkerState,
    blocking_slots: BlockingCommandSlots,
) {
    while let Some(mut command) = commands.recv().await {
        let Some(permits) = acquire_command_permits(&blocking_slots, &mut command).await else {
            // Inspect/Process 的 None 表示等待 reply 的 Pump future 已取消；CommitDisplay
            // 的 None 表示当前无阻塞许可。两者都不进入 blocking pool，但 Display 必须
            // 同时消费上一帧的待展示状态，否则下一帧会被误判为“尚未提交”。
            discard_skipped_command(&mut state.pending_output, &command);
            continue;
        };
        let result = tokio::task::spawn_blocking(move || {
            let _permits = permits;
            run_command(state, command)
        })
        .await;
        match result {
            Ok(next) => state = next,
            // Rust panic 只终止当前方向 worker；等待中的 oneshot 随 command drop 自动关闭，
            // processor 会把它稳定映射为 ProcessorPanicked，绝不携带 panic payload。
            Err(_) => return,
        }
    }
}

fn discard_skipped_command<T>(pending_output: &mut Option<T>, command: &DirectionCommand) {
    if matches!(command, DirectionCommand::CommitDisplay) {
        let _ = pending_output.take();
    }
}

fn run_command(mut state: DirectionWorkerState, command: DirectionCommand) -> DirectionWorkerState {
    match command {
        DirectionCommand::Inspect { buffered, reply } => {
            let result = state
                .inspector
                .inspect(&buffered)
                .map(frame_boundary)
                .map_err(|error| framing_failure(&error));
            let _ = reply.send(result);
        }
        DirectionCommand::Process { origin, reply } => {
            let result = process_frame(&mut state, &origin);
            let _ = reply.send(result);
        }
        DirectionCommand::CommitDisplay => {
            if let Some(output) = state.pending_output.take() {
                // Display 是 write/flush 后的旁路。脚本错误已由 render_display 降级；额外隔离
                // Rust panic，保证下一 Frame 仍可继续使用同一连接方向 worker。
                let _ = std::panic::catch_unwind(AssertUnwindSafe(|| {
                    let _ = state.executor.render_display(&output);
                }));
            }
        }
    }
    state
}

fn process_frame(
    state: &mut DirectionWorkerState,
    origin: &Bytes,
) -> Result<Bytes, SocketProcessingFailure> {
    if state.pending_output.is_some() {
        return Err(processing_failure(
            "previous frame output was not committed before next process",
        ));
    }
    let package = state.package.clone();
    let cancellation = state.cancellation.clone();
    let output = state
        .executor
        .execute_frame_with_document_transform(origin.to_vec(), |document| {
            state
                .rules
                .execute_with_cancellation(state.rules.bind_document(document), || {
                    cancellation.is_cancelled()
                })
                .map(intercept_proxy_domain::SocketDocumentRuleExecution::into_document)
                .map_err(|_| ProtocolRuntimeError::DocumentTransformFailed {
                    package: package.clone(),
                })
        })
        .map_err(|error| runtime_failure(&error))?;
    let written = Bytes::from_owner(output.written_owner());
    state.pending_output = Some(output);
    Ok(written)
}

fn frame_boundary(inspection: ProtocolFrameInspection) -> FrameBoundary {
    match inspection {
        ProtocolFrameInspection::NeedMore { total } => FrameBoundary::NeedMore { total },
        ProtocolFrameInspection::Complete { bytes } => FrameBoundary::Complete { bytes },
        ProtocolFrameInspection::Reject { reason } => FrameBoundary::Reject { reason },
    }
}

fn framing_failure(error: &ProtocolFramingError) -> SocketProcessingFailure {
    let kind = match error {
        ProtocolFramingError::FrameTooLarge { .. }
        | ProtocolFramingError::FifoLimitExceeded { .. } => {
            SocketProcessingFailureKind::BufferLimitExceeded
        }
        ProtocolFramingError::InvalidLimit { .. }
        | ProtocolFramingError::FifoSmallerThanFrame { .. } => {
            SocketProcessingFailureKind::InvalidLimits
        }
        ProtocolFramingError::InvalidDecisionLength
        | ProtocolFramingError::InvalidRejectReason
        | ProtocolFramingError::NeedMoreWithoutProgress
        | ProtocolFramingError::CompleteEmpty
        | ProtocolFramingError::CompleteOutOfBounds => {
            SocketProcessingFailureKind::InvalidFrameBoundary
        }
        ProtocolFramingError::Rejected { .. } => SocketProcessingFailureKind::FrameRejected,
        ProtocolFramingError::TruncatedFrame { .. } => SocketProcessingFailureKind::TruncatedFrame,
        ProtocolFramingError::ReaderOutOfBounds
        | ProtocolFramingError::EmptyFindPattern
        | ProtocolFramingError::InvalidFindStart
        | ProtocolFramingError::FrameEntryFailed { .. } => {
            SocketProcessingFailureKind::ProcessingFailed
        }
        ProtocolFramingError::FrameExecutionCancelled { .. } => {
            SocketProcessingFailureKind::Cancelled
        }
    };
    SocketProcessingFailure::new(kind, "protocol frame inspection failed")
}

fn runtime_failure(error: &ProtocolRuntimeError) -> SocketProcessingFailure {
    if matches!(error, ProtocolRuntimeError::ExecutionCancelled { .. }) {
        SocketProcessingFailure::new(
            SocketProcessingFailureKind::Cancelled,
            "protocol frame execution cancelled",
        )
    } else {
        processing_failure("protocol frame processing failed")
    }
}

fn processing_failure(message: &'static str) -> SocketProcessingFailure {
    SocketProcessingFailure::new(SocketProcessingFailureKind::ProcessingFailed, message)
}

fn worker_failure() -> SocketProcessingFailure {
    SocketProcessingFailure::new(
        SocketProcessingFailureKind::ProcessorPanicked,
        "scripted direction worker stopped",
    )
}

fn invalid_limits() -> SocketProcessingFailure {
    SocketProcessingFailure::new(
        SocketProcessingFailureKind::InvalidLimits,
        "scripted frame limits cannot be represented safely",
    )
}

#[cfg(test)]
mod tests;
