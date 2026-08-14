//! Validated Workspace listener snapshots mapped to immutable runtime-native plans.

use std::{net::SocketAddr, sync::Arc, time::Duration};

use intercept_proxy_application::{AppError, AppResult};
use intercept_proxy_domain::{
    DownstreamClientAuthentication, HttpListenerSettings, ListenerDataPlane, ProxyListener,
    ProxyWorkspace, SocketDownstreamSecurity, SocketDownstreamTlsSettings, SocketPayloadProcessing,
    SocketRelaySecurity as DomainSocketSecurity, SocketRelaySettings, SocketTopology,
    SocketUpstreamTlsSettings,
};
use intercept_proxy_runtime::{
    ChannelId, DEFAULT_MAX_CONNECTIONS, ForwardAuthenticationMode, ForwardMitmConfig,
    ForwardProxyAuthenticator, ForwardProxyConfig, ForwardProxyService, MessageLimits,
    NativeRootMitmConnector, NoAuthentication, ReverseProxyConfig, ReverseProxyService,
    SocketDownstreamTlsConfig, SocketEndpoint, SocketRelayConfig,
    SocketRelaySecurity as RuntimeSocketSecurity, SocketRelayService, SocketTlsIdentity,
    SocketUpstreamTlsConfig,
};

use super::{
    ListenerRuntimeAdapter,
    helpers::ensure_snapshot_matches,
    parse_bind_address,
    scripted_snapshot::{ScriptedSocketRuntimeSnapshot, ScriptedSocketSecuritySnapshot},
    socket_diagnostics::SocketDiagnosticObserver,
    socket_plan,
};

pub(super) enum PreparedListenerRuntime {
    HttpForward {
        bind_addr: SocketAddr,
        service: ForwardProxyService,
    },
    HttpFixed {
        bind_addr: SocketAddr,
        service: Box<ReverseProxyService>,
    },
    Socket {
        bind_addr: SocketAddr,
        service: Arc<SocketRelayService>,
    },
    /// T21 冻结校验完成；T22 Frame Pump 接线前真实 start 必须 fail-closed。
    ScriptedSocket {
        bind_addr: SocketAddr,
        snapshot: Arc<ScriptedSocketRuntimeSnapshot>,
    },
}
impl PreparedListenerRuntime {
    pub(super) const fn bind_addr(&self) -> SocketAddr {
        match self {
            Self::HttpForward { bind_addr, .. }
            | Self::HttpFixed { bind_addr, .. }
            | Self::Socket { bind_addr, .. }
            | Self::ScriptedSocket { bind_addr, .. } => *bind_addr,
        }
    }

    pub(super) fn scripted_snapshot(&self) -> Option<Arc<ScriptedSocketRuntimeSnapshot>> {
        match self {
            Self::ScriptedSocket { snapshot, .. } => Some(Arc::clone(snapshot)),
            _ => None,
        }
    }
}

pub(super) struct ListenerRuntimePlanBuilder<'ctx> {
    adapter: &'ctx ListenerRuntimeAdapter,
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
        if let Some(fixed) = &http.fixed_server {
            let mut service = ReverseProxyService::build(ReverseProxyConfig {
                bind_addr,
                allowed_client_cidrs: listener.allowed_client_cidrs.clone(),
                upstream_origin: fixed.upstream_url.clone(),
                downstream_tls: if full_runtime {
                    self.adapter.downstream_tls(workspace, listener, http)?
                } else {
                    None
                },
                upstream_tls: self.adapter.upstream_tls(workspace, fixed)?,
                connect_timeout: Duration::from_millis(listener.connect_timeout_ms),
                read_timeout: Duration::from_millis(listener.read_timeout_ms),
                write_timeout: Duration::from_millis(listener.write_timeout_ms),
            })
            .await
            .map_err(|error| runtime_error(listener, error.code, error.message))?;
            if full_runtime {
                service = service
                    .with_pipeline(
                        channel(listener)?,
                        self.pipeline(listener)?,
                        MessageLimits::default(),
                        DEFAULT_MAX_CONNECTIONS,
                    )
                    .map_err(|error| runtime_error(listener, error.code, error.message))?;
            }
            return Ok(PreparedListenerRuntime::HttpFixed {
                bind_addr,
                service: Box::new(service),
            });
        }

        if !full_runtime {
            return Err(AppError::new(
                "FIXED_SERVER_NOT_CONFIGURED",
                "该代理监听未配置固定 Server，没有上游连接可测试。",
            )
            .entity(listener.id.to_string()));
        }

        let (authentication, authenticator) = self.authentication(listener, http)?;
        let mut service = ForwardProxyService::new(
            ForwardProxyConfig {
                bind_addr,
                authentication,
                allowed_client_cidrs: listener.allowed_client_cidrs.clone(),
                connect_timeout: Duration::from_millis(listener.connect_timeout_ms),
                read_timeout: Duration::from_millis(listener.read_timeout_ms),
                write_timeout: Duration::from_millis(listener.write_timeout_ms),
                tunnel_idle_timeout: Duration::from_millis(
                    listener.read_timeout_ms.min(listener.write_timeout_ms),
                ),
            },
            authenticator,
        )
        .map_err(|error| runtime_error(listener, error.code, error.message))?;
        if let Some(tls) = self.adapter.downstream_tls(workspace, listener, http)? {
            service = service
                .with_downstream_tls(&tls)
                .map_err(|error| runtime_error(listener, error.code, error.message))?;
        }
        if http.mitm.enabled {
            let authority = self
                .adapter
                .mitm_certificate_authority
                .clone()
                .ok_or_else(|| certificate_not_ready(listener, "MITM Root CA 签发能力"))?;
            let upstream = NativeRootMitmConnector::new()
                .map_err(|error| AppError::new(error.code, error.message))?;
            service = service
                .with_mitm(
                    ForwardMitmConfig {
                        authority_allowlist: http.mitm.authority_allowlist.clone(),
                        maximum_cached_leaf_certificates: usize::from(
                            http.mitm.maximum_cached_leaf_certificates,
                        ),
                    },
                    authority,
                    Arc::new(upstream),
                )
                .map_err(|error| runtime_error(listener, error.code, error.message))?;
        }
        Ok(PreparedListenerRuntime::HttpForward {
            bind_addr,
            service: service.with_pipeline(
                channel(listener)?,
                runtime_epoch,
                self.pipeline(listener)?,
                MessageLimits::default(),
            ),
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
            let security = match &socket.topology {
                SocketTopology::Relay(relay) => ScriptedSocketSecuritySnapshot::Relay(
                    self.socket_security(workspace, &relay.security)?,
                ),
                SocketTopology::LocalResponder(local) => {
                    ScriptedSocketSecuritySnapshot::LocalResponder {
                        downstream_tls: match &local.downstream_security {
                            SocketDownstreamSecurity::Tcp => None,
                            SocketDownstreamSecurity::Tls { downstream_tls } => {
                                Some(self.socket_downstream_tls(workspace, downstream_tls)?)
                            }
                        },
                    }
                }
            };
            let snapshot = ScriptedSocketRuntimeSnapshot::prepare(
                self.adapter,
                workspace,
                listener,
                security,
            )?
            .ok_or_else(|| {
                AppError::new(
                    "SCRIPTED_SOCKET_PLAN_INVALID",
                    "Scripted Socket 未能生成不可变运行计划。",
                )
                .entity(listener.id.to_string())
            })?;
            return Ok(PreparedListenerRuntime::ScriptedSocket {
                bind_addr,
                snapshot,
            });
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
                "LOCAL_RESPONDER_NOT_AVAILABLE",
                "LocalResponder 没有上游连接、DNS 或 TLS 探测能力。",
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
        listener: &ProxyListener,
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
            ForwardProxyAuthentication::Basic { credential } => {
                let resolver = self.adapter.protected_secrets.as_ref().ok_or_else(|| {
                    AppError::new(
                        "SECRET_PROTECTOR_UNAVAILABLE",
                        "当前宿主没有提供代理认证安全引用解析能力。",
                    )
                    .entity(listener.id.to_string())
                })?;
                Ok((
                    ForwardAuthenticationMode::Required,
                    resolver.resolve_basic_authenticator(credential)?,
                ))
            }
        }
    }

    fn pipeline(
        &self,
        listener: &ProxyListener,
    ) -> AppResult<Arc<dyn intercept_proxy_runtime::PipelinePorts>> {
        self.adapter.pipeline_ports.read().clone().ok_or_else(|| {
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

    fn socket_downstream_tls(
        &self,
        workspace: &ProxyWorkspace,
        settings: &SocketDownstreamTlsSettings,
    ) -> AppResult<SocketDownstreamTlsConfig> {
        let identity = self
            .adapter
            .load_identity_by_id(workspace, settings.server_identity)?;
        let (client_trust_der, client_authentication_required) =
            match settings.client_authentication {
                DownstreamClientAuthentication::Disabled => (Vec::new(), false),
                DownstreamClientAuthentication::Optional { trust } => {
                    (self.adapter.load_trust_by_id(workspace, trust)?, false)
                }
                DownstreamClientAuthentication::Required { trust } => {
                    (self.adapter.load_trust_by_id(workspace, trust)?, true)
                }
            };
        Ok(SocketDownstreamTlsConfig {
            server_identity: SocketTlsIdentity {
                certificate_chain_der: identity.certificate_chain_der,
                private_key_pkcs8_der: identity.private_key_pkcs8_der,
            },
            client_trust_der,
            client_authentication_required,
        })
    }

    fn socket_upstream_tls(
        &self,
        workspace: &ProxyWorkspace,
        settings: &SocketUpstreamTlsSettings,
    ) -> AppResult<SocketUpstreamTlsConfig> {
        let server_trust_der = settings
            .server_trust
            .map(|id| self.adapter.load_trust_by_id(workspace, id))
            .transpose()?
            .unwrap_or_default();
        let client_identity = settings
            .client_identity
            .map(|id| self.adapter.load_identity_by_id(workspace, id))
            .transpose()?
            .map(|identity| SocketTlsIdentity {
                certificate_chain_der: identity.certificate_chain_der,
                private_key_pkcs8_der: identity.private_key_pkcs8_der,
            });
        Ok(SocketUpstreamTlsConfig {
            server_trust_der,
            client_identity,
            verify_hostname: settings.verify_hostname,
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
