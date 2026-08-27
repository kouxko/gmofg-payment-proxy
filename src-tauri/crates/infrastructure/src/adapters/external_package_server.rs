//! 外部软件包 WebSocket 服务的进程级生命周期。
//!
//! 本模块只负责 TCP 接受循环、注册 actor 接线和断线联动。JSON-RPC 协议状态机位于
//! `external_packages`，SQLite/在线身份门禁位于 `external_package_registry`，避免一个对象
//! 同时持有网络协议、持久化事务和 Listener 运行时规则。

use std::{net::SocketAddr, sync::Arc, time::Duration};

use intercept_proxy_application::{
    AppResult, ListenerId, ListenerRuntimePort, ListenerRuntimeState, ListenerStatusViewModel,
    ProtocolPackageUsageQueryPort,
};
use parking_lot::Mutex;
use tokio::{net::TcpListener, sync::Semaphore, task::JoinHandle};
use tokio_util::sync::CancellationToken;
use uuid::Uuid;

use super::{
    AcceptedExternalPackageConnection, ExternalPackageClient, ExternalPackageConnectionConfig,
    ExternalPackageConnectionError, ExternalPackageRegistryAdapter,
    external_package_registration_fingerprint,
};

const WEBSOCKET_HANDSHAKE_TIMEOUT: Duration = Duration::from_secs(10);
const MAX_ACCEPTED_CONNECTIONS: usize = 256;

#[async_trait::async_trait]
/// External-package cleanup extension for exact Listener runtime ownership.
///
/// Implementations must compare `expected_run_token` and remove the matching runtime while
/// holding the same ownership lock. Checking a token before an unconditional `stop` leaves a
/// TOCTOU window and does not satisfy this contract.
pub trait ExternalPackageListenerRuntime: ListenerRuntimePort {
    async fn current_run_token(&self, listener_id: ListenerId) -> Option<Uuid>;

    async fn stop_if_run_token(
        &self,
        listener_id: ListenerId,
        expected_run_token: Uuid,
    ) -> AppResult<Option<ListenerStatusViewModel>>;
}

#[derive(Clone)]
struct ConnectionServices {
    registry: Arc<ExternalPackageRegistryAdapter>,
    usage: Arc<dyn ProtocolPackageUsageQueryPort>,
    listener_runtime: Arc<dyn ExternalPackageListenerRuntime>,
}

/// 固定路径 `/packages` 的外部软件包服务启动配置。
#[derive(Clone, Debug)]
pub struct ExternalPackageServerConfig {
    pub bind_address: SocketAddr,
    pub connection: ExternalPackageConnectionConfig,
}

/// Host 持有的外部软件包服务任务。
///
/// 绑定失败时 `task` 为空且失败原因已经写入注册表状态；这不是 Host 构建失败条件。
#[derive(Debug)]
pub struct ExternalPackageServer {
    cancellation: CancellationToken,
    task: Mutex<Option<JoinHandle<()>>>,
}

impl ExternalPackageServer {
    /// 尝试绑定配置地址并启动接受循环。
    ///
    /// 端口占用等绑定错误会被投影到设置页状态，返回的空服务句柄仍可安全参与统一关闭。
    pub async fn start(
        config: ExternalPackageServerConfig,
        registry: Arc<ExternalPackageRegistryAdapter>,
        usage: Arc<dyn ProtocolPackageUsageQueryPort>,
        listener_runtime: Arc<dyn ExternalPackageListenerRuntime>,
    ) -> Self {
        let websocket_url = format!("ws://{}/packages", config.bind_address);
        let cancellation = CancellationToken::new();
        let listener = match TcpListener::bind(config.bind_address).await {
            Ok(listener) => listener,
            Err(error) => {
                registry
                    .mark_service_failed(websocket_url, error.to_string())
                    .await;
                return Self {
                    cancellation,
                    task: Mutex::new(None),
                };
            }
        };
        registry.mark_service_listening(websocket_url).await;
        let task_cancellation = cancellation.clone();
        let task = tokio::spawn(async move {
            run_accept_loop(
                listener,
                config.connection,
                registry,
                usage,
                listener_runtime,
                task_cancellation,
            )
            .await;
        });
        Self {
            cancellation,
            task: Mutex::new(Some(task)),
        }
    }

    /// 取消接受循环并等待所有已纳管连接任务结束。
    pub async fn shutdown(&self) {
        self.cancel();
        let task = self.task.lock().take();
        if let Some(task) = task
            && let Err(error) = task.await
            && !error.is_cancelled()
        {
            tracing::error!(
                ?error,
                "external package server task failed during shutdown"
            );
        }
    }

    /// 请求停止接受新连接；等待任务回收由 [`Self::shutdown`] 完成。
    pub fn cancel(&self) {
        self.cancellation.cancel();
    }
}

impl Drop for ExternalPackageServer {
    fn drop(&mut self) {
        self.cancellation.cancel();
        if let Some(task) = self.task.get_mut().take() {
            task.abort();
        }
    }
}

async fn run_accept_loop(
    listener: TcpListener,
    config: ExternalPackageConnectionConfig,
    registry: Arc<ExternalPackageRegistryAdapter>,
    usage: Arc<dyn ProtocolPackageUsageQueryPort>,
    listener_runtime: Arc<dyn ExternalPackageListenerRuntime>,
    cancellation: CancellationToken,
) {
    let mut connections = tokio::task::JoinSet::new();
    let admission = Arc::new(Semaphore::new(MAX_ACCEPTED_CONNECTIONS));
    let services = ConnectionServices {
        registry,
        usage,
        listener_runtime,
    };
    let mut generation = 1_u64;
    loop {
        tokio::select! {
            () = cancellation.cancelled() => break,
            accepted = listener.accept() => match accepted {
                Ok((stream, remote_address)) => {
                    let Some(permit) = try_admit_connection(
                        &admission,
                        services.registry.as_ref(),
                        remote_address,
                    ) else {
                        continue;
                    };
                    let connection_generation = generation;
                    generation = generation.saturating_add(1);
                    connections.spawn(handle_connection(
                        stream,
                        remote_address,
                        connection_generation,
                        config.clone(),
                        services.clone(),
                        cancellation.child_token(),
                        permit,
                    ));
                }
                Err(error) => {
                    tracing::warn!(%error, "external package TCP accept failed");
                }
            },
            completed = connections.join_next(), if !connections.is_empty() => {
                if let Some(Err(error)) = completed
                    && !error.is_cancelled()
                {
                    tracing::error!(?error, "external package connection task panicked");
                }
            }
        }
    }
    // 每个连接任务都收到同一个取消树，并负责显式关闭 actor/WebSocket 及发布离线状态。
    // 直接 abort handler 会遗留 detached actor 和错误的 online 投影。
    while connections.join_next().await.is_some() {}
}

fn try_admit_connection(
    admission: &Arc<Semaphore>,
    registry: &ExternalPackageRegistryAdapter,
    remote_address: SocketAddr,
) -> Option<tokio::sync::OwnedSemaphorePermit> {
    if let Ok(permit) = Arc::clone(admission).try_acquire_owned() {
        Some(permit)
    } else {
        tracing::warn!(%remote_address, "external package connection limit reached");
        registry.record_connection_attempt_failure(
            "connection_admission",
            remote_address,
            "EXTERNAL_PACKAGE_CONNECTION_LIMIT_REACHED",
        );
        None
    }
}

async fn handle_connection(
    stream: tokio::net::TcpStream,
    remote_address: SocketAddr,
    generation: u64,
    config: ExternalPackageConnectionConfig,
    services: ConnectionServices,
    cancellation: CancellationToken,
    _permit: tokio::sync::OwnedSemaphorePermit,
) {
    let handshake =
        super::accept_packages_websocket(stream, config.registration_websocket_message_bytes());
    let websocket = match tokio::select! {
        () = cancellation.cancelled() => return,
        result = tokio::time::timeout(WEBSOCKET_HANDSHAKE_TIMEOUT, handshake) => result,
    } {
        Err(_) => {
            tracing::warn!(%remote_address, "external package WebSocket handshake timed out");
            services.registry.record_connection_attempt_failure(
                "websocket_handshake",
                remote_address,
                "EXTERNAL_PACKAGE_HANDSHAKE_TIMEOUT",
            );
            return;
        }
        Ok(result) => match result {
            Ok(websocket) => websocket,
            Err(error) => {
                tracing::warn!(%remote_address, %error, "external package WebSocket handshake rejected");
                services.registry.record_connection_attempt_failure(
                    "websocket_handshake",
                    remote_address,
                    "EXTERNAL_PACKAGE_HANDSHAKE_REJECTED",
                );
                return;
            }
        },
    };
    let connecting = super::ExternalPackageClient::connect(websocket, generation, config);
    let (registration, client) = match tokio::select! {
        () = cancellation.cancelled() => return,
        connection = connecting => connection,
    } {
        Ok(connection) => connection,
        Err(error) => {
            tracing::warn!(%remote_address, %error, "external package registration failed");
            services
                .registry
                .record_registration_failure(remote_address, &error);
            return;
        }
    };
    let fingerprint = match external_package_registration_fingerprint(&registration) {
        Ok(fingerprint) => fingerprint,
        Err(error) => {
            tracing::error!(code = %error.view_model.code, %remote_address, "external package fingerprint failed");
            services.registry.record_application_failure(
                "fingerprint",
                remote_address,
                Some(registration.package().identity()),
                &error,
            );
            client.disconnect().await;
            return;
        }
    };
    let monitor = client.clone();
    let accepted = match services
        .registry
        .accept_registration(&registration, fingerprint, client)
        .await
    {
        Ok(accepted) => accepted,
        Err(error) => {
            tracing::warn!(code = %error.view_model.code, %remote_address, "external package identity rejected");
            services.registry.record_application_failure(
                "identity",
                remote_address,
                Some(registration.package().identity()),
                &error,
            );
            monitor.disconnect().await;
            return;
        }
    };
    monitor_connection(accepted, monitor, remote_address, &services, &cancellation).await;
}

async fn monitor_connection(
    accepted: AcceptedExternalPackageConnection,
    mut monitor: ExternalPackageClient,
    remote_address: SocketAddr,
    services: &ConnectionServices,
    cancellation: &CancellationToken,
) {
    tracing::info!(
        package_id = %accepted.package.id,
        package_version = %accepted.package.version,
        connection_id = %accepted.connection_id.as_uuid(),
        %remote_address,
        "external package connected"
    );
    persist_remote_address(&services.registry, &accepted, remote_address).await;
    let reason = tokio::select! {
        () = cancellation.cancelled() => {
            monitor.disconnect().await;
            super::ExternalPackageConnectionError::Disconnected
        }
        reason = monitor.wait_closed() => reason,
    };
    persist_connection_error(&services.registry, &accepted, &reason).await;
    if services
        .registry
        .mark_disconnected(&accepted.package, accepted.connection_id)
        .await
    {
        tracing::warn!(
            package_id = %accepted.package.id,
            package_version = %accepted.package.version,
            connection_id = %accepted.connection_id.as_uuid(),
            %remote_address,
            %reason,
            "external package disconnected"
        );
        stop_exact_package_listeners(
            &accepted.package,
            accepted.connection_id,
            services.registry.as_ref(),
            services.usage.as_ref(),
            services.listener_runtime.as_ref(),
        )
        .await;
    }
}

async fn persist_remote_address(
    registry: &ExternalPackageRegistryAdapter,
    accepted: &AcceptedExternalPackageConnection,
    remote_address: SocketAddr,
) {
    if let Err(error) = registry
        .record_remote_address(&accepted.package, accepted.connection_id, remote_address)
        .await
    {
        tracing::error!(
            code = %error.view_model.code,
            package_id = %accepted.package.id,
            package_version = %accepted.package.version,
            "external package remote address persistence failed"
        );
    }
}

async fn persist_connection_error(
    registry: &ExternalPackageRegistryAdapter,
    accepted: &AcceptedExternalPackageConnection,
    reason: &ExternalPackageConnectionError,
) {
    if let Err(error) = registry
        .record_connection_error(&accepted.package, accepted.connection_id, reason)
        .await
    {
        tracing::error!(
            code = %error.view_model.code,
            package_id = %accepted.package.id,
            package_version = %accepted.package.version,
            "external package recent error persistence failed"
        );
    }
}

async fn stop_exact_package_listeners(
    package: &intercept_proxy_domain::ProtocolPackageRef,
    disconnected_connection_id: super::ExternalPackageConnectionId,
    registry: &ExternalPackageRegistryAdapter,
    usage: &dyn ProtocolPackageUsageQueryPort,
    listener_runtime: &dyn ExternalPackageListenerRuntime,
) {
    let usages = match usage.usages(package).await {
        Ok(usages) => usages,
        Err(error) => {
            tracing::error!(
                code = %error.view_model.code,
                package_id = %package.id,
                package_version = %package.version,
                "failed to query listeners after external package disconnect"
            );
            registry.record_package_operation_failure(
                "usage_query_after_disconnect",
                package,
                &error,
            );
            return;
        }
    };
    for reference in usages {
        if reference.runtime_state == ListenerRuntimeState::Stopped {
            continue;
        }
        let Some(expected_run_token) = listener_runtime
            .current_run_token(reference.listener_id)
            .await
        else {
            continue;
        };
        // `usages()` 与前一个 Listener 的 `stop()` 都是异步边界。旧连接离线后，同一精确
        // 版本可以在任一边界期间完成重连，并由用户重新启动 Listener。每次停止前必须重新
        // 核验原 connection ID 仍是当前离线代次，不能让旧清理任务作用于新连接的运行代次。
        if !registry
            .is_still_offline_after(package, disconnected_connection_id)
            .await
        {
            tracing::info!(
                package_id = %package.id,
                package_version = %package.version,
                connection_id = %disconnected_connection_id.as_uuid(),
                "skipped stale listener cleanup after external package reconnect"
            );
            break;
        }
        match listener_runtime
            .stop_if_run_token(reference.listener_id, expected_run_token)
            .await
        {
            Ok(Some(_)) => {
                tracing::warn!(
                    listener_id = %reference.listener_id,
                    package_id = %package.id,
                    package_version = %package.version,
                    reason = "external_package_offline",
                    "listener stopped after external package disconnect"
                );
                registry.record_listener_stopped_after_disconnect(package, reference.listener_id);
            }
            Ok(None) => {
                tracing::info!(
                    listener_id = %reference.listener_id,
                    package_id = %package.id,
                    package_version = %package.version,
                    "skipped stale listener runtime cleanup after listener restart"
                );
            }
            Err(error) => {
                tracing::error!(
                    code = %error.view_model.code,
                    listener_id = %reference.listener_id,
                    package_id = %package.id,
                    package_version = %package.version,
                    "failed to stop listener after external package disconnect"
                );
                registry.record_listener_stop_failure(package, reference.listener_id, &error);
            }
        }
    }
}

#[cfg(test)]
#[path = "external_package_server/tests.rs"]
mod tests;
