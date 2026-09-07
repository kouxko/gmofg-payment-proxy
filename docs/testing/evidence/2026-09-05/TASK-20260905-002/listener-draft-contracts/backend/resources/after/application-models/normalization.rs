use std::collections::BTreeMap;

use intercept_proxy_domain::{
    DownstreamClientAuthentication, DownstreamTlsSettings, HttpListenerSettings, ListenerDataPlane,
    ProxyListener, ScriptedSocketProcessing, SocketDownstreamSecurity, SocketDownstreamTlsSettings,
    SocketLocalResponderTopology, SocketPayloadProcessing, SocketRelaySecurity,
    SocketRelaySettings, SocketRelayTopology, SocketTopology,
};

use super::{
    DownstreamClientAuthenticationDraft, HttpListenerDraftSettings, ListenerDataPlaneDraft,
    ListenerDraft, ScriptedSocketProcessingDraft, SocketDownstreamSecurityDraft,
    SocketDownstreamTlsDraftSettings, SocketPayloadProcessingDraft, SocketRelayDraftSettings,
    SocketRelaySecurityDraft, SocketTopologyDraft,
};

impl ListenerDraft {
    pub(crate) fn try_into_listener(
        self,
        index: usize,
    ) -> Result<ProxyListener, BTreeMap<String, Vec<String>>> {
        let mut errors = BTreeMap::new();
        let prefix = format!("listeners.{index}.data_plane.settings");
        let data_plane = self.data_plane.try_into_domain(&prefix, &mut errors);
        if !errors.is_empty() {
            return Err(errors);
        }
        Ok(ProxyListener {
            id: self.id,
            name: self.name,
            enabled: self.enabled,
            bind_address: self.bind_address,
            port: self.port,
            connect_timeout_ms: self.connect_timeout_ms,
            read_timeout_ms: self.read_timeout_ms,
            write_timeout_ms: self.write_timeout_ms,
            data_plane: data_plane.expect("all incomplete fields are recorded"),
        })
    }
}

impl ListenerDataPlaneDraft {
    fn try_into_domain(
        self,
        prefix: &str,
        errors: &mut BTreeMap<String, Vec<String>>,
    ) -> Option<ListenerDataPlane> {
        match self {
            Self::Http(value) => value
                .try_into_domain(prefix, errors)
                .map(ListenerDataPlane::Http),
            Self::Socket(value) => value
                .try_into_domain(prefix, errors)
                .map(ListenerDataPlane::Socket),
        }
    }
}

impl HttpListenerDraftSettings {
    fn try_into_domain(
        self,
        prefix: &str,
        errors: &mut BTreeMap<String, Vec<String>>,
    ) -> Option<HttpListenerSettings> {
        let downstream_tls = self
            .downstream_tls
            .try_into_domain(&format!("{prefix}.downstream_tls"), errors);
        Some(HttpListenerSettings {
            authentication: self.authentication,
            mitm: self.mitm,
            downstream_tls: downstream_tls?,
            request_body_codec: self.request_body_codec,
            response_body_codec: self.response_body_codec,
            body_processing: self.body_processing,
            topology: self.topology,
        })
    }
}

impl super::DownstreamTlsDraftSettings {
    fn try_into_domain(
        self,
        prefix: &str,
        errors: &mut BTreeMap<String, Vec<String>>,
    ) -> Option<DownstreamTlsSettings> {
        let client_authentication = self
            .client_authentication
            .try_into_domain(&format!("{prefix}.client_authentication"), errors);
        Some(DownstreamTlsSettings {
            enabled: self.enabled,
            server_identity: self.server_identity,
            dynamic_sni_allowlist: self.dynamic_sni_allowlist,
            client_authentication: client_authentication?,
        })
    }
}

impl DownstreamClientAuthenticationDraft {
    fn try_into_domain(
        self,
        prefix: &str,
        errors: &mut BTreeMap<String, Vec<String>>,
    ) -> Option<DownstreamClientAuthentication> {
        match self {
            Self::Disabled => Some(DownstreamClientAuthentication::Disabled),
            Self::Optional { trust } => required(
                trust,
                format!("{prefix}.trust"),
                "必须选择客户端信任证书",
                errors,
            )
            .map(|trust| DownstreamClientAuthentication::Optional { trust }),
            Self::Required { trust } => required(
                trust,
                format!("{prefix}.trust"),
                "必须选择客户端信任证书",
                errors,
            )
            .map(|trust| DownstreamClientAuthentication::Required { trust }),
        }
    }
}

impl SocketRelayDraftSettings {
    fn try_into_domain(
        self,
        prefix: &str,
        errors: &mut BTreeMap<String, Vec<String>>,
    ) -> Option<SocketRelaySettings> {
        let topology = self
            .topology
            .try_into_domain(&format!("{prefix}.topology"), errors);
        let processing = self
            .processing
            .try_into_domain(&format!("{prefix}.processing"), errors);
        Some(SocketRelaySettings {
            topology: topology?,
            maximum_connections: self.maximum_connections,
            runtime_limits: self.runtime_limits,
            processing: processing?,
        })
    }
}

impl SocketTopologyDraft {
    fn try_into_domain(
        self,
        prefix: &str,
        errors: &mut BTreeMap<String, Vec<String>>,
    ) -> Option<SocketTopology> {
        match self {
            Self::Relay(value) => value
                .security
                .try_into_domain(&format!("{prefix}.settings.security"), errors)
                .map(|security| {
                    SocketTopology::Relay(SocketRelayTopology {
                        upstream: value.upstream,
                        security,
                    })
                }),
            Self::LocalResponder(value) => value
                .downstream_security
                .try_into_domain(&format!("{prefix}.settings.downstream_security"), errors)
                .map(|downstream_security| {
                    SocketTopology::LocalResponder(SocketLocalResponderTopology {
                        downstream_security,
                    })
                }),
        }
    }
}

impl SocketRelaySecurityDraft {
    fn try_into_domain(
        self,
        prefix: &str,
        errors: &mut BTreeMap<String, Vec<String>>,
    ) -> Option<SocketRelaySecurity> {
        match self {
            Self::Transparent => Some(SocketRelaySecurity::Transparent),
            Self::TcpToTls { upstream_tls } => Some(SocketRelaySecurity::TcpToTls { upstream_tls }),
            Self::TlsToTcp { downstream_tls } => downstream_tls
                .try_into_domain(&format!("{prefix}.downstream_tls"), errors)
                .map(|downstream_tls| SocketRelaySecurity::TlsToTcp { downstream_tls }),
            Self::TlsToTls {
                downstream_tls,
                upstream_tls,
            } => downstream_tls
                .try_into_domain(&format!("{prefix}.downstream_tls"), errors)
                .map(|downstream_tls| SocketRelaySecurity::TlsToTls {
                    downstream_tls,
                    upstream_tls,
                }),
        }
    }
}

impl SocketDownstreamSecurityDraft {
    fn try_into_domain(
        self,
        prefix: &str,
        errors: &mut BTreeMap<String, Vec<String>>,
    ) -> Option<SocketDownstreamSecurity> {
        match self {
            Self::Tcp => Some(SocketDownstreamSecurity::Tcp),
            Self::Tls { downstream_tls } => downstream_tls
                .try_into_domain(&format!("{prefix}.downstream_tls"), errors)
                .map(|downstream_tls| SocketDownstreamSecurity::Tls { downstream_tls }),
        }
    }
}

impl SocketDownstreamTlsDraftSettings {
    fn try_into_domain(
        self,
        prefix: &str,
        errors: &mut BTreeMap<String, Vec<String>>,
    ) -> Option<SocketDownstreamTlsSettings> {
        let server_identity = required(
            self.server_identity,
            format!("{prefix}.server_identity"),
            "必须选择下游 TLS 服务端身份",
            errors,
        );
        let client_authentication = self
            .client_authentication
            .try_into_domain(&format!("{prefix}.client_authentication"), errors);
        Some(SocketDownstreamTlsSettings {
            server_identity: server_identity?,
            client_authentication: client_authentication?,
        })
    }
}

impl SocketPayloadProcessingDraft {
    fn try_into_domain(
        self,
        prefix: &str,
        errors: &mut BTreeMap<String, Vec<String>>,
    ) -> Option<SocketPayloadProcessing> {
        match self {
            Self::Direct => Some(SocketPayloadProcessing::Direct),
            Self::Scripted(ScriptedSocketProcessingDraft { package }) => required(
                package,
                format!("{prefix}.settings.package"),
                "必须选择协议包",
                errors,
            )
            .map(|package| SocketPayloadProcessing::Scripted(ScriptedSocketProcessing { package })),
        }
    }
}

fn required<T>(
    value: Option<T>,
    field: String,
    message: &'static str,
    errors: &mut BTreeMap<String, Vec<String>>,
) -> Option<T> {
    value.or_else(|| {
        errors.entry(field).or_default().push(message.into());
        None
    })
}
