//! Scripted Socket 冻结快照到具体 Relay service 的构建。

use std::{net::SocketAddr, sync::Arc, time::Duration};

use intercept_proxy_application::{AppError, AppResult};
use intercept_proxy_domain::{
    ProxyListener, ProxyWorkspace, SocketDownstreamSecurity, SocketRelaySettings, SocketTopology,
};
use intercept_proxy_runtime::{
    SocketDownstreamSecurity as RuntimeDownstreamSecurity, SocketEndpoint, SocketFramePumpLimits,
    SocketLocalResponderConfig, SocketRelayConfig, SocketRelayService,
};

use super::{ListenerRuntimePlanBuilder, PreparedListenerRuntime, runtime_error};
use crate::adapters::listener_runtime::{
    local_responder::{LocalResponderProcessorFactoryAdapter, local_frame_pump_limits},
    scripted_relay::{ScriptedRelayProcessorFactoryAdapter, frame_pump_limits},
    scripted_snapshot::{ScriptedSocketRuntimeSnapshot, ScriptedSocketSecuritySnapshot},
    socket_diagnostics::SocketDiagnosticObserver,
};

impl ListenerRuntimePlanBuilder<'_> {
    pub(super) fn build_scripted_socket(
        &self,
        workspace: &ProxyWorkspace,
        listener: &ProxyListener,
        socket: &SocketRelaySettings,
        bind_addr: SocketAddr,
    ) -> AppResult<PreparedListenerRuntime> {
        let security = self.scripted_socket_security(workspace, &socket.topology)?;
        let snapshot =
            ScriptedSocketRuntimeSnapshot::prepare(self.adapter, workspace, listener, security)?
                .ok_or_else(|| {
                    AppError::new(
                        "SCRIPTED_SOCKET_PLAN_INVALID",
                        "Scripted Socket 未能生成不可变运行计划。",
                    )
                    .entity(listener.id.to_string())
                })?;
        let SocketTopology::Relay(relay) = &socket.topology else {
            return self.build_local_responder(listener, socket, bind_addr, snapshot);
        };
        let ScriptedSocketSecuritySnapshot::Relay(security) = snapshot.security() else {
            return Err(AppError::new(
                "SCRIPTED_SOCKET_PLAN_INVALID",
                "Scripted Relay 快照的传输安全模式不一致。",
            )
            .entity(listener.id.to_string()));
        };
        let framing_limits = intercept_proxy_protocol_scripting::ProtocolFramingLimits::default();
        let pump_limits: SocketFramePumpLimits = frame_pump_limits(
            snapshot.runtime_limits(),
            framing_limits,
            snapshot.upstream(),
            snapshot.downstream(),
        )
        .map_err(|error| {
            runtime_error(
                listener,
                error.stable_code(),
                "脚本 Frame 资源限制无效".to_owned(),
            )
        })?;
        let factory = Arc::new(ScriptedRelayProcessorFactoryAdapter::new(
            &snapshot,
            listener.id.to_string(),
            framing_limits,
        ));
        let observer = Arc::new(SocketDiagnosticObserver::new(
            self.adapter.socket_diagnostic_events.read().clone(),
        ));
        let service = SocketRelayService::build_scripted_with_observer(
            SocketRelayConfig {
                bind_addr,
                allowed_client_cidrs: listener.allowed_client_cidrs.clone(),
                upstream: SocketEndpoint {
                    host: relay.upstream.host.clone(),
                    port: relay.upstream.port,
                },
                security: security.clone(),
                maximum_connections: socket.maximum_connections,
                connect_timeout: Duration::from_millis(listener.connect_timeout_ms),
                read_timeout: Duration::from_millis(listener.read_timeout_ms),
                write_timeout: Duration::from_millis(listener.write_timeout_ms),
            },
            factory,
            pump_limits,
            observer,
        )
        .map_err(|error| runtime_error(listener, error.code, error.message))?;
        Ok(PreparedListenerRuntime::ScriptedSocket {
            bind_addr,
            snapshot,
            service: Some(Arc::new(service)),
        })
    }

    fn build_local_responder(
        &self,
        listener: &ProxyListener,
        socket: &SocketRelaySettings,
        bind_addr: SocketAddr,
        snapshot: Arc<ScriptedSocketRuntimeSnapshot>,
    ) -> AppResult<PreparedListenerRuntime> {
        let ScriptedSocketSecuritySnapshot::LocalResponder { downstream_tls } = snapshot.security()
        else {
            return Err(AppError::new(
                "SCRIPTED_SOCKET_PLAN_INVALID",
                "LocalResponder 快照的传输安全模式不一致。",
            )
            .entity(listener.id.to_string()));
        };
        let framing_limits = intercept_proxy_protocol_scripting::ProtocolFramingLimits::default();
        let pump_limits = local_frame_pump_limits(
            snapshot.runtime_limits(),
            framing_limits,
            snapshot.upstream(),
            snapshot.downstream(),
        )
        .map_err(|error| {
            runtime_error(
                listener,
                error.stable_code(),
                "LocalResponder 脚本资源限制无效".to_owned(),
            )
        })?;
        let factory = Arc::new(LocalResponderProcessorFactoryAdapter::new(
            &snapshot,
            listener.id.to_string(),
            framing_limits,
        ));
        let observer = Arc::new(SocketDiagnosticObserver::new(
            self.adapter.socket_diagnostic_events.read().clone(),
        ));
        let service = SocketRelayService::build_local_responder_with_observer(
            SocketLocalResponderConfig {
                bind_addr,
                allowed_client_cidrs: listener.allowed_client_cidrs.clone(),
                security: downstream_tls.as_ref().map_or(
                    RuntimeDownstreamSecurity::Tcp,
                    |downstream_tls| RuntimeDownstreamSecurity::Tls {
                        downstream_tls: downstream_tls.clone(),
                    },
                ),
                maximum_connections: socket.maximum_connections,
                // LocalResponder 没有 connect；沿用 Listener 的 connect timeout 作为唯一 App TLS
                // handshake 上限，保证现有配置无需增加一个只对该拓扑生效的隐藏字段。
                handshake_timeout: Duration::from_millis(listener.connect_timeout_ms),
                read_timeout: Duration::from_millis(listener.read_timeout_ms),
                write_timeout: Duration::from_millis(listener.write_timeout_ms),
            },
            factory,
            pump_limits,
            observer,
        )
        .map_err(|error| runtime_error(listener, error.code, error.message))?;
        Ok(PreparedListenerRuntime::ScriptedSocket {
            bind_addr,
            snapshot,
            service: Some(Arc::new(service)),
        })
    }

    fn scripted_socket_security(
        &self,
        workspace: &ProxyWorkspace,
        topology: &SocketTopology,
    ) -> AppResult<ScriptedSocketSecuritySnapshot> {
        match topology {
            SocketTopology::Relay(relay) => Ok(ScriptedSocketSecuritySnapshot::Relay(
                self.socket_security(workspace, &relay.security)?,
            )),
            SocketTopology::LocalResponder(local) => {
                Ok(ScriptedSocketSecuritySnapshot::LocalResponder {
                    downstream_tls: match &local.downstream_security {
                        SocketDownstreamSecurity::Tcp => None,
                        SocketDownstreamSecurity::Tls { downstream_tls } => {
                            Some(self.socket_downstream_tls(workspace, downstream_tls)?)
                        }
                    },
                })
            }
        }
    }
}
