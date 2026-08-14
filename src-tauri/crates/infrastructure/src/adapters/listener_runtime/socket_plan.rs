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
    let observer = Arc::new(SocketDiagnosticObserver::new(
        adapter.socket_diagnostic_events.read().clone(),
    ));
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
