//! Socket 上游探测计划的无脚本网络配置构造。

use std::{net::SocketAddr, sync::Arc, time::Duration};

use intercept_proxy_application::AppResult;
use intercept_proxy_domain::{ProxyListener, SocketRelaySettings, SocketRelayTopology};
use intercept_proxy_runtime::{
    SocketEndpoint, SocketRelayConfig, SocketRelaySecurity, SocketRelayService,
};

use super::{
    ListenerRuntimeAdapter, PreparedListenerRuntime, plan::runtime_error,
    socket_diagnostics::SocketDiagnosticObserver,
};

pub(super) fn build_probe(
    adapter: &ListenerRuntimeAdapter,
    listener: &ProxyListener,
    socket: &SocketRelaySettings,
    relay: &SocketRelayTopology,
    bind_addr: SocketAddr,
    security: SocketRelaySecurity,
) -> AppResult<PreparedListenerRuntime> {
    let capacity =
        usize::try_from(socket.runtime_limits.diagnostic_event_capacity).map_err(|_| {
            runtime_error(
                listener,
                "CONFIG_INVALID",
                "Socket 诊断事件容量超出平台范围".into(),
            )
        })?;
    let max_logical_bytes = usize::try_from(socket.runtime_limits.diagnostic_memory_bytes)
        .map_err(|_| {
            runtime_error(
                listener,
                "CONFIG_INVALID",
                "Socket 诊断内存容量超出平台范围".into(),
            )
        })?;
    let observer = Arc::new(
        SocketDiagnosticObserver::new(
            adapter.socket_diagnostic_events.read().clone(),
            capacity,
            max_logical_bytes,
        )
        .map_err(|error| runtime_error(listener, error.code, error.message))?,
    );
    let service = SocketRelayService::build_with_observer(
        SocketRelayConfig {
            bind_addr,
            allowed_client_cidrs: listener.allowed_client_cidrs.clone(),
            upstream: SocketEndpoint {
                host: relay.upstream.host.clone(),
                port: relay.upstream.port,
            },
            security,
            maximum_connections: socket.maximum_connections,
            read_chunk_bytes: usize::try_from(socket.runtime_limits.read_chunk_bytes).map_err(
                |_| {
                    runtime_error(
                        listener,
                        "CONFIG_INVALID",
                        "Socket 单次读取字节数超出平台范围".into(),
                    )
                },
            )?,
            connect_timeout: Duration::from_millis(listener.connect_timeout_ms),
            read_timeout: Duration::from_millis(listener.read_timeout_ms),
            write_timeout: Duration::from_millis(listener.write_timeout_ms),
        },
        observer,
    )
    .map_err(|error| runtime_error(listener, error.code, error.message))?;
    Ok(PreparedListenerRuntime::Socket {
        bind_addr,
        service: Arc::new(service),
    })
}
