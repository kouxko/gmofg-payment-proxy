use std::{
    fmt,
    sync::{
        Arc,
        atomic::{AtomicU64, Ordering},
    },
    time::Duration,
};

use intercept_proxy_domain::ExternalPackageRegistration;
use serde::{Serialize, de::DeserializeOwned};
use tokio::{
    io::{AsyncRead, AsyncWrite},
    sync::{Semaphore, mpsc, oneshot, watch},
    task::AbortHandle,
};
use tokio_tungstenite::WebSocketStream;

use super::error::{ExternalPackageConnectionError, ExternalPackageFatalProtocolError};

mod config;
mod lifecycle;
mod protocol;
#[cfg(test)]
mod queue_tests;
pub(super) mod recent_ids;
mod registration;
pub(super) mod response;
mod runtime;
pub use config::ExternalPackageConnectionConfig;
use lifecycle::AbortActorOnDrop;
use protocol::{CallCommand, CancellationOnDrop, ControlCommand, DataCommand};
use runtime::run_actor;

/// 已完成注册、可并发发起 JSON-RPC 调用的连接句柄。
///
/// 每个句柄共享同一个严格并发额度。调用 future 被丢弃时，其 owned permit 会立即释放，并向 actor
/// 发送本地取消标记；协议不发送远端取消 RPC，迟到响应会被静默丢弃。
#[derive(Clone)]
pub struct ExternalPackageClient {
    generation: u64,
    next_sequence: Arc<AtomicU64>,
    commands: mpsc::Sender<DataCommand>,
    controls: mpsc::Sender<ControlCommand>,
    permits: Arc<Semaphore>,
    rpc_timeout: Duration,
    max_logical_frame_bytes: usize,
    max_rpc_message_bytes: usize,
    max_display_message_bytes: usize,
    closed: watch::Receiver<Option<ExternalPackageConnectionError>>,
    actor_abort: AbortHandle,
}

impl fmt::Debug for ExternalPackageClient {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("ExternalPackageClient")
            .field("generation", &self.generation)
            .field("available_permits", &self.permits.available_permits())
            .finish_non_exhaustive()
    }
}

impl ExternalPackageClient {
    /// 返回该 WebSocket actor 的不可复用连接代次。
    #[must_use]
    pub const fn generation(&self) -> u64 {
        self.generation
    }

    /// 返回数据面可接收的最大 JSON-RPC 消息字节数。
    #[must_use]
    pub const fn max_rpc_message_bytes(&self) -> usize {
        self.max_rpc_message_bytes
    }

    /// 返回单次 RPC 的配置期限。
    #[must_use]
    pub const fn rpc_timeout(&self) -> Duration {
        self.rpc_timeout
    }

    /// 返回 Socket Pipeline 的逻辑报文字节上限。
    #[must_use]
    pub const fn max_logical_frame_bytes(&self) -> usize {
        self.max_logical_frame_bytes
    }

    /// 启动单连接 actor，并等待唯一一次 `package.register` 完成。
    pub async fn connect<S>(
        websocket: WebSocketStream<S>,
        generation: u64,
        config: ExternalPackageConnectionConfig,
    ) -> Result<(ExternalPackageRegistration, Self), ExternalPackageConnectionError>
    where
        S: AsyncRead + AsyncWrite + Unpin + Send + 'static,
    {
        // Calls and best-effort cancellation share a bounded data queue. Terminal control uses a
        // separate bounded channel so data saturation can never discard close or fatal signals.
        let (commands, command_rx) = mpsc::channel(config.max_in_flight.saturating_mul(2));
        let (controls, control_rx) = mpsc::channel(2);
        let (registration_tx, registration_rx) = oneshot::channel();
        let (closed_tx, closed) = watch::channel(None);
        let permits = Arc::new(Semaphore::new(config.max_in_flight));
        let actor = tokio::spawn(run_actor(
            websocket,
            generation,
            config.clone(),
            command_rx,
            control_rx,
            registration_tx,
            closed_tx,
        ));
        let actor_abort = actor.abort_handle();
        let mut attempt_guard = AbortActorOnDrop::new(actor_abort.clone());
        let client = Self {
            generation,
            next_sequence: Arc::new(AtomicU64::new(1)),
            commands,
            controls,
            permits,
            rpc_timeout: config.rpc_timeout,
            max_logical_frame_bytes: config.max_logical_frame_bytes,
            max_rpc_message_bytes: config
                .max_rpc_message_bytes
                .min(config.registration_websocket_message_bytes()),
            max_display_message_bytes: config
                .max_display_message_bytes
                .min(config.registration_websocket_message_bytes()),
            closed,
            actor_abort,
        };
        let registration = registration_rx
            .await
            .map_err(|_| ExternalPackageConnectionError::Disconnected)??;
        attempt_guard.disarm();
        Ok((registration, client))
    }

    /// 发起普通处理 RPC；容量满时立即返回 [`ExternalPackageConnectionError::Busy`]。
    pub async fn call<P, R>(
        &self,
        method: impl Into<String>,
        params: &P,
    ) -> Result<R, ExternalPackageConnectionError>
    where
        P: Serialize + ?Sized,
        R: DeserializeOwned,
    {
        self.call_with_limit(method.into(), params, self.max_rpc_message_bytes)
            .await
    }

    /// 发起 display RPC，并使用独立的 128 KiB 默认响应限制。
    pub async fn call_display<P, R>(
        &self,
        method: impl Into<String>,
        params: &P,
    ) -> Result<R, ExternalPackageConnectionError>
    where
        P: Serialize + ?Sized,
        R: DeserializeOwned,
    {
        self.call_with_limit(method.into(), params, self.max_display_message_bytes)
            .await
    }

    async fn call_with_limit<P, R>(
        &self,
        method: String,
        params: &P,
        response_limit: usize,
    ) -> Result<R, ExternalPackageConnectionError>
    where
        P: Serialize + ?Sized,
        R: DeserializeOwned,
    {
        let permit = Arc::clone(&self.permits)
            .try_acquire_owned()
            .map_err(|_| ExternalPackageConnectionError::Busy)?;
        let sequence = self.next_sequence.fetch_add(1, Ordering::Relaxed);
        let request_id = format!("g{}-c{sequence}", self.generation);
        let params = serde_json::to_value(params)
            .map_err(|error| ExternalPackageConnectionError::InvalidPayload(error.to_string()))?;
        let (response_tx, response_rx) = oneshot::channel();
        self.commands
            .try_send(DataCommand::Call(CallCommand {
                request_id: request_id.clone(),
                method: method.clone(),
                params,
                response_limit,
                response: response_tx,
            }))
            .map_err(|error| match error {
                mpsc::error::TrySendError::Full(_) => ExternalPackageConnectionError::Busy,
                mpsc::error::TrySendError::Closed(_) => {
                    ExternalPackageConnectionError::Disconnected
                }
            })?;
        let mut cancellation = CancellationOnDrop::new(request_id.clone(), self.commands.clone());
        let reply = match tokio::time::timeout(self.rpc_timeout, response_rx).await {
            Ok(Ok(reply)) => reply?,
            Ok(Err(_)) => return Err(ExternalPackageConnectionError::Disconnected),
            Err(_) => {
                return Err(ExternalPackageConnectionError::Timeout { request_id, method });
            }
        };
        cancellation.complete();
        drop(permit);
        let Ok(value) = serde_json::from_value(reply) else {
            let kind = ExternalPackageFatalProtocolError::InvalidResponse;
            let _ = self
                .controls
                .send(ControlCommand::ProtocolFatal(kind))
                .await;
            return Err(ExternalPackageConnectionError::Fatal(kind));
        };
        Ok(value)
    }

    /// 等待连接终止并返回 actor 记录的关闭原因。
    pub async fn wait_closed(&mut self) -> ExternalPackageConnectionError {
        loop {
            if let Some(reason) = self.closed.borrow().clone() {
                return reason;
            }
            if self.closed.changed().await.is_err() {
                return ExternalPackageConnectionError::Disconnected;
            }
        }
    }

    /// 主动关闭连接，并等待 actor 完成 WebSocket close 写出及本地清理。
    ///
    /// 所有 clone 共享同一 actor；重复调用或 actor 已停止都视为成功。仅丢弃某一个 clone 不会关闭连接，
    /// 注册表必须在移除在线连接前显式调用本方法。
    pub async fn disconnect(&self) {
        let mut closed = self.closed.clone();
        let _ = self.controls.send(ControlCommand::Close).await;
        let closed_actor = async {
            loop {
                if closed.borrow().is_some() {
                    return;
                }
                if closed.changed().await.is_err() {
                    return;
                }
            }
        };
        if tokio::time::timeout(Duration::from_secs(5), closed_actor)
            .await
            .is_err()
        {
            // 对端停止读取时 WebSocket close/flush 可能永久阻塞。注册表关闭是资源回收门禁，
            // 到达本地期限后必须终止 actor，使 socket、pending 调用和 online 投影可继续收敛。
            self.actor_abort.abort();
        }
    }
}
