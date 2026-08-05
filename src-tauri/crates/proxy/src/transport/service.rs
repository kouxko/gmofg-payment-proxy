use super::{
    Arc, BoundListener, BoxIo, Bytes, CancellationToken, ChannelId, Clock, ConnectionAcceptor,
    ConnectionContext, Debug, Duration, ErrorCode, InformationalResponseSink, IntentionalWireFault,
    JoinSet, MessageLimits, OwnedSemaphorePermit, PipelinePorts, ProxyError, RawHttp1HeadCapture,
    Result, Semaphore, StdMutex, TcpListener, TokioBoundListener, TryAcquireError,
    UpstreamConnector, Uuid, timeout_stage,
};

#[derive(Debug, Clone)]
pub struct ConnectionService {
    // 这些 Arc 是一个 epoch 的不可变服务快照；每个连接只克隆所有权，不在处理中热换实现。
    pub acceptor: Arc<dyn ConnectionAcceptor>,
    pub upstream: Arc<dyn UpstreamConnector>,
    pub ports: Arc<dyn PipelinePorts>,
    pub clock: Arc<dyn Clock>,
    pub admission: ConnectionAdmission,
    pub limits: MessageLimits,
    /// Covers the inbound TLS handshake and the downstream request body.
    pub read_timeout: Duration,
    /// Covers each downstream response write stage.
    pub write_timeout: Duration,
}

pub(super) struct RequestWireState<'a> {
    pub(super) raw_request_head: &'a StdMutex<RawHttp1HeadCapture>,
    pub(super) canonical_response_head: &'a StdMutex<Option<Bytes>>,
    pub(super) informational_response_sink: &'a InformationalResponseSink,
    pub(super) raw_tail: &'a StdMutex<Option<Bytes>>,
    pub(super) intentional_wire_fault: &'a StdMutex<Option<IntentionalWireFault>>,
}

/// Shared per-epoch admission control. All channel listeners clone the same
/// instance so their combined pre-handshake and active connection count stays
/// within one configured capacity.
#[derive(Debug, Clone)]
pub struct ConnectionAdmission {
    permits: Arc<Semaphore>,
}

impl ConnectionAdmission {
    pub fn new(capacity: usize) -> Result<Self> {
        if capacity == 0 {
            return Err(ProxyError::new(
                ErrorCode::ConfigInvalid,
                "connection capacity must be greater than zero",
            ));
        }
        Ok(Self {
            permits: Arc::new(Semaphore::new(capacity)),
        })
    }

    fn try_acquire(&self) -> std::result::Result<OwnedSemaphorePermit, TryAcquireError> {
        Arc::clone(&self.permits).try_acquire_owned()
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
        let mut connections = JoinSet::new();
        loop {
            tokio::select! {
                () = cancellation.cancelled() => break,
                accepted = listener.accept() => {
                    let (io, peer_addr) = accepted
                        .map_err(|error| ProxyError::io("accept connection", &error))?;
                    let permit = match self.admission.try_acquire() {
                        Ok(permit) => permit,
                        Err(error) => {
                            tracing::warn!(
                                ?channel,
                                %peer_addr,
                                ?error,
                                "proxy connection rejected because runtime capacity is exhausted"
                            );
                            drop(io);
                            continue;
                        }
                    };
                    let context = ConnectionContext {
                        runtime_epoch: epoch,
                        connection_id: Uuid::new_v4(),
                        channel: channel.clone(),
                        peer_addr,
                        accepted_at: self.clock.now(),
                        tls_peer: None,
                    };
                    let service = self.clone();
                    let child_cancel = cancellation.child_token();
                    connections.spawn(async move {
                        let _permit = permit;
                        service.run_connection(io, context, child_cancel).await;
                    });
                }
                Some(joined) = connections.join_next(), if !connections.is_empty() => {
                    if let Err(error) = joined {
                        tracing::warn!(?error, "proxy connection task failed");
                    }
                }
            }
        }
        cancellation.cancel();
        while connections.join_next().await.is_some() {}
        Ok(())
    }

    async fn run_connection(
        &self,
        io: BoxIo,
        mut context: ConnectionContext,
        cancellation: CancellationToken,
    ) {
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
                self.run_connection_inner(accepted.io, &context, cancellation)
                    .await
            }
            Err(error) => Err(error),
        };
        self.ports.connection_closed(&context, &result).await;
    }
}
