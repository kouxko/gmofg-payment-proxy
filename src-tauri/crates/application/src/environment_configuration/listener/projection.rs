use intercept_proxy_domain::{
    BodyCodecKind, FixedServerSettings, ForwardProxyAuthentication, HttpBodyProcessing,
    HttpListenerSettings, HttpRemoteServerTopology, HttpTopology, ListenerDataPlane, ListenerId,
    MitmSettings, ProtocolPackageId, ProtocolPackageRef, ProtocolPackageVersion, ProxyListener,
    ScriptedSocketProcessing, SocketDownstreamSecurity, SocketEndpoint,
    SocketLocalResponderTopology, SocketPayloadProcessing, SocketRelaySecurity,
    SocketRelaySettings, SocketRelayTopology, SocketRuntimeLimits, SocketTopology,
    UpstreamTlsSettings,
};

use super::{
    AppError, AppResult, AuthenticationTemplate, BodyCodec, BodyProcessingTemplate,
    HttpListenerTemplate, HttpTopologyTemplate, ListenerDataPlaneTemplate, ListenerTemplate,
    ProtocolPackageExactRef, SocketListenerTemplate, SocketPayloadProcessingTemplate,
    SocketTopologyTemplate,
};

impl ListenerTemplate {
    pub(crate) fn to_domain(&self, id: ListenerId) -> AppResult<ProxyListener> {
        Ok(ProxyListener {
            id,
            name: self.name.clone(),
            enabled: self.enabled,
            bind_address: self.bind_address.clone(),
            port: self.port,
            connect_timeout_ms: self.connect_timeout_ms,
            read_timeout_ms: self.read_timeout_ms,
            write_timeout_ms: self.write_timeout_ms,
            data_plane: match &self.data_plane {
                ListenerDataPlaneTemplate::Http(settings) => {
                    ListenerDataPlane::Http(settings.to_domain()?)
                }
                ListenerDataPlaneTemplate::Socket(settings) => {
                    ListenerDataPlane::Socket(settings.to_domain()?)
                }
            },
        })
    }
}

impl HttpListenerTemplate {
    fn to_domain(&self) -> AppResult<HttpListenerSettings> {
        Ok(HttpListenerSettings {
            authentication: match &self.authentication {
                AuthenticationTemplate::None => ForwardProxyAuthentication::None,
                AuthenticationTemplate::Basic { credential_alias } => {
                    ForwardProxyAuthentication::Basic {
                        credential: intercept_proxy_domain::SecretReference {
                            provider: "candidate".into(),
                            key: credential_alias.clone(),
                        },
                    }
                }
            },
            mitm: MitmSettings {
                enabled: false,
                authority_allowlist: self.mitm.authority_allowlist.clone(),
                root_ca: None,
                maximum_cached_leaf_certificates: self.mitm.maximum_cached_leaf_certificates,
            },
            downstream_tls: intercept_proxy_domain::DownstreamTlsSettings::default(),
            request_body_codec: self.request_body_codec.into_domain(),
            response_body_codec: self.response_body_codec.into_domain(),
            body_processing: match &self.body_processing {
                BodyProcessingTemplate::Plain => HttpBodyProcessing::Plain,
                BodyProcessingTemplate::Protocol { package } => HttpBodyProcessing::Protocol {
                    package: package.to_domain()?,
                },
            },
            topology: match &self.topology {
                HttpTopologyTemplate::RemoteServer(remote) => {
                    HttpTopology::RemoteServer(HttpRemoteServerTopology {
                        fixed_server: remote.fixed_server.as_ref().map(|fixed| {
                            FixedServerSettings {
                                upstream_url: fixed.upstream_url.clone(),
                                upstream_tls: UpstreamTlsSettings {
                                    verify_hostname: fixed.upstream_tls.verify_hostname,
                                    server_trust: None,
                                    client_identity: None,
                                },
                            }
                        }),
                    })
                }
                HttpTopologyTemplate::LocalServer => HttpTopology::LocalServer,
            },
        })
    }
}

impl SocketListenerTemplate {
    fn to_domain(&self) -> AppResult<SocketRelaySettings> {
        let topology = match &self.topology {
            SocketTopologyTemplate::Relay(relay) => SocketTopology::Relay(SocketRelayTopology {
                upstream: SocketEndpoint {
                    host: relay.upstream.host.clone(),
                    port: relay.upstream.port,
                },
                security: SocketRelaySecurity::Transparent,
            }),
            SocketTopologyTemplate::LocalResponder(_) => {
                SocketTopology::LocalResponder(SocketLocalResponderTopology {
                    downstream_security: SocketDownstreamSecurity::Tcp,
                })
            }
        };
        Ok(SocketRelaySettings {
            topology,
            maximum_connections: self.maximum_connections,
            runtime_limits: SocketRuntimeLimits {
                read_chunk_bytes: self.runtime_limits.read_chunk_bytes,
                diagnostic_event_capacity: self.runtime_limits.diagnostic_event_capacity,
                diagnostic_memory_bytes: self.runtime_limits.diagnostic_memory_bytes,
            },
            processing: match &self.processing {
                SocketPayloadProcessingTemplate::Direct => SocketPayloadProcessing::Direct,
                SocketPayloadProcessingTemplate::Scripted(settings) => {
                    SocketPayloadProcessing::Scripted(ScriptedSocketProcessing {
                        package: settings.package.to_domain()?,
                    })
                }
            },
        })
    }
}

impl ProtocolPackageExactRef {
    pub(crate) fn to_domain(&self) -> AppResult<ProtocolPackageRef> {
        Ok(ProtocolPackageRef {
            id: ProtocolPackageId::new(self.id.clone()).map_err(AppError::from)?,
            version: ProtocolPackageVersion::new(self.version.clone()).map_err(AppError::from)?,
        })
    }
}

impl BodyCodec {
    const fn into_domain(self) -> BodyCodecKind {
        match self {
            Self::Auto => BodyCodecKind::Auto,
            Self::Raw => BodyCodecKind::Raw,
            Self::Utf8 => BodyCodecKind::Utf8,
            Self::ShiftJis => BodyCodecKind::ShiftJis,
        }
    }
}
