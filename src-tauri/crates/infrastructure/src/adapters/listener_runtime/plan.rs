//! Validated Workspace listener snapshots mapped to immutable runtime-native plans.

use std::{net::SocketAddr, sync::Arc, time::Duration};

use intercept_proxy_application::{AppError, AppResult};
use intercept_proxy_domain::{
    DownstreamClientAuthentication, HttpListenerSettings, ListenerDataPlane, ProxyListener,
    ProxyWorkspace, SocketDownstreamTlsSettings, SocketPayloadProcessing,
    SocketRelaySecurity as DomainSocketSecurity, SocketRelaySettings, SocketTopology,
    SocketUpstreamTlsSettings,
};
use intercept_proxy_runtime::{
    ChannelId, DEFAULT_MAX_CONNECTIONS, ForwardAuthenticationMode, ForwardMitmConfig,
    ForwardProxyAuthenticator, ForwardProxyConfig, ForwardProxyService, MessageLimits,
    NativeRootMitmConnector, NoAuthentication, PipelinePorts, ReverseProxyConfig,
    ReverseProxyService, SocketDownstreamTlsConfig, SocketEndpoint, SocketRelayConfig,
    SocketRelaySecurity as RuntimeSocketSecurity, SocketRelayService, SocketTlsIdentity,
    SocketUpstreamTlsConfig,
};

use super::{
    ListenerRuntimeAdapter, helpers::ensure_snapshot_matches,
    http_protocol_pipeline::HttpProtocolRuntimeSnapshot, parse_bind_address,
    scripted_snapshot::ScriptedSocketRuntimeSnapshot, socket_diagnostics::SocketDiagnosticObserver,
    socket_plan,
};

mod tls;

mod scripted;

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
    /// Scripted Relay 与 `LocalResponder` 共用冻结快照，但各自装配拓扑专用服务。
    /// `None` 只保留为反序列化之外的内部防御状态，启动门禁会在 bind 前拒绝它。
    ScriptedSocket {
        bind_addr: SocketAddr,
        snapshot: Arc<ScriptedSocketRuntimeSnapshot>,
        service: Option<Arc<SocketRelayService>>,
    },
}
impl PreparedListenerRuntime {
    pub(super) const fn bind_addr(&self) -> SocketAddr {
        match self {
            Self::HttpForward { bind_addr, .. }
            | Self::HttpFixed { bind_addr, .. }
            | Self::Socket { bind_addr, .. }
            | Self::ExternalScriptedSocket { bind_addr, .. }
            | Self::ScriptedSocket { bind_addr, .. } => *bind_addr,
        }
    }

    pub(super) fn scripted_snapshot(&self) -> Option<Arc<ScriptedSocketRuntimeSnapshot>> {
        match self {
            Self::ScriptedSocket { snapshot, .. } => Some(Arc::clone(snapshot)),
            _ => None,
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
            Self::HttpForward { protocol, .. } | Self::HttpFixed { protocol, .. } => {
                protocol.clone()
            }
            Self::Socket { .. }
            | Self::ExternalScriptedSocket { .. }
            | Self::ScriptedSocket { .. } => None,
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
}

impl<'ctx> ListenerRuntimePlanBuilder<'ctx> {
    pub(super) const fn new(adapter: &'ctx ListenerRuntimeAdapter) -> Self {
        Self { adapter }
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
            HttpProtocolRuntimeSnapshot::prepare(self.adapter, workspace, listener)?
        } else {
            None
        };
        let pipeline = if full_runtime {
            let services = self.pipeline(listener)?;
            Some(protocol.as_ref().map_or_else(
                || services.ports.clone(),
                |snapshot| {
                    snapshot.wrap(
                        services.ports.clone(),
                        services.http_protocol_observations.clone(),
                    )
                },
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
        };
        if let Some(fixed) = &http.fixed_server {
            return self.build_fixed_http(&context, fixed, full_runtime).await;
        }

        if !full_runtime {
            return Err(AppError::new(
                "FIXED_SERVER_NOT_CONFIGURED",
                "该代理监听未配置固定 Server，没有上游连接可测试。",
            )
            .entity(listener.id.to_string()));
        }

        self.build_forward_http(&context, runtime_epoch)
    }

    async fn build_fixed_http(
        &self,
        context: &HttpBuildContext<'_>,
        fixed: &intercept_proxy_domain::FixedServerSettings,
        full_runtime: bool,
    ) -> AppResult<PreparedListenerRuntime> {
        let mut service = ReverseProxyService::build(ReverseProxyConfig {
            bind_addr: context.bind_addr,
            allowed_client_cidrs: context.listener.allowed_client_cidrs.clone(),
            upstream_origin: fixed.upstream_url.clone(),
            downstream_tls: if full_runtime {
                self.adapter
                    .downstream_tls(context.workspace, context.listener, context.http)?
            } else {
                None
            },
            upstream_tls: self.adapter.upstream_tls(context.workspace, fixed)?,
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

    fn build_forward_http(
        &self,
        context: &HttpBuildContext<'_>,
        runtime_epoch: uuid::Uuid,
    ) -> AppResult<PreparedListenerRuntime> {
        let (authentication, authenticator) = self.authentication(context.http)?;
        let mut service = ForwardProxyService::new(
            ForwardProxyConfig {
                bind_addr: context.bind_addr,
                authentication,
                allowed_client_cidrs: context.listener.allowed_client_cidrs.clone(),
                connect_timeout: Duration::from_millis(context.listener.connect_timeout_ms),
                read_timeout: Duration::from_millis(context.listener.read_timeout_ms),
                write_timeout: Duration::from_millis(context.listener.write_timeout_ms),
                tunnel_idle_timeout: Duration::from_millis(
                    context
                        .listener
                        .read_timeout_ms
                        .min(context.listener.write_timeout_ms),
                ),
            },
            authenticator,
        )
        .map_err(|error| runtime_error(context.listener, error.code, error.message))?;
        if let Some(tls) =
            self.adapter
                .downstream_tls(context.workspace, context.listener, context.http)?
        {
            service = service
                .with_downstream_tls(&tls)
                .map_err(|error| runtime_error(context.listener, error.code, error.message))?;
        }
        if context.http.mitm.enabled {
            let authority = self
                .adapter
                .mitm_certificate_authority
                .clone()
                .ok_or_else(|| certificate_not_ready(context.listener, "MITM Root CA 签发能力"))?;
            let upstream = NativeRootMitmConnector::new()
                .map_err(|error| AppError::new(error.code, error.message))?;
            service = service
                .with_mitm(
                    ForwardMitmConfig {
                        authority_allowlist: context.http.mitm.authority_allowlist.clone(),
                        maximum_cached_leaf_certificates: usize::from(
                            context.http.mitm.maximum_cached_leaf_certificates,
                        ),
                    },
                    authority,
                    Arc::new(upstream),
                )
                .map_err(|error| runtime_error(context.listener, error.code, error.message))?;
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
                MessageLimits::default(),
            ),
            protocol: context.protocol.clone(),
        })
    }

    fn build_socket(
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
        let SocketTopology::Relay(relay) = &socket.topology else {
            return Err(AppError::new(
                "LOCAL_RESPONDER_SCRIPTED_REQUIRED",
                "LocalResponder 必须使用 Scripted 数据处理模式。",
            )
            .entity(listener.id.to_string()));
        };
        let security = self.socket_security(workspace, &relay.security)?;
        let observer = Arc::new(SocketDiagnosticObserver::new(
            self.adapter.socket_diagnostic_events.read().clone(),
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

    fn build_socket_probe(
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
            self.socket_probe_security(workspace, &relay.security)?,
        )
    }

    fn authentication(
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
                    .resolve_basic_authenticator(credential)?,
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
                    "通用规则、抓包与断点管线尚未完成装配。",
                )
                .entity(listener.id.to_string())
            })
    }

    fn socket_security(
        &self,
        workspace: &ProxyWorkspace,
        security: &DomainSocketSecurity,
    ) -> AppResult<RuntimeSocketSecurity> {
        Ok(match security {
            DomainSocketSecurity::Transparent => RuntimeSocketSecurity::Transparent,
            DomainSocketSecurity::TcpToTls { upstream_tls } => RuntimeSocketSecurity::TcpToTls {
                upstream_tls: self.socket_upstream_tls(workspace, upstream_tls)?,
            },
            DomainSocketSecurity::TlsToTcp { downstream_tls } => RuntimeSocketSecurity::TlsToTcp {
                downstream_tls: self.socket_downstream_tls(workspace, downstream_tls)?,
            },
            DomainSocketSecurity::TlsToTls {
                downstream_tls,
                upstream_tls,
            } => RuntimeSocketSecurity::TlsToTls {
                downstream_tls: self.socket_downstream_tls(workspace, downstream_tls)?,
                upstream_tls: self.socket_upstream_tls(workspace, upstream_tls)?,
            },
        })
    }

    fn socket_probe_security(
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
                    upstream_tls: self.socket_upstream_tls(workspace, upstream_tls)?,
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

fn certificate_not_ready(listener: &ProxyListener, capability: &str) -> AppError {
    AppError::new("CERTIFICATE_NOT_READY", format!("{capability}尚未就绪。"))
        .entity(listener.id.to_string())
}
