//! Direct Socket runtime assembly for `RemoteServer` and `LocalServer` topologies.

use std::{net::SocketAddr, sync::Arc, time::Duration};

use intercept_proxy_application::AppResult;
use intercept_proxy_domain::{
    ProxyListener, ProxyWorkspace, SocketDownstreamSecurity as DomainSocketDownstreamSecurity,
    SocketPayloadProcessing, SocketRelaySettings, SocketTopology,
};
use intercept_proxy_runtime::{
    SocketDownstreamSecurity as RuntimeSocketDownstreamSecurity, SocketEndpoint,
    SocketLocalResponderConfig, SocketRelayConfig, SocketRelayService,
};

use super::{ListenerRuntimePlanBuilder, PreparedListenerRuntime, runtime_error};

impl ListenerRuntimePlanBuilder<'_> {
    pub(super) fn build_socket(
        &self,
        workspace: &ProxyWorkspace,
        listener: &ProxyListener,
        socket: &SocketRelaySettings,
        bind_addr: SocketAddr,
        full_runtime: bool,
    ) -> AppResult<PreparedListenerRuntime> {
        if !full_runtime {
            return self.build_socket_probe(workspace, listener, socket, bind_addr);
        }
        if matches!(&socket.processing, SocketPayloadProcessing::Scripted(_)) {
            return self.build_scripted_socket(workspace, listener, socket, bind_addr);
        }
        let observer = self.socket_observer(listener, socket)?;
        let read_chunk_bytes =
            usize::try_from(socket.runtime_limits.read_chunk_bytes).map_err(|_| {
                runtime_error(
                    listener,
                    "CONFIG_INVALID",
                    "Socket 单次读取字节数超出平台范围".into(),
                )
            })?;
        let service = match &socket.topology {
            SocketTopology::Relay(relay) => SocketRelayService::build_with_observer(
                SocketRelayConfig {
                    bind_addr,
                    allowed_client_cidrs: listener.allowed_client_cidrs.clone(),
                    upstream: SocketEndpoint {
                        host: relay.upstream.host.clone(),
                        port: relay.upstream.port,
                    },
                    security: self.socket_security(workspace, &relay.security)?,
                    maximum_connections: socket.maximum_connections,
                    read_chunk_bytes,
                    connect_timeout: Duration::from_millis(listener.connect_timeout_ms),
                    read_timeout: Duration::from_millis(listener.read_timeout_ms),
                    write_timeout: Duration::from_millis(listener.write_timeout_ms),
                },
                observer.clone(),
            ),
            SocketTopology::LocalResponder(local) => {
                let security = match &local.downstream_security {
                    DomainSocketDownstreamSecurity::Tcp => RuntimeSocketDownstreamSecurity::Tcp,
                    DomainSocketDownstreamSecurity::Tls { downstream_tls } => {
                        RuntimeSocketDownstreamSecurity::Tls {
                            downstream_tls: self
                                .socket_downstream_tls(workspace, downstream_tls)?,
                        }
                    }
                };
                SocketRelayService::build_local_raw_responder_with_observer(
                    SocketLocalResponderConfig {
                        bind_addr,
                        allowed_client_cidrs: listener.allowed_client_cidrs.clone(),
                        security,
                        maximum_connections: socket.maximum_connections,
                        read_chunk_bytes,
                        handshake_timeout: Duration::from_millis(listener.connect_timeout_ms),
                        read_timeout: Duration::from_millis(listener.read_timeout_ms),
                        write_timeout: Duration::from_millis(listener.write_timeout_ms),
                    },
                    observer,
                )
            }
        }
        .map_err(|error| runtime_error(listener, error.code, error.message))?;
        Ok(PreparedListenerRuntime::Socket {
            bind_addr,
            service: Arc::new(service),
        })
    }
}
