use super::{
    Arc, BoundListener, BoxIo, Bytes, CancellationToken, CanonicalResponseHead, ChannelId, Clock,
    ConnectionAcceptor, ConnectionContext, Debug, Duration, ErrorCode,
    HttpProtocolCapabilityFactory, InformationalResponseSink, IntentionalWireFault, MessageLimits,
    PipelinePorts, ProxyError, RawHttp1HeadCapture, Result, StdMutex, TcpListener,
    TokioBoundListener, UpstreamConnector, Uuid, timeout_stage,
};
use async_trait::async_trait;

use crate::listener::{
    ConnectionHandler, ConnectionTaskScope, ListenerCapacity, ListenerConfig, ListenerSupervisor,
    NoopConnectionLifecycleObserver, PrimaryConnectionOutcome, sealed,
};
use crate::transport::TokioListenerBinder;

const HTTP_LISTENER_SHUTDOWN_GRACE: Duration = Duration::from_secs(5);

#[derive(Debug, Clone)]
pub struct ConnectionService {
    // 这些 Arc 是一个 epoch 的不可变服务快照；每个连接只克隆所有权，不在处理中热换实现。
    pub acceptor: Arc<dyn ConnectionAcceptor>,
    pub upstream: Arc<dyn UpstreamConnector>,
    pub ports: Arc<dyn PipelinePorts>,
    /// 一条 accepted connection 独占创建的 HTTP 协议能力。
    pub capabilities: Arc<dyn HttpProtocolCapabilityFactory>,
    /// Listener 创建时固定的 Server Endpoint；Exchange 生命周期内不可切换。
    pub endpoint: String,
    pub clock: Arc<dyn Clock>,
    pub admission: ConnectionAdmission,
    pub allowed_client_cidrs: Vec<String>,
    pub limits: MessageLimits,
    /// Covers the inbound TLS handshake and the downstream request body.
    pub read_timeout: Duration,
    /// Covers each downstream response write stage.
    pub write_timeout: Duration,
}

pub(super) struct RequestWireState<'a> {
    pub(super) raw_request_head: &'a StdMutex<RawHttp1HeadCapture>,
    pub(super) canonical_response_head: &'a CanonicalResponseHead,
    pub(super) informational_response_sink: &'a InformationalResponseSink,
    pub(super) raw_tail: &'a StdMutex<Option<Bytes>>,
    pub(super) intentional_wire_fault: &'a StdMutex<Option<IntentionalWireFault>>,
}

/// Shared per-epoch admission control. All channel listeners clone the same
/// instance so their combined pre-handshake and active connection count stays
/// within one configured capacity.
#[derive(Debug, Clone)]
pub struct ConnectionAdmission {
    capacity: ListenerCapacity,
}

impl ConnectionAdmission {
    pub fn new(capacity: usize) -> Result<Self> {
        Ok(Self {
            capacity: ListenerCapacity::new(capacity)?,
        })
    }

    fn listener_capacity(&self) -> ListenerCapacity {
        self.capacity.clone()
    }
}

impl ConnectionService {
    /// 在调用方已经绑定好 `TcpListener` 的场景运行完整 HTTP/规则管线。
    ///
    /// 动态 Workspace Listener 需要先完成持久化快照与端口冲突校验，再把同一个 socket
    /// 交给运行时；这个入口避免它复制 supervisor 的连接管理、容量和取消语义。
    pub async fn run_tcp_listener(
        &self,
        listener: TcpListener,
        channel: ChannelId,
        epoch: Uuid,
        cancellation: CancellationToken,
    ) -> Result<()> {
        self.run_listener(
            Arc::new(TokioBoundListener(listener)),
            channel,
            epoch,
            cancellation,
        )
        .await
    }

    /// 接受一个通道的连接，直到根取消令牌触发或 listener 本身失败。
    ///
    /// 容量许可从 accept 后、TLS 前开始计数，并由连接任务持有到完全退出；这样沉默握手
    /// 也受全局上限约束。退出循环后仍会取消并 join 全部子任务，防止旧 epoch 泄漏。
    pub async fn run_listener(
        &self,
        listener: Arc<dyn BoundListener>,
        channel: ChannelId,
        epoch: Uuid,
        cancellation: CancellationToken,
    ) -> Result<()> {
        let bind_addr = listener
            .local_addr()
            .map_err(|error| ProxyError::io("read HTTP listener address", &error))?;
        let supervisor = ListenerSupervisor::new(
            ListenerConfig {
                bind_addr,
                runtime_epoch: epoch,
                listener_id: channel,
                allowed_client_cidrs: self.allowed_client_cidrs.clone(),
                capacity: self.admission.listener_capacity(),
                shutdown_grace: HTTP_LISTENER_SHUTDOWN_GRACE,
            },
            Arc::new(TokioListenerBinder),
            Arc::clone(&self.clock),
            Arc::new(self.clone()),
            Arc::new(NoopConnectionLifecycleObserver),
        )?;
        supervisor
            .run_bound(listener, cancellation)
            .await?
            .into_result("HTTP listener stopped after a fatal lifecycle failure")
    }

    async fn run_connection(
        &self,
        io: BoxIo,
        mut context: ConnectionContext,
        cancellation: CancellationToken,
        task_scope: &ConnectionTaskScope,
    ) -> Result<()> {
        let accepted = timeout_stage(
            self.read_timeout,
            &cancellation,
            self.acceptor.accept(io, &context),
            ErrorCode::TlsHandshakeFailed,
        )
        .await
        .and_then(std::convert::identity);
        let result = match accepted {
            Ok(accepted) => {
                context.tls_peer = accepted.tls_peer;
                self.ports.connection_opened(&context).await;
                self.run_connection_inner_in_scope(accepted.io, &context, cancellation, task_scope)
                    .await
            }
            Err(error) => Err(error),
        };
        self.ports.connection_closed(&context, &result).await;
        result
    }
}

impl sealed::Sealed for ConnectionService {}

#[async_trait]
impl ConnectionHandler for ConnectionService {
    async fn handle(
        &self,
        io: BoxIo,
        context: ConnectionContext,
        child_tasks: ConnectionTaskScope,
        cancellation: CancellationToken,
    ) -> PrimaryConnectionOutcome {
        self.run_connection(io, context, cancellation, &child_tasks)
            .await
            .into()
    }
}
