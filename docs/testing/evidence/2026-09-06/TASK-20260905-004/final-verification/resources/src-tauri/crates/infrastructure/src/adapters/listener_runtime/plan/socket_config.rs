//! Socket runtime endpoint configuration shared by Direct and Scripted processing.

use std::{net::SocketAddr, sync::Arc, time::Duration};

use intercept_proxy_application::AppResult;
use intercept_proxy_domain::{
    ProxyListener, ProxyWorkspace, SocketDownstreamSecurity, SocketRelaySettings, SocketTopology,
};
use intercept_proxy_runtime::{
    SocketDownstreamSecurity as RuntimeDownstreamSecurity, SocketEndpoint,
    SocketLocalResponderConfig, SocketRelayConfig,
};

use super::{
    super::socket_diagnostics::SocketDiagnosticObserver, ListenerRuntimePlanBuilder, runtime_error,
};

pub(super) enum SocketServiceConfig {
    Relay(SocketRelayConfig),
    LocalResponder(SocketLocalResponderConfig),
}

struct SocketCommonConfig {
    bind_addr: SocketAddr,
    maximum_connections: u16,
    read_chunk_bytes: usize,
    connect_timeout: Duration,
    read_timeout: Duration,
    write_timeout: Duration,
}

impl SocketCommonConfig {
    fn new(
        listener: &ProxyListener,
        socket: &SocketRelaySettings,
        bind_addr: SocketAddr,
    ) -> AppResult<Self> {
        Ok(Self {
            bind_addr,
            maximum_connections: socket.maximum_connections,
            read_chunk_bytes: read_chunk_bytes(listener, socket)?,
            connect_timeout: Duration::from_millis(listener.connect_timeout_ms),
            read_timeout: Duration::from_millis(listener.read_timeout_ms),
            write_timeout: Duration::from_millis(listener.write_timeout_ms),
        })
    }

    fn relay(
        &self,
        upstream: &intercept_proxy_domain::SocketEndpoint,
        security: intercept_proxy_runtime::SocketRelaySecurity,
    ) -> SocketRelayConfig {
        SocketRelayConfig {
            bind_addr: self.bind_addr,
            upstream: SocketEndpoint {
                host: upstream.host.clone(),
                port: upstream.port,
            },
            security,
            maximum_connections: self.maximum_connections,
            read_chunk_bytes: self.read_chunk_bytes,
            connect_timeout: self.connect_timeout,
            read_timeout: self.read_timeout,
            write_timeout: self.write_timeout,
        }
    }

    fn local_responder(&self, security: RuntimeDownstreamSecurity) -> SocketLocalResponderConfig {
        SocketLocalResponderConfig {
            bind_addr: self.bind_addr,
            security,
            maximum_connections: self.maximum_connections,
            read_chunk_bytes: self.read_chunk_bytes,
            handshake_timeout: self.connect_timeout,
            read_timeout: self.read_timeout,
            write_timeout: self.write_timeout,
        }
    }
}

pub(super) fn read_chunk_bytes(
    listener: &ProxyListener,
    socket: &SocketRelaySettings,
) -> AppResult<usize> {
    usize::try_from(socket.runtime_limits.read_chunk_bytes).map_err(|_| {
        runtime_error(
            listener,
            "CONFIG_INVALID",
            "Socket 单次读取字节数超出平台范围".into(),
        )
    })
}

impl ListenerRuntimePlanBuilder<'_> {
    pub(super) fn socket_observer(
        &self,
        listener: &ProxyListener,
        socket: &SocketRelaySettings,
    ) -> AppResult<Arc<SocketDiagnosticObserver>> {
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
        SocketDiagnosticObserver::new(
            self.adapter.socket_diagnostic_events.read().clone(),
            capacity,
            max_logical_bytes,
        )
        .map(Arc::new)
        .map_err(|error| runtime_error(listener, error.code, error.message))
    }

    pub(super) async fn socket_service_config(
        &self,
        workspace: &ProxyWorkspace,
        listener: &ProxyListener,
        socket: &SocketRelaySettings,
        bind_addr: SocketAddr,
    ) -> AppResult<SocketServiceConfig> {
        let common = SocketCommonConfig::new(listener, socket, bind_addr)?;
        match &socket.topology {
            SocketTopology::Relay(relay) => Ok(SocketServiceConfig::Relay(common.relay(
                &relay.upstream,
                self.socket_security(workspace, &relay.security).await?,
            ))),
            SocketTopology::LocalResponder(local) => {
                let security = match &local.downstream_security {
                    SocketDownstreamSecurity::Tcp => RuntimeDownstreamSecurity::Tcp,
                    SocketDownstreamSecurity::Tls { downstream_tls } => {
                        RuntimeDownstreamSecurity::Tls {
                            downstream_tls: self
                                .socket_downstream_tls(workspace, downstream_tls)
                                .await?,
                        }
                    }
                };
                Ok(SocketServiceConfig::LocalResponder(
                    common.local_responder(security),
                ))
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use intercept_proxy_domain::{
        SocketDownstreamSecurity, SocketLocalResponderTopology, SocketPayloadProcessing,
        SocketRelaySettings, SocketRuntimeLimits, SocketTopology,
    };

    use super::*;

    #[test]
    fn common_socket_settings_map_identically_to_relay_and_local_configs() {
        let listener = ProxyListener {
            connect_timeout_ms: 101,
            read_timeout_ms: 202,
            write_timeout_ms: 303,
            ..ProxyListener::default()
        };
        let socket = SocketRelaySettings {
            topology: SocketTopology::LocalResponder(SocketLocalResponderTopology {
                downstream_security: SocketDownstreamSecurity::Tcp,
            }),
            maximum_connections: 17,
            runtime_limits: SocketRuntimeLimits {
                read_chunk_bytes: 4097,
                ..SocketRuntimeLimits::default()
            },
            processing: SocketPayloadProcessing::Direct,
        };
        let bind_addr = "127.0.0.1:19090".parse().unwrap();
        let common = SocketCommonConfig::new(&listener, &socket, bind_addr).unwrap();
        let relay = common.relay(
            &intercept_proxy_domain::SocketEndpoint {
                host: "upstream.example".into(),
                port: 443,
            },
            intercept_proxy_runtime::SocketRelaySecurity::Transparent,
        );
        let local = common.local_responder(RuntimeDownstreamSecurity::Tcp);

        assert_eq!(relay.bind_addr, local.bind_addr);
        assert_eq!(relay.maximum_connections, local.maximum_connections);
        assert_eq!(relay.read_chunk_bytes, local.read_chunk_bytes);
        assert_eq!(relay.connect_timeout, local.handshake_timeout);
        assert_eq!(relay.read_timeout, local.read_timeout);
        assert_eq!(relay.write_timeout, local.write_timeout);
        assert_eq!(relay.upstream.host, "upstream.example");
        assert_eq!(relay.upstream.port, 443);
        assert!(matches!(
            relay.security,
            intercept_proxy_runtime::SocketRelaySecurity::Transparent
        ));
        assert!(matches!(local.security, RuntimeDownstreamSecurity::Tcp));
    }
}
