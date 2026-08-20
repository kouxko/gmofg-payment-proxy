//! 冻结协议包到 Proxy `LocalResponder` request-response processor 的连接级适配。
//!
//! 该模块只接收 App 侧完整 request，并把一个完整 response 写回同一连接。网络服务由
//! Proxy 的专用 `LocalResponder` pump 持有，因此这里不存在 resolver、connector、upstream
//! address 或 upstream TLS 字段；请求和响应之间的唯一桥由 protocol-scripting 的
//! `LocalResponderCoordinator` 管理。

use async_trait::async_trait;
use bytes::Bytes;
use chrono::{DateTime, Utc};
use intercept_proxy_application::SocketCaptureSchemaRef;
use intercept_proxy_domain::ProtocolRuleStage;
use intercept_proxy_protocol_scripting::{
    LocalResponderCoordinator, ProtocolDirection, ProtocolExecutionCancellation,
    ProtocolFrameInspector, ProtocolFramingLimits, ProtocolRuntimeLimits,
};
use intercept_proxy_runtime::{
    FrameBoundary, LocalResponderDiagnostics, LocalResponderProcessorFactory,
    SocketConnectionIdentity, SocketFrameProcessor, SocketProcessingFailure,
    SocketProcessingFailureKind,
};
use tokio::sync::{mpsc, oneshot};

use super::{
    ProtocolDocumentRuleConnection, ProtocolDocumentRuleConnectionFactory,
    scripted_relay::limiter::{BlockingCommandSlots, acquire_for_reply},
    scripted_snapshot::ScriptedSocketRuntimeSnapshot,
    socket_capture_publisher::{SocketCaptureContext, SocketCapturePublishTicket},
};
use crate::adapters::protocol_packages::runtime_snapshot::RuntimeProtocolPackageSnapshot;

mod capture;
mod failure;
mod limits;
mod preview;
mod worker;
use failure::{
    frame_boundary, framing_failure, processing_failure, request_runtime_failure,
    response_runtime_failure, worker_failure,
};
pub(super) use limits::local_frame_pump_limits;
use worker::run_local_worker;

/// 同一次 Listener 启动快照派生的 `LocalResponder` processor factory。
pub(super) struct LocalResponderProcessorFactoryAdapter {
    package: RuntimeProtocolPackageSnapshot,
    rules: ProtocolDocumentRuleConnectionFactory,
    listener_id: String,
    runtime_limits: ProtocolRuntimeLimits,
    framing_limits: ProtocolFramingLimits,
    blocking_slots: BlockingCommandSlots,
    capture: SocketCaptureContext,
    request_schema: SocketCaptureSchemaRef,
    response_schema: SocketCaptureSchemaRef,
}

impl LocalResponderProcessorFactoryAdapter {
    pub(super) fn new(
        snapshot: &ScriptedSocketRuntimeSnapshot,
        listener_id: String,
        framing_limits: ProtocolFramingLimits,
        capture: SocketCaptureContext,
    ) -> Self {
        let request_schema = snapshot
            .package()
            .compiled()
            .schema(ProtocolDirection::Upstream);
        let response_schema = snapshot
            .package()
            .compiled()
            .schema(ProtocolDirection::Downstream);
        Self {
            package: snapshot.package().clone(),
            rules: snapshot.rule_connections().clone(),
            listener_id,
            runtime_limits: snapshot.runtime_limits(),
            framing_limits,
            blocking_slots: BlockingCommandSlots::new_local(snapshot.maximum_connections()),
            capture,
            request_schema: SocketCaptureSchemaRef {
                id: request_schema.id().clone(),
                version: request_schema.version(),
            },
            response_schema: SocketCaptureSchemaRef {
                id: response_schema.id().clone(),
                version: response_schema.version(),
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
            connection_context,
            self.listener_id.clone(),
            self.runtime_limits,
            cancellation.clone(),
        )
        .map_err(|error| request_runtime_failure(&error))?;
        let request_rules = self
            .rules
            .connection(connection.clone(), ProtocolRuleStage::AppToProxy);
        let response_rules = self
            .rules
            .connection(connection.clone(), ProtocolRuleStage::ProxyToApp);
        Ok(LocalResponderFrameProcessor::spawn(
            LocalWorkerState {
                inspector,
                coordinator,
                request_rules,
                response_rules,
                package: self.package.compiled().package().clone(),
                pending_response: None,
                diagnostics: None,
                cancellation: cancellation.clone(),
                connection,
                capture: self.capture.clone(),
                request_schema: self.request_schema.clone(),
                response_schema: self.response_schema.clone(),
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

    fn output_failed(&mut self, failure: &SocketProcessingFailure, written_bytes: usize) {
        let ticket = self.capture.ticket();
        let _ = self.commands.try_send(LocalCommand::FailOutput {
            completed_at: Utc::now(),
            ticket,
            failure_kind: failure.kind,
            written_bytes,
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
    FailOutput {
        completed_at: DateTime<Utc>,
        ticket: Option<SocketCapturePublishTicket>,
        failure_kind: SocketProcessingFailureKind,
        written_bytes: usize,
    },
}

struct LocalWorkerState {
    inspector: ProtocolFrameInspector,
    coordinator: LocalResponderCoordinator,
    request_rules: ProtocolDocumentRuleConnection,
    response_rules: ProtocolDocumentRuleConnection,
    package: intercept_proxy_domain::ProtocolPackageRef,
    pending_response: Option<capture::PendingLocalCapture>,
    diagnostics: Option<LocalResponderDiagnostics>,
    cancellation: ProtocolExecutionCancellation,
    connection: SocketConnectionIdentity,
    capture: SocketCaptureContext,
    request_schema: SocketCaptureSchemaRef,
    response_schema: SocketCaptureSchemaRef,
}
