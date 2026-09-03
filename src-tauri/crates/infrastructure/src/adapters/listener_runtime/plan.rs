//! Validated Workspace listener snapshots mapped to immutable runtime-native plans.

use std::{net::SocketAddr, sync::Arc, time::Duration};

use intercept_proxy_application::{AppError, AppResult};
use intercept_proxy_domain::{
    DownstreamClientAuthentication, HttpListenerSettings, HttpTopology, ListenerDataPlane,
    ProxyListener, ProxyWorkspace, SocketDownstreamTlsSettings,
    SocketRelaySecurity as DomainSocketSecurity, SocketRelaySettings, SocketTopology,
    SocketUpstreamTlsSettings,
};
use intercept_proxy_runtime::{
    ChannelId, DEFAULT_MAX_CONNECTIONS, ForwardAuthenticationMode, ForwardProxyAuthenticator,
    ForwardProxyConfig, ForwardProxyService, HttpProtocolCapabilityFactory, LocalHttpServerConfig,
    LocalHttpServerService, MessageLimits, NoAuthentication, PipelinePorts,
    PlainHttpCapabilityFactory, ReverseProxyConfig, ReverseProxyService, SocketDownstreamTlsConfig,
    SocketRelaySecurity as RuntimeSocketSecurity, SocketRelayService, SocketTlsIdentity,
    SocketUpstreamTlsConfig,
};

use super::{
    ListenerRuntimeAdapter, helpers::ensure_snapshot_matches,
    http_protocol_pipeline::HttpProtocolRuntimeSnapshot, parse_bind_address,
    socket_diagnostics::SocketDiagnosticObserver, socket_plan,
};

mod tls;

mod scripted;
mod socket;

pub(super) enum PreparedListenerRuntime {
    HttpForward {
        bind_addr: SocketAddr,
        service: ForwardProxyService,
        protocol: Option<Arc<HttpProtocolRuntimeSnapshot>>,
    },
    HttpFixed {
        bind_addr: SocketAddr,
        service: Box<ReverseProxyService>,
        protocol: Option<Arc<HttpProtocolRuntimeSnapshot>>,
    },
    HttpLocal {
        bind_addr: SocketAddr,
        service: LocalHttpServerService,
        protocol: Option<Arc<HttpProtocolRuntimeSnapshot>>,
    },
    Socket {
        bind_addr: SocketAddr,
        service: Arc<SocketRelayService>,
    },
    /// 外部协议包 Relay；注册合同和 actor 句柄由 processor factory 冻结持有。
    ExternalScriptedSocket {
        bind_addr: SocketAddr,
        snapshot: Arc<super::external_relay::ExternalSocketRuntimeSnapshot>,
        service: Arc<SocketRelayService>,
    },
}
impl PreparedListenerRuntime {
    pub(super) const fn bind_addr(&self) -> SocketAddr {
        match self {
            Self::HttpForward { bind_addr, .. }
            | Self::HttpFixed { bind_addr, .. }
            | Self::HttpLocal { bind_addr, .. }
            | Self::Socket { bind_addr, .. }
            | Self::ExternalScriptedSocket { bind_addr, .. } => *bind_addr,
        }
    }

    pub(super) fn external_socket_snapshot(
        &self,
    ) -> Option<Arc<super::external_relay::ExternalSocketRuntimeSnapshot>> {
        match self {
            Self::ExternalScriptedSocket { snapshot, .. } => Some(Arc::clone(snapshot)),
            _ => None,
        }
    }

    pub(super) fn http_protocol_snapshot(&self) -> Option<Arc<HttpProtocolRuntimeSnapshot>> {
        match self {
            Self::HttpForward { protocol, .. }
            | Self::HttpFixed { protocol, .. }
            | Self::HttpLocal { protocol, .. } => protocol.clone(),
            Self::Socket { .. } | Self::ExternalScriptedSocket { .. } => None,
        }
    }
}

pub(super) struct ListenerRuntimePlanBuilder<'ctx> {
    adapter: &'ctx ListenerRuntimeAdapter,
}

struct HttpBuildContext<'a> {
    workspace: &'a ProxyWorkspace,
    listener: &'a ProxyListener,
    http: &'a HttpListenerSettings,
    bind_addr: SocketAddr,
    protocol: Option<Arc<HttpProtocolRuntimeSnapshot>>,
    pipeline: Option<Arc<dyn PipelinePorts>>,
    capabilities: Option<Arc<dyn HttpProtocolCapabilityFactory>>,
}

impl<'ctx> ListenerRuntimePlanBuilder<'ctx> {
    pub(super) const fn new(adapter: &'ctx ListenerRuntimeAdapter) -> Self {
        Self { adapter }
    }

    fn socket_observer(
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

    pub(super) async fn build(
        &self,
        workspace: &ProxyWorkspace,
        listener: &ProxyListener,
        runtime_epoch: uuid::Uuid,
    ) -> AppResult<PreparedListenerRuntime> {
        self.build_prepared(workspace, listener, runtime_epoch, true)
            .await
    }

    pub(super) async fn build_probe(
        &self,
        workspace: &ProxyWorkspace,
        listener: &ProxyListener,
        runtime_epoch: uuid::Uuid,
    ) -> AppResult<PreparedListenerRuntime> {
        self.build_prepared(workspace, listener, runtime_epoch, false)
            .await
    }

    async fn build_prepared(
        &self,
        workspace: &ProxyWorkspace,
        listener: &ProxyListener,
        runtime_epoch: uuid::Uuid,
        full_runtime: bool,
    ) -> AppResult<PreparedListenerRuntime> {
        ensure_snapshot_matches(workspace, listener)?;
        let bind_addr = parse_bind_address(&listener.bind_address, listener.port, listener.id)?;
        match &listener.data_plane {
            ListenerDataPlane::Http(http) => {
                self.build_http(
                    workspace,
                    listener,
                    http,
                    bind_addr,
                    runtime_epoch,
                    full_runtime,
                )
                .await
            }
            ListenerDataPlane::Socket(socket) => {
                self.build_socket(workspace, listener, socket, bind_addr, full_runtime)
                    .await
            }
        }
    }

    async fn build_http(
        &self,
        workspace: &ProxyWorkspace,
        listener: &ProxyListener,
        http: &HttpListenerSettings,
        bind_addr: SocketAddr,
        runtime_epoch: uuid::Uuid,
        full_runtime: bool,
    ) -> AppResult<PreparedListenerRuntime> {
        let protocol = if full_runtime {
            HttpProtocolRuntimeSnapshot::prepare_async(self.adapter, workspace, listener).await?
        } else {
            None
        };
        let pipeline = if full_runtime {
            Some(self.pipeline(listener)?.ports.clone())
        } else {
            None
        };
        let capabilities = if full_runtime {
            Some(protocol.as_ref().map_or_else(
                || {
                    Arc::new(PlainHttpCapabilityFactory::new(
                        workspace.id.to_string(),
                        listener.id.to_string(),
                    )) as Arc<dyn HttpProtocolCapabilityFactory>
                },
                |snapshot| Arc::clone(snapshot) as Arc<dyn HttpProtocolCapabilityFactory>,
            ))
        } else {
            None
        };
        let context = HttpBuildContext {
            workspace,
            listener,
            http,
            bind_addr,
            protocol,
            pipeline,
            capabilities,
        };
        match &http.topology {
            HttpTopology::RemoteServer(remote) => {
                if let Some(fixed) = &remote.fixed_server {
                    return self.build_fixed_http(&context, fixed, full_runtime).await;
                }
                if !full_runtime {
                    return Err(AppError::new(
                        "FIXED_SERVER_NOT_CONFIGURED",
                        "该代理监听未配置固定 Server，没有上游连接可测试。",
                    )
                    .entity(listener.id.to_string()));
                }
                self.build_forward_http(&context, runtime_epoch).await
            }
            HttpTopology::LocalServer => {
                if !full_runtime {
                    return Err(AppError::new(
                        "LISTENER_UPSTREAM_NOT_APPLICABLE",
                        "HTTP LocalServer 没有可测试的真实 Server 上游。",
                    )
                    .entity(listener.id.to_string()));
                }
                self.build_local_http(&context).await
            }
        }
    }

    async fn build_fixed_http(
        &self,
        context: &HttpBuildContext<'_>,
        fixed: &intercept_proxy_domain::FixedServerSettings,
        full_runtime: bool,
    ) -> AppResult<PreparedListenerRuntime> {
        let mut service = ReverseProxyService::build(ReverseProxyConfig {
            bind_addr: context.bind_addr,
            upstream_origin: fixed.upstream_url.clone(),
            downstream_tls: if full_runtime {
                self.adapter
                    .downstream_tls(context.workspace, context.listener, context.http)
                    .await?
            } else {
                None
            },
            upstream_tls: self.adapter.upstream_tls(context.workspace, fixed).await?,
            connect_timeout: Duration::from_millis(context.listener.connect_timeout_ms),
            read_timeout: Duration::from_millis(context.listener.read_timeout_ms),
            write_timeout: Duration::from_millis(context.listener.write_timeout_ms),
        })
        .await
        .map_err(|error| runtime_error(context.listener, error.code, error.message))?;
        if let Some(pipeline) = &context.pipeline {
            service = service
                .with_pipeline(
                    channel(context.listener)?,
                    Arc::clone(pipeline),
                    context
                        .capabilities
                        .clone()
                        .expect("full runtime HTTP capabilities prepared"),
                    MessageLimits::default(),
                    DEFAULT_MAX_CONNECTIONS,
                )
                .map_err(|error| runtime_error(context.listener, error.code, error.message))?;
        }
        Ok(PreparedListenerRuntime::HttpFixed {
            bind_addr: context.bind_addr,
            service: Box::new(service),
            protocol: context.protocol.clone(),
        })
    }

    async fn build_forward_http(
        &self,
        context: &HttpBuildContext<'_>,
        runtime_epoch: uuid::Uuid,
    ) -> AppResult<PreparedListenerRuntime> {
        let (authentication, authenticator) = self.authentication(context.http).await?;
        let mut service = ForwardProxyService::new(
            ForwardProxyConfig {
                bind_addr: context.bind_addr,
                authentication,
                connect_timeout: Duration::from_millis(context.listener.connect_timeout_ms),
                read_timeout: Duration::from_millis(context.listener.read_timeout_ms),
                write_timeout: Duration::from_millis(context.listener.write_timeout_ms),
            },
            authenticator,
        )
        .map_err(|error| runtime_error(context.listener, error.code, error.message))?;
        if let Some(tls) = self
            .adapter
            .downstream_tls(context.workspace, context.listener, context.http)
            .await?
        {
            service = service
                .with_downstream_tls(&tls)
                .map_err(|error| runtime_error(context.listener, error.code, error.message))?;
        }
        if context.http.mitm.enabled {
            return Err(AppError::new(
                "HTTP_TUNNEL_UNSUPPORTED",
                "当前 Exchange 架构不支持 HTTP CONNECT、Upgrade 或 MITM 隧道。",
            )
            .entity(context.listener.id.to_string()));
        }
        Ok(PreparedListenerRuntime::HttpForward {
            bind_addr: context.bind_addr,
            service: service.with_pipeline(
                channel(context.listener)?,
                runtime_epoch,
                context
                    .pipeline
                    .clone()
                    .expect("full runtime pipeline prepared"),
                context
                    .capabilities
                    .clone()
                    .expect("full runtime HTTP capabilities prepared"),
                MessageLimits::default(),
            ),
            protocol: context.protocol.clone(),
        })
    }

    async fn build_local_http(
        &self,
        context: &HttpBuildContext<'_>,
    ) -> AppResult<PreparedListenerRuntime> {
        let config = LocalHttpServerConfig {
            bind_addr: context.bind_addr,
            downstream_tls: self
                .adapter
                .downstream_tls(context.workspace, context.listener, context.http)
                .await?,
            read_timeout: Duration::from_millis(context.listener.read_timeout_ms),
            write_timeout: Duration::from_millis(context.listener.write_timeout_ms),
        };
        let service = LocalHttpServerService::build(
            &config,
            channel(context.listener)?,
            context
                .pipeline
                .clone()
                .expect("full runtime pipeline prepared"),
            context
                .capabilities
                .clone()
                .expect("full runtime HTTP capabilities prepared"),
            MessageLimits::default(),
            DEFAULT_MAX_CONNECTIONS,
        )
        .map_err(|error| runtime_error(context.listener, error.code, error.message))?;
        Ok(PreparedListenerRuntime::HttpLocal {
            bind_addr: context.bind_addr,
            service,
            protocol: context.protocol.clone(),
        })
    }

    async fn build_socket_probe(
        &self,
        workspace: &ProxyWorkspace,
        listener: &ProxyListener,
        socket: &SocketRelaySettings,
        bind_addr: SocketAddr,
    ) -> AppResult<PreparedListenerRuntime> {
        let SocketTopology::Relay(relay) = &socket.topology else {
            return Err(AppError::new(
                "LISTENER_UPSTREAM_NOT_APPLICABLE",
                "本地应答没有上游连接、DNS 或 TLS 探测能力。",
            )
            .entity(listener.id.to_string()));
        };
        socket_plan::build_probe(
            self.adapter,
            listener,
            socket,
            relay,
            bind_addr,
            self.socket_probe_security(workspace, &relay.security)
                .await?,
        )
    }

    async fn authentication(
        &self,
        http: &HttpListenerSettings,
    ) -> AppResult<(
        ForwardAuthenticationMode,
        Arc<dyn ForwardProxyAuthenticator>,
    )> {
        use intercept_proxy_domain::ForwardProxyAuthentication;
        match &http.authentication {
            ForwardProxyAuthentication::None => {
                Ok((ForwardAuthenticationMode::None, Arc::new(NoAuthentication)))
            }
            ForwardProxyAuthentication::Basic { credential } => Ok((
                ForwardAuthenticationMode::Required,
                self.adapter
                    .protected_secrets
                    .resolve_basic_authenticator(credential)
                    .await?,
            )),
        }
    }

    fn pipeline(&self, listener: &ProxyListener) -> AppResult<super::RuntimePipelineServices> {
        self.adapter
            .pipeline_services
            .read()
            .clone()
            .ok_or_else(|| {
                AppError::new(
                    "LISTENER_RUNTIME_NOT_READY",
                    "通用规则与抓包管线尚未完成装配。",
                )
                .entity(listener.id.to_string())
            })
    }

    async fn socket_security(
        &self,
        workspace: &ProxyWorkspace,
        security: &DomainSocketSecurity,
    ) -> AppResult<RuntimeSocketSecurity> {
        Ok(match security {
            DomainSocketSecurity::Transparent => RuntimeSocketSecurity::Transparent,
            DomainSocketSecurity::TcpToTls { upstream_tls } => RuntimeSocketSecurity::TcpToTls {
                upstream_tls: self.socket_upstream_tls(workspace, upstream_tls).await?,
            },
            DomainSocketSecurity::TlsToTcp { downstream_tls } => RuntimeSocketSecurity::TlsToTcp {
                downstream_tls: self
                    .socket_downstream_tls(workspace, downstream_tls)
                    .await?,
            },
            DomainSocketSecurity::TlsToTls {
                downstream_tls,
                upstream_tls,
            } => RuntimeSocketSecurity::TlsToTls {
                downstream_tls: self
                    .socket_downstream_tls(workspace, downstream_tls)
                    .await?,
                upstream_tls: self.socket_upstream_tls(workspace, upstream_tls).await?,
            },
        })
    }

    async fn socket_probe_security(
        &self,
        workspace: &ProxyWorkspace,
        security: &DomainSocketSecurity,
    ) -> AppResult<RuntimeSocketSecurity> {
        Ok(match security {
            DomainSocketSecurity::Transparent | DomainSocketSecurity::TlsToTcp { .. } => {
                RuntimeSocketSecurity::Transparent
            }
            DomainSocketSecurity::TcpToTls { upstream_tls }
            | DomainSocketSecurity::TlsToTls { upstream_tls, .. } => {
                RuntimeSocketSecurity::TcpToTls {
                    upstream_tls: self.socket_upstream_tls(workspace, upstream_tls).await?,
                }
            }
        })
    }
}

fn channel(listener: &ProxyListener) -> AppResult<ChannelId> {
    ChannelId::new(listener.id.to_string())
        .map_err(|error| AppError::new(error.code, error.message))
}

pub(super) fn runtime_error(
    listener: &ProxyListener,
    code: &'static str,
    message: String,
) -> AppError {
    AppError::new(code, message).entity(listener.id.to_string())
}
