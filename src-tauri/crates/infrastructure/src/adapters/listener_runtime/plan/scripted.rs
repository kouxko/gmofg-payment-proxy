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
use crate::adapters::listener_runtime::external_relay::{
    ExternalSocketCapabilityFactoryAdapter, ExternalSocketRuntimeSnapshot,
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
        let provider = provider.ok_or_else(|| {
            AppError::new(
                "EXTERNAL_PACKAGE_PROVIDER_MISSING",
                "Socket 协议包注册表尚未装配。",
            )
        })?;
        let binding = provider.resolve(&scripted.package).await?.ok_or_else(|| {
            AppError::new(
                "EXTERNAL_PACKAGE_NOT_FOUND",
                "Socket 协议包未在统一注册表中找到。",
            )
            .entity(format!(
                "{}@{}",
                scripted.package.id, scripted.package.version
            ))
        })?;
        self.build_external_scripted_socket(workspace, listener, socket, bind_addr, binding)
            .await
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
            SocketTopology::LocalResponder(local) => {
                let downstream_tls = match &local.downstream_security {
                    SocketDownstreamSecurity::Tcp => None,
                    SocketDownstreamSecurity::Tls { downstream_tls } => Some(
                        self.socket_downstream_tls(workspace, downstream_tls)
                            .await?,
                    ),
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
                super::super::document_rules::compile_document_rules(
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

    fn observation_metadata(
        workspace: &ProxyWorkspace,
        listener: &ProxyListener,
    ) -> SocketObservationMetadata {
        SocketObservationMetadata {
            workspace_id: workspace.id.to_string(),
            listener_id: listener.id.to_string(),
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
