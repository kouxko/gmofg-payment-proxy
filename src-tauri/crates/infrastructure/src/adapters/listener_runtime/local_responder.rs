//! 冻结协议包到 Proxy `LocalResponder` request-response processor 的连接级适配。
//!
//! 该模块只接收 App 侧完整 request，并把一个完整 response 写回同一连接。网络服务由
//! Proxy 的专用 `LocalResponder` pump 持有，因此这里不存在 resolver、connector、upstream
//! address 或 upstream TLS 字段；请求和响应之间的唯一桥由 protocol-scripting 的
//! `LocalResponderCoordinator` 管理。

use async_trait::async_trait;
use bytes::Bytes;
use chrono::{DateTime, Utc};
use intercept_proxy_application::{SocketCaptureSchemaRef, SocketExchangeId};
use intercept_proxy_domain::SocketDirection;
use intercept_proxy_protocol_scripting::{
    LocalResponderCoordinator, ProtocolDirection, ProtocolExecutionCancellation,
    ProtocolFrameInspector, ProtocolFramingLimits, ProtocolRuntimeError, ProtocolRuntimeLimits,
};
use intercept_proxy_runtime::{
    FrameBoundary, LocalResponderDiagnostics, LocalResponderProcessorFactory,
    SocketConnectionIdentity, SocketFrameProcessor, SocketProcessingFailure,
};
use tokio::sync::{mpsc, oneshot};

use super::{
    SocketDocumentRuleConnection, SocketDocumentRuleConnectionFactory,
    scripted_relay::limiter::{BlockingCommandSlots, acquire_for_reply},
    scripted_snapshot::ScriptedSocketRuntimeSnapshot,
    socket_capture_publisher::{SocketCaptureContext, SocketCapturePublishTicket},
};
use crate::adapters::protocol_packages::runtime_snapshot::RuntimeProtocolPackageSnapshot;

mod capture;
mod failure;
mod limits;
mod preview;
use failure::{
    frame_boundary, framing_failure, invalid_limits, processing_failure, request_runtime_failure,
    response_runtime_failure, worker_failure,
};
pub(super) use limits::local_frame_pump_limits;

/// 同一次 Listener 启动快照派生的 `LocalResponder` processor factory。
pub(super) struct LocalResponderProcessorFactoryAdapter {
    package: RuntimeProtocolPackageSnapshot,
    request_decode_enabled: bool,
    response_encode_enabled: bool,
    rules: SocketDocumentRuleConnectionFactory,
    listener_id: String,
    runtime_limits: ProtocolRuntimeLimits,
    framing_limits: ProtocolFramingLimits,
    blocking_slots: BlockingCommandSlots,
    capture: SocketCaptureContext,
    schema: SocketCaptureSchemaRef,
}

impl LocalResponderProcessorFactoryAdapter {
    pub(super) fn new(
        snapshot: &ScriptedSocketRuntimeSnapshot,
        listener_id: String,
        framing_limits: ProtocolFramingLimits,
        capture: SocketCaptureContext,
    ) -> Self {
        let schema = snapshot.package().compiled().schema();
        Self {
            package: snapshot.package().clone(),
            request_decode_enabled: snapshot.upstream().decode_enabled(),
            response_encode_enabled: snapshot.downstream().encode_enabled(),
            rules: snapshot.rule_connections().clone(),
            listener_id,
            runtime_limits: snapshot.runtime_limits(),
            framing_limits,
            blocking_slots: BlockingCommandSlots::new_local(snapshot.maximum_connections()),
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
    ) -> Result<LocalResponderFrameProcessor, SocketProcessingFailure> {
        let connection_context =
            format!("{}:{}", connection.runtime_epoch, connection.connection_id);
        let cancellation = ProtocolExecutionCancellation::new();
        let inspector = ProtocolFrameInspector::new_with_cancellation(
            self.package.compiled(),
            ProtocolDirection::Upstream,
            connection_context.clone(),
            self.listener_id.clone(),
            self.runtime_limits,
            self.framing_limits,
            cancellation.clone(),
        );
        let coordinator = LocalResponderCoordinator::new_with_cancellation(
            self.package.compiled(),
            self.request_decode_enabled,
            self.response_encode_enabled,
            connection_context,
            self.listener_id.clone(),
            self.runtime_limits,
            cancellation.clone(),
        )
        .map_err(|error| request_runtime_failure(&error))?;
        let rules = self
            .rules
            .connection(connection.clone(), SocketDirection::Downstream);
        Ok(LocalResponderFrameProcessor::spawn(
            LocalWorkerState {
                inspector,
                coordinator,
                rules,
                package: self.package.compiled().package().clone(),
                pending_response: None,
                diagnostics: None,
                cancellation: cancellation.clone(),
                connection,
                capture: self.capture.clone(),
                schema: self.schema.clone(),
                request_decode_enabled: self.request_decode_enabled,
                response_encode_enabled: self.response_encode_enabled,
            },
            self.blocking_slots.clone(),
            cancellation,
        ))
    }
}

impl LocalResponderProcessorFactory for LocalResponderProcessorFactoryAdapter {
    fn create_exchange(
        &self,
        connection: SocketConnectionIdentity,
    ) -> Box<dyn SocketFrameProcessor> {
        match self.build_processor(connection) {
            Ok(processor) => Box::new(processor),
            Err(failure) => Box::new(FailedLocalProcessor { failure }),
        }
    }
}

struct LocalResponderFrameProcessor {
    commands: mpsc::Sender<LocalCommand>,
    cancellation: ProtocolExecutionCancellation,
    capture: SocketCaptureContext,
}

impl LocalResponderFrameProcessor {
    fn spawn(
        state: LocalWorkerState,
        blocking_slots: BlockingCommandSlots,
        cancellation: ProtocolExecutionCancellation,
    ) -> Self {
        let (commands, receiver) = mpsc::channel(1);
        let capture = state.capture.clone();
        // 每连接仅创建一个 exchange worker。所有 Rhai 与规则调用继续经同一进程级 blocking
        // limiter 进入 spawn_blocking，避免同步脚本阻塞 Tokio I/O worker。
        std::mem::drop(tokio::spawn(run_local_worker(
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
        build: impl FnOnce(oneshot::Sender<T>) -> LocalCommand,
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
impl SocketFrameProcessor for LocalResponderFrameProcessor {
    async fn inspect(&mut self, buffered: Bytes) -> Result<FrameBoundary, SocketProcessingFailure> {
        self.send(|reply| LocalCommand::Inspect { buffered, reply })
            .await?
    }

    async fn process(&mut self, origin: Bytes) -> Result<Bytes, SocketProcessingFailure> {
        let occurred_at = Utc::now();
        self.send(|reply| LocalCommand::Process {
            origin,
            occurred_at,
            reply,
        })
        .await?
    }

    fn set_local_diagnostics(&mut self, diagnostics: LocalResponderDiagnostics) {
        // Proxy 在 processor 构造后、第一次 inspect 前恰好注入一次；有界 mailbox 此时为空。
        // 若 worker 已异常退出，只会失去旁路诊断，不会创建第二条事件通道。
        let _ = self
            .commands
            .try_send(LocalCommand::SetDiagnostics(diagnostics));
    }

    fn output_committed(&mut self) {
        // Proxy pump 只会在 response 全量 write + flush 后通知。Display 是可丢弃旁路，不能
        // 因脚本失败、panic 或 blocking pool 饱和而改变已经提交的线路结果。generation
        // 必须在此处冻结，所有晚到的 Display 分支只能沿用该票。
        let ticket = self.capture.ticket();
        let _ = self.commands.try_send(LocalCommand::CommitDisplay {
            completed_at: Utc::now(),
            ticket,
        });
    }
}

struct FailedLocalProcessor {
    failure: SocketProcessingFailure,
}

#[async_trait]
impl SocketFrameProcessor for FailedLocalProcessor {
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

enum LocalCommand {
    SetDiagnostics(LocalResponderDiagnostics),
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

struct LocalWorkerState {
    inspector: ProtocolFrameInspector,
    coordinator: LocalResponderCoordinator,
    rules: SocketDocumentRuleConnection,
    package: intercept_proxy_domain::ProtocolPackageRef,
    pending_response: Option<capture::PendingLocalCapture>,
    diagnostics: Option<LocalResponderDiagnostics>,
    cancellation: ProtocolExecutionCancellation,
    connection: SocketConnectionIdentity,
    capture: SocketCaptureContext,
    schema: SocketCaptureSchemaRef,
    request_decode_enabled: bool,
    response_encode_enabled: bool,
}

async fn run_local_worker(
    mut commands: mpsc::Receiver<LocalCommand>,
    mut state: LocalWorkerState,
    blocking_slots: BlockingCommandSlots,
) {
    while let Some(mut command) = commands.recv().await {
        if let LocalCommand::SetDiagnostics(diagnostics) = command {
            state.diagnostics = Some(diagnostics);
            continue;
        }
        let permits = match &mut command {
            LocalCommand::Inspect { reply, .. } => acquire_for_reply(&blocking_slots, reply).await,
            LocalCommand::Process { reply, .. } => acquire_for_reply(&blocking_slots, reply).await,
            // Display 已在线路提交之后。资源繁忙时丢弃展示并清理 pending response，不能
            // 留下等待 blocking permit 的孤儿任务，也不能阻塞下一个 request。
            LocalCommand::CommitDisplay { .. } => blocking_slots.try_acquire(),
            LocalCommand::SetDiagnostics(_) => unreachable!("handled before permit acquisition"),
        };
        let Some(permits) = permits else {
            if let LocalCommand::CommitDisplay {
                completed_at,
                ticket,
            } = command
                && let Some(pending) = state.pending_response.take()
            {
                capture::commit(
                    &mut state.coordinator,
                    pending,
                    ticket,
                    &state.capture,
                    &state.connection,
                    completed_at,
                    state.package.clone(),
                    state.schema.clone(),
                    state.request_decode_enabled,
                    state.response_encode_enabled,
                    false,
                );
            }
            continue;
        };
        let result = tokio::task::spawn_blocking(move || {
            let _permits = permits;
            run_local_command(state, command)
        })
        .await;
        match result {
            Ok(next) => state = next,
            // panic 只关闭当前 exchange worker；等待的 oneshot 自动关闭并由 processor
            // 映射为稳定 ProcessorPanicked，panic payload 不进入诊断。
            Err(_) => return,
        }
    }
}

fn run_local_command(mut state: LocalWorkerState, command: LocalCommand) -> LocalWorkerState {
    match command {
        LocalCommand::SetDiagnostics(_) => unreachable!("handled by async worker"),
        LocalCommand::Inspect { buffered, reply } => {
            let result = state
                .inspector
                .inspect(&buffered)
                .map(frame_boundary)
                .map_err(|error| framing_failure(&error));
            let _ = reply.send(result);
        }
        LocalCommand::Process {
            origin,
            occurred_at,
            reply,
        } => {
            let result = process_exchange(&mut state, &origin, occurred_at);
            let _ = reply.send(result);
        }
        LocalCommand::CommitDisplay {
            completed_at,
            ticket,
        } => {
            if let Some(pending) = state.pending_response.take() {
                // response_committed 验证 handle 归属；任一错误或 panic 都只降级展示，不能
                // 反写已提交 response 或毒化下一次 request。
                capture::commit(
                    &mut state.coordinator,
                    pending,
                    ticket,
                    &state.capture,
                    &state.connection,
                    completed_at,
                    state.package.clone(),
                    state.schema.clone(),
                    state.request_decode_enabled,
                    state.response_encode_enabled,
                    true,
                );
            }
        }
    }
    state
}

fn process_exchange(
    state: &mut LocalWorkerState,
    origin: &Bytes,
    occurred_at: DateTime<Utc>,
) -> Result<Bytes, SocketProcessingFailure> {
    if state.pending_response.is_some() {
        return Err(processing_failure(
            "previous local response was not committed before next request",
        ));
    }
    let exchange_id = SocketExchangeId::new();
    let request = state
        .coordinator
        .decode_request(origin.to_vec())
        .map_err(|error| request_runtime_failure(&error))?;
    preview::publish_request_parsed(state.diagnostics.as_ref(), exchange_id, &request);
    let request_document = request.document().cloned();
    let package = state.package.clone();
    let cancellation = state.cancellation.clone();
    let mut matched_rule_ids = Vec::new();
    let response = state
        .coordinator
        .build_response(&request, |document| {
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
        .map_err(|error| response_runtime_failure(&error))?;
    let written = Bytes::from_owner(response.written_owner());
    state.pending_response = Some(capture::PendingLocalCapture::new(
        response,
        exchange_id,
        request_document,
        matched_rule_ids,
        occurred_at,
    ));
    Ok(written)
}
