//! Scripted Socket 冻结快照到具体 Relay service 的构建。

use std::{net::SocketAddr, sync::Arc, time::Duration};

use intercept_proxy_application::{AppError, AppResult};
use intercept_proxy_domain::{
    ProxyListener, ProxyWorkspace, SocketDownstreamSecurity, SocketPayloadProcessing,
    SocketRelaySettings, SocketTopology,
};
use intercept_proxy_runtime::{
    SocketDownstreamSecurity as RuntimeDownstreamSecurity, SocketEndpoint,
    SocketLocalResponderConfig, SocketObservationMetadata, SocketPipelineLimits, SocketRelayConfig,
    SocketRelayService,
};

use super::{ListenerRuntimePlanBuilder, PreparedListenerRuntime, runtime_error};
use crate::adapters::listener_runtime::{
    external_relay::{ExternalSocketCapabilityFactoryAdapter, ExternalSocketRuntimeSnapshot},
    scripted_relay::{ScriptedSocketCapabilityFactoryAdapter, pipeline_limits},
    scripted_snapshot::{ScriptedSocketRuntimeSnapshot, ScriptedSocketSecuritySnapshot},
};

impl ListenerRuntimePlanBuilder<'_> {
    pub(super) async fn build_scripted_socket(
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
        let binding = match provider {
            Some(provider) => provider.resolve(&scripted.package).await?,
            None => None,
        };
        if let Some(binding) = binding {
            return self
                .build_external_scripted_socket(workspace, listener, socket, bind_addr, binding)
                .await;
        }
        let security = self
            .scripted_socket_security(workspace, &socket.topology)
            .await?;
        let snapshot = ScriptedSocketRuntimeSnapshot::prepare_async(
            self.adapter,
            workspace,
            listener,
            security,
        )
        .await?
        .ok_or_else(|| {
            AppError::new(
                "SCRIPTED_SOCKET_PLAN_INVALID",
                "Scripted Socket 未能生成不可变运行计划。",
            )
            .entity(listener.id.to_string())
        })?;
        let SocketTopology::Relay(_) = &socket.topology else {
            return self.build_local_responder(workspace, listener, socket, bind_addr, snapshot);
        };
        let ScriptedSocketSecuritySnapshot::Relay(security) = snapshot.security() else {
            return Err(AppError::new(
                "SCRIPTED_SOCKET_PLAN_INVALID",
                "Scripted Relay 快照的传输安全模式不一致。",
            )
            .entity(listener.id.to_string()));
        };
        let security = security.clone();
        self.build_internal_scripted_relay(
            workspace, listener, socket, bind_addr, snapshot, security,
        )
    }

    fn build_internal_scripted_relay(
        &self,
        workspace: &ProxyWorkspace,
        listener: &ProxyListener,
        socket: &SocketRelaySettings,
        bind_addr: SocketAddr,
        snapshot: Arc<ScriptedSocketRuntimeSnapshot>,
        security: intercept_proxy_runtime::SocketRelaySecurity,
    ) -> AppResult<PreparedListenerRuntime> {
        let SocketTopology::Relay(relay) = &socket.topology else {
            unreachable!("relay topology was checked before building the scripted relay")
        };
        let framing_limits = intercept_proxy_protocol_scripting::ProtocolFramingLimits::default();
        let pipeline_limits: SocketPipelineLimits = pipeline_limits(
            snapshot.runtime_limits(),
            framing_limits,
            usize::try_from(socket.runtime_limits.read_chunk_bytes).map_err(|_| {
                runtime_error(
                    listener,
                    "CONFIG_INVALID",
                    "Socket 单次读取字节数超出平台范围".into(),
                )
            })?,
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
        let factory = Arc::new(ScriptedSocketCapabilityFactoryAdapter::new(
            &snapshot,
            listener.id.to_string(),
            framing_limits,
            Self::observation_metadata(workspace, listener),
        ));
        let observer = self.socket_observer(listener, socket)?;
        let service = SocketRelayService::build_scripted_with_observer(
            SocketRelayConfig {
                bind_addr,
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
            factory,
            pipeline_limits,
            observer,
        )
        .map_err(|error| runtime_error(listener, error.code, error.message))?;
        Ok(PreparedListenerRuntime::ScriptedSocket {
            bind_addr,
            snapshot,
            service: Some(Arc::new(service)),
        })
    }

    async fn build_external_scripted_socket(
        &self,
        workspace: &ProxyWorkspace,
        listener: &ProxyListener,
        socket: &SocketRelaySettings,
        bind_addr: SocketAddr,
        binding: crate::adapters::listener_runtime::ExternalSocketPackageBinding,
    ) -> AppResult<PreparedListenerRuntime> {
        let max_frame_bytes = binding.max_frame_bytes();
        let registration = binding.registration();
        let rules = self
            .compile_external_document_rules(workspace, listener, socket, registration)
            .await?;
        let snapshot = Arc::new(ExternalSocketRuntimeSnapshot::new(
            binding,
            rules,
            socket.topology.clone(),
        ));
        let pipeline_limits = SocketPipelineLimits::new(
            max_frame_bytes,
            max_frame_bytes,
            read_chunk_bytes(listener, socket)?,
        )
        .map_err(|error| {
            runtime_error(
                listener,
                error.stable_code(),
                "外部协议包 Frame 资源限制无效".to_owned(),
            )
        })?;
        let observer = self.socket_observer(listener, socket)?;
        let pipeline = self.pipeline(listener)?.ports;
        let service = match &socket.topology {
            SocketTopology::Relay(relay) => {
                let factory = Arc::new(ExternalSocketCapabilityFactoryAdapter::new_with_pipeline(
                    &snapshot,
                    Self::observation_metadata(workspace, listener),
                    Arc::clone(&pipeline),
                ));
                SocketRelayService::build_scripted_with_observer(
                    SocketRelayConfig {
                        bind_addr,
                        upstream: SocketEndpoint {
                            host: relay.upstream.host.clone(),
                            port: relay.upstream.port,
                        },
                        security: self.socket_security(workspace, &relay.security).await?,
                        maximum_connections: socket.maximum_connections,
                        read_chunk_bytes: read_chunk_bytes(listener, socket)?,
                        connect_timeout: Duration::from_millis(listener.connect_timeout_ms),
                        read_timeout: Duration::from_millis(listener.read_timeout_ms),
                        write_timeout: Duration::from_millis(listener.write_timeout_ms),
                    },
                    factory,
                    pipeline_limits,
                    observer,
                )
            }
            SocketTopology::LocalResponder(_) => {
                let ScriptedSocketSecuritySnapshot::LocalResponder { downstream_tls } = self
                    .scripted_socket_security(workspace, &socket.topology)
                    .await?
                else {
                    return Err(AppError::new(
                        "SCRIPTED_SOCKET_PLAN_INVALID",
                        "外部 LocalResponder 传输安全模式不一致。",
                    )
                    .entity(listener.id.to_string()));
                };
                let factory = Arc::new(ExternalSocketCapabilityFactoryAdapter::new_with_pipeline(
                    &snapshot,
                    Self::observation_metadata(workspace, listener),
                    Arc::clone(&pipeline),
                ));
                SocketRelayService::build_local_responder_with_observer(
                    SocketLocalResponderConfig {
                        bind_addr,
                        security: downstream_tls.as_ref().map_or(
                            RuntimeDownstreamSecurity::Tcp,
                            |tls| RuntimeDownstreamSecurity::Tls {
                                downstream_tls: tls.clone(),
                            },
                        ),
                        maximum_connections: socket.maximum_connections,
                        read_chunk_bytes: read_chunk_bytes(listener, socket)?,
                        handshake_timeout: Duration::from_millis(listener.connect_timeout_ms),
                        read_timeout: Duration::from_millis(listener.read_timeout_ms),
                        write_timeout: Duration::from_millis(listener.write_timeout_ms),
                    },
                    factory,
                    pipeline_limits,
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

    async fn compile_external_document_rules(
        &self,
        workspace: &ProxyWorkspace,
        listener: &ProxyListener,
        socket: &SocketRelaySettings,
        registration: &intercept_proxy_package_contract::PackageManifest,
    ) -> AppResult<crate::adapters::listener_runtime::ProtocolDocumentRuleConnectionFactory> {
        let workspace_for_compile = workspace.clone();
        let listener_for_compile = listener.clone();
        let package = registration.package().identity().clone();
        let upstream_schema = registration
            .document()
            .upstream()
            .schema()
            .expect("validated Socket Manifest requires upstream schema")
            .clone();
        let downstream_schema = registration
            .document()
            .downstream()
            .schema()
            .expect("validated Socket Manifest requires downstream schema")
            .clone();
        let topology = socket.topology.clone();
        self.adapter
            .compile_document_rules_on_blocking_owner(move || {
                super::super::scripted_snapshot::compile_document_rules(
                    &workspace_for_compile,
                    &listener_for_compile,
                    &package,
                    &upstream_schema,
                    &downstream_schema,
                    &topology,
                )
            })
            .await
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
        let pipeline_limits = pipeline_limits(
            snapshot.runtime_limits(),
            framing_limits,
            usize::try_from(socket.runtime_limits.read_chunk_bytes).map_err(|_| {
                runtime_error(
                    listener,
                    "CONFIG_INVALID",
                    "Socket 单次读取字节数超出平台范围".into(),
                )
            })?,
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
        let factory = Arc::new(ScriptedSocketCapabilityFactoryAdapter::new(
            &snapshot,
            listener.id.to_string(),
            framing_limits,
            Self::observation_metadata(workspace, listener),
        ));
        let observer = self.socket_observer(listener, socket)?;
        let service = SocketRelayService::build_local_responder_with_observer(
            SocketLocalResponderConfig {
                bind_addr,
                security: downstream_tls.as_ref().map_or(
                    RuntimeDownstreamSecurity::Tcp,
                    |downstream_tls| RuntimeDownstreamSecurity::Tls {
                        downstream_tls: downstream_tls.clone(),
                    },
                ),
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
                // LocalResponder 没有 connect；沿用 Listener 的 connect timeout 作为唯一 App TLS
                // handshake 上限，保证现有配置无需增加一个只对该拓扑生效的隐藏字段。
                handshake_timeout: Duration::from_millis(listener.connect_timeout_ms),
                read_timeout: Duration::from_millis(listener.read_timeout_ms),
                write_timeout: Duration::from_millis(listener.write_timeout_ms),
            },
            factory,
            pipeline_limits,
            observer,
        )
        .map_err(|error| runtime_error(listener, error.code, error.message))?;
        Ok(PreparedListenerRuntime::ScriptedSocket {
            bind_addr,
            snapshot,
            service: Some(Arc::new(service)),
        })
    }

    fn observation_metadata(
        workspace: &ProxyWorkspace,
        listener: &ProxyListener,
    ) -> SocketObservationMetadata {
        SocketObservationMetadata {
            workspace_id: workspace.id.to_string(),
            listener_id: listener.id.to_string(),
        }
    }

    async fn scripted_socket_security(
        &self,
        workspace: &ProxyWorkspace,
        topology: &SocketTopology,
    ) -> AppResult<ScriptedSocketSecuritySnapshot> {
        match topology {
            SocketTopology::Relay(relay) => Ok(ScriptedSocketSecuritySnapshot::Relay(
                self.socket_security(workspace, &relay.security).await?,
            )),
            SocketTopology::LocalResponder(local) => {
                Ok(ScriptedSocketSecuritySnapshot::LocalResponder {
                    downstream_tls: match &local.downstream_security {
                        SocketDownstreamSecurity::Tcp => None,
                        SocketDownstreamSecurity::Tls { downstream_tls } => Some(
                            self.socket_downstream_tls(workspace, downstream_tls)
                                .await?,
                        ),
                    },
                })
            }
        }
    }
}

fn read_chunk_bytes(listener: &ProxyListener, socket: &SocketRelaySettings) -> AppResult<usize> {
    usize::try_from(socket.runtime_limits.read_chunk_bytes).map_err(|_| {
        runtime_error(
            listener,
            "CONFIG_INVALID",
            "Socket 单次读取字节数超出平台范围".into(),
        )
    })
}
