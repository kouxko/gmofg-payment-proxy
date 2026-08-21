//! Scripted Socket 冻结快照到具体 Relay service 的构建。

use std::{net::SocketAddr, sync::Arc, time::Duration};

use intercept_proxy_application::{AppError, AppResult};
use intercept_proxy_domain::{
    ProxyListener, ProxyWorkspace, SocketDownstreamSecurity, SocketPayloadProcessing,
    SocketRelaySettings, SocketTopology,
};
use intercept_proxy_runtime::{
    SocketDownstreamSecurity as RuntimeDownstreamSecurity, SocketEndpoint, SocketFramePumpLimits,
    SocketLocalResponderConfig, SocketRelayConfig, SocketRelayService,
};

use super::{ListenerRuntimePlanBuilder, PreparedListenerRuntime, runtime_error};
use crate::adapters::listener_runtime::{
    external_local_responder::ExternalLocalResponderProcessorFactoryAdapter,
    external_relay::{ExternalRelayProcessorFactoryAdapter, ExternalSocketRuntimeSnapshot},
    local_responder::{LocalResponderProcessorFactoryAdapter, local_frame_pump_limits},
    scripted_relay::{ScriptedRelayProcessorFactoryAdapter, frame_pump_limits},
    scripted_snapshot::{ScriptedSocketRuntimeSnapshot, ScriptedSocketSecuritySnapshot},
    socket_capture_publisher::SocketCaptureContext,
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
        let SocketPayloadProcessing::Scripted(scripted) = &socket.processing else {
            return Err(AppError::new(
                "SCRIPTED_SOCKET_PLAN_INVALID",
                "Scripted Socket 缺少协议包绑定。",
            )
            .entity(listener.id.to_string()));
        };
        let provider = self.adapter.external_package_provider.read().clone();
        if let Some(binding) = provider
            .as_ref()
            .map(|provider| provider.resolve(&scripted.package))
            .transpose()?
            .flatten()
        {
            return self
                .build_external_scripted_socket(workspace, listener, socket, bind_addr, binding);
        }
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
            return self.build_local_responder(workspace, listener, socket, bind_addr, snapshot);
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
            self.capture_context(workspace, listener),
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

    fn build_external_scripted_socket(
        &self,
        workspace: &ProxyWorkspace,
        listener: &ProxyListener,
        socket: &SocketRelaySettings,
        bind_addr: SocketAddr,
        binding: crate::adapters::listener_runtime::ExternalSocketPackageBinding,
    ) -> AppResult<PreparedListenerRuntime> {
        let max_frame_bytes = binding.max_frame_bytes();
        let rpc_timeout = binding.rpc_timeout();
        let registration = binding.registration();
        let package = registration.package().identity();
        let rules = super::super::scripted_snapshot::compile_document_rules(
            workspace,
            listener,
            package,
            registration.document().upstream().schema(),
            registration.document().downstream().schema(),
            &socket.topology,
        )?;
        let snapshot = Arc::new(ExternalSocketRuntimeSnapshot::new(
            binding,
            rules,
            socket.topology.clone(),
        ));
        let pump_limits = SocketFramePumpLimits::new(
            max_frame_bytes,
            max_frame_bytes,
            (16 * 1024).min(max_frame_bytes),
            rpc_timeout,
        )
        .map_err(|error| {
            runtime_error(
                listener,
                error.stable_code(),
                "外部协议包 Frame 资源限制无效".to_owned(),
            )
        })?;
        let observer = Arc::new(SocketDiagnosticObserver::new(
            self.adapter.socket_diagnostic_events.read().clone(),
        ));
        let service = match &socket.topology {
            SocketTopology::Relay(relay) => {
                let factory = Arc::new(ExternalRelayProcessorFactoryAdapter::new(
                    &snapshot,
                    self.capture_context(workspace, listener),
                ));
                SocketRelayService::build_scripted_with_observer(
                    SocketRelayConfig {
                        bind_addr,
                        allowed_client_cidrs: listener.allowed_client_cidrs.clone(),
                        upstream: SocketEndpoint {
                            host: relay.upstream.host.clone(),
                            port: relay.upstream.port,
                        },
                        security: self.socket_security(workspace, &relay.security)?,
                        maximum_connections: socket.maximum_connections,
                        connect_timeout: Duration::from_millis(listener.connect_timeout_ms),
                        read_timeout: Duration::from_millis(listener.read_timeout_ms),
                        write_timeout: Duration::from_millis(listener.write_timeout_ms),
                    },
                    factory,
                    pump_limits,
                    observer,
                )
            }
            SocketTopology::LocalResponder(_) => {
                let ScriptedSocketSecuritySnapshot::LocalResponder { downstream_tls } =
                    self.scripted_socket_security(workspace, &socket.topology)?
                else {
                    return Err(AppError::new(
                        "SCRIPTED_SOCKET_PLAN_INVALID",
                        "外部 LocalResponder 传输安全模式不一致。",
                    )
                    .entity(listener.id.to_string()));
                };
                let factory = Arc::new(ExternalLocalResponderProcessorFactoryAdapter::new(
                    Arc::clone(&snapshot),
                    self.capture_context(workspace, listener),
                ));
                SocketRelayService::build_local_responder_with_observer(
                    SocketLocalResponderConfig {
                        bind_addr,
                        allowed_client_cidrs: listener.allowed_client_cidrs.clone(),
                        security: downstream_tls.as_ref().map_or(
                            RuntimeDownstreamSecurity::Tcp,
                            |tls| RuntimeDownstreamSecurity::Tls {
                                downstream_tls: tls.clone(),
                            },
                        ),
                        maximum_connections: socket.maximum_connections,
                        handshake_timeout: Duration::from_millis(listener.connect_timeout_ms),
                        read_timeout: Duration::from_millis(listener.read_timeout_ms),
                        write_timeout: Duration::from_millis(listener.write_timeout_ms),
                    },
                    factory,
                    pump_limits,
                    observer,
                )
            }
        }
        .map_err(|error| runtime_error(listener, error.code, error.message))?;
        Ok(PreparedListenerRuntime::ExternalScriptedSocket {
            bind_addr,
            snapshot,
            service: Arc::new(service),
        })
    }

    fn build_local_responder(
        &self,
        workspace: &ProxyWorkspace,
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
            self.capture_context(workspace, listener),
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

    fn capture_context(
        &self,
        workspace: &ProxyWorkspace,
        listener: &ProxyListener,
    ) -> SocketCaptureContext {
        SocketCaptureContext {
            workspace_id: workspace.id,
            listener_id: listener.id,
            publisher: self.adapter.socket_capture_publisher.read().clone(),
        }
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
