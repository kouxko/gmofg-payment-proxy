//! 冻结协议包到 Proxy Scripted Relay processor 的连接级适配。

use async_trait::async_trait;
use bytes::Bytes;
use chrono::{DateTime, Utc};
use intercept_proxy_application::SocketCaptureSchemaRef;
use intercept_proxy_domain::SocketDirection;
use intercept_proxy_protocol_scripting::{
    DirectionExecutionPlan, ProtocolDirection, ProtocolDirectionExecutor,
    ProtocolExecutionCancellation, ProtocolFrameInspector, ProtocolFramingLimits,
    ProtocolRuntimeError, ProtocolRuntimeLimits,
};
use intercept_proxy_runtime::{
    FrameBoundary, ScriptedRelayProcessorFactory, SocketConnectionIdentity, SocketFrameProcessor,
    SocketPayloadDirection, SocketProcessingFailure,
};
use tokio::sync::{mpsc, oneshot};

use super::{
    SocketDocumentRuleConnection, SocketDocumentRuleConnectionFactory,
    scripted_snapshot::ScriptedSocketRuntimeSnapshot,
    socket_capture_publisher::{SocketCaptureContext, SocketCapturePublishTicket},
};
use crate::adapters::protocol_packages::runtime_snapshot::RuntimeProtocolPackageSnapshot;

use limiter::{BlockingCommandSlots, acquire_command_permits};

mod capture;
mod failure;
pub(super) mod limiter;
mod limits;
use failure::{
    frame_boundary, framing_failure, invalid_limits, processing_failure, runtime_failure,
    worker_failure,
};
#[cfg(test)]
use limits::processing_budget_ms;
pub(super) use limits::{frame_pump_limits, frame_pump_limits_for_entry_calls};

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
    capture: SocketCaptureContext,
    schema: SocketCaptureSchemaRef,
}

impl ScriptedRelayProcessorFactoryAdapter {
    pub(super) fn new(
        snapshot: &ScriptedSocketRuntimeSnapshot,
        listener_id: String,
        framing_limits: ProtocolFramingLimits,
        capture: SocketCaptureContext,
    ) -> Self {
        let schema = snapshot.package().compiled().schema();
        Self {
            package: snapshot.package().clone(),
            upstream: snapshot.upstream(),
            downstream: snapshot.downstream(),
            rules: snapshot.rule_connections().clone(),
            listener_id,
            runtime_limits: snapshot.runtime_limits(),
            framing_limits,
            blocking_slots: BlockingCommandSlots::new_relay(snapshot.maximum_connections()),
            capture,
            schema: SocketCaptureSchemaRef {
                id: schema.id().clone(),
                version: schema.version(),
            },
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
        let rules = self.rules.connection(connection.clone(), rule_direction);
        Ok(ScriptedRelayFrameProcessor::spawn(
            DirectionWorkerState {
                inspector,
                executor,
                rules,
                package: self.package.compiled().package().clone(),
                pending_output: None,
                connection,
                direction: rule_direction,
                decode_enabled: plan.decode_enabled(),
                encode_enabled: plan.encode_enabled(),
                capture: self.capture.clone(),
                schema: self.schema.clone(),
                cancellation: cancellation.clone(),
            },
            self.blocking_slots.clone(),
            cancellation,
        ))
    }
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
    capture: SocketCaptureContext,
}

impl ScriptedRelayFrameProcessor {
    fn spawn(
        state: DirectionWorkerState,
        blocking_slots: BlockingCommandSlots,
        cancellation: ProtocolExecutionCancellation,
    ) -> Self {
        let (commands, receiver) = mpsc::channel(1);
        let capture = state.capture.clone();
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
            capture,
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
        let occurred_at = Utc::now();
        self.send(|reply| DirectionCommand::Process {
            origin,
            occurred_at,
            reply,
        })
        .await?
    }

    fn output_committed(&mut self) {
        // Pump 保证该方法紧跟当前 processor 的 Process/write/flush，且在下一次 inspect 之前调用；
        // 此处先冻结 Workspace generation，再把同一票交给所有 Display 结果。容量 1 mailbox
        // 此刻必有空间；异常只丢弃旁路 Display，不能反写已提交线路。
        let ticket = self.capture.ticket();
        let _ = self.commands.try_send(DirectionCommand::CommitDisplay {
            completed_at: Utc::now(),
            ticket,
        });
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
        occurred_at: DateTime<Utc>,
        reply: oneshot::Sender<Result<Bytes, SocketProcessingFailure>>,
    },
    CommitDisplay {
        completed_at: DateTime<Utc>,
        ticket: Option<SocketCapturePublishTicket>,
    },
}

struct DirectionWorkerState {
    inspector: ProtocolFrameInspector,
    executor: ProtocolDirectionExecutor,
    rules: SocketDocumentRuleConnection,
    package: intercept_proxy_domain::ProtocolPackageRef,
    pending_output: Option<capture::PendingRelayCapture>,
    connection: SocketConnectionIdentity,
    direction: SocketDirection,
    decode_enabled: bool,
    encode_enabled: bool,
    capture: SocketCaptureContext,
    schema: SocketCaptureSchemaRef,
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
            if let DirectionCommand::CommitDisplay {
                completed_at,
                ticket,
            } = command
                && let Some(pending) = state.pending_output.take()
            {
                capture::commit(
                    None,
                    pending,
                    ticket,
                    &state.capture,
                    &state.connection,
                    completed_at,
                    state.direction,
                    state.package.clone(),
                    state.schema.clone(),
                    state.decode_enabled,
                    state.encode_enabled,
                );
            }
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
        DirectionCommand::Process {
            origin,
            occurred_at,
            reply,
        } => {
            let result = process_frame(&mut state, &origin, occurred_at);
            let _ = reply.send(result);
        }
        DirectionCommand::CommitDisplay {
            completed_at,
            ticket,
        } => {
            if let Some(pending) = state.pending_output.take() {
                // Display 是 write/flush 后的旁路。脚本错误已由 render_display 降级；额外隔离
                // Rust panic，保证下一 Frame 仍可继续使用同一连接方向 worker。
                capture::commit(
                    Some(&mut state.executor),
                    pending,
                    ticket,
                    &state.capture,
                    &state.connection,
                    completed_at,
                    state.direction,
                    state.package.clone(),
                    state.schema.clone(),
                    state.decode_enabled,
                    state.encode_enabled,
                );
            }
        }
    }
    state
}

fn process_frame(
    state: &mut DirectionWorkerState,
    origin: &Bytes,
    occurred_at: DateTime<Utc>,
) -> Result<Bytes, SocketProcessingFailure> {
    if state.pending_output.is_some() {
        return Err(processing_failure(
            "previous frame output was not committed before next process",
        ));
    }
    let package = state.package.clone();
    let cancellation = state.cancellation.clone();
    let mut matched_rule_ids = Vec::new();
    let output = state
        .executor
        .execute_frame_with_document_transform(origin.to_vec(), |document| {
            state
                .rules
                .execute_with_cancellation(state.rules.bind_document(document), || {
                    cancellation.is_cancelled()
                })
                .map(|execution| {
                    let (document, ids) = execution.into_parts();
                    matched_rule_ids = ids;
                    document
                })
                .map_err(|_| ProtocolRuntimeError::DocumentTransformFailed {
                    package: package.clone(),
                })
        })
        .map_err(|error| runtime_failure(&error))?;
    let written = Bytes::from_owner(output.written_owner());
    state.pending_output = Some(capture::PendingRelayCapture::new(
        output,
        matched_rule_ids,
        occurred_at,
    ));
    Ok(written)
}

#[cfg(test)]
mod tests;
