use super::{
    AuthenticationTemplate, ClientAuthenticationTemplate, ListenerDataPlaneTemplate,
    ListenerTemplate, SocketDownstreamSecurityTemplate, SocketDownstreamTlsTemplate,
    SocketRelaySecurityTemplate, SocketTopologyTemplate, SocketUpstreamTlsTemplate,
};

impl ListenerTemplate {
    pub(in crate::environment_configuration) fn referenced_materials(
        &self,
    ) -> Vec<(&str, &'static str, bool)> {
        let mut refs = Vec::new();
        match &self.data_plane {
            ListenerDataPlaneTemplate::Http(http) => {
                if let AuthenticationTemplate::Basic { credential_alias } = &http.authentication {
                    refs.push((credential_alias.as_str(), "proxy_basic_auth", true));
                }
                if let Some(alias) = &http.downstream_tls.server_identity_alias {
                    refs.push((alias.as_str(), "downstream_server_identity", false));
                }
                match &http.downstream_tls.client_authentication {
                    ClientAuthenticationTemplate::Optional { trust_alias }
                    | ClientAuthenticationTemplate::Required { trust_alias } => {
                        refs.push((trust_alias.as_str(), "downstream_client_trust", false));
                    }
                    ClientAuthenticationTemplate::Disabled => {}
                }
                if let Some(fixed) = http.fixed_server() {
                    if let Some(alias) = &fixed.upstream_tls.server_trust_alias {
                        refs.push((alias.as_str(), "upstream_server_trust", false));
                    }
                    if let Some(alias) = &fixed.upstream_tls.client_identity_alias {
                        refs.push((alias.as_str(), "upstream_client_identity", false));
                    }
                }
            }
            ListenerDataPlaneTemplate::Socket(socket) => match &socket.topology {
                SocketTopologyTemplate::Relay(relay) => match &relay.security {
                    SocketRelaySecurityTemplate::Transparent => {}
                    SocketRelaySecurityTemplate::TcpToTls { upstream_tls } => {
                        upstream_material_refs(upstream_tls, &mut refs);
                    }
                    SocketRelaySecurityTemplate::TlsToTcp { downstream_tls } => {
                        downstream_material_refs(downstream_tls, &mut refs);
                    }
                    SocketRelaySecurityTemplate::TlsToTls {
                        downstream_tls,
                        upstream_tls,
                    } => {
                        downstream_material_refs(downstream_tls, &mut refs);
                        upstream_material_refs(upstream_tls, &mut refs);
                    }
                },
                SocketTopologyTemplate::LocalResponder(local) => {
                    if let SocketDownstreamSecurityTemplate::Tls { downstream_tls } =
                        &local.downstream_security
                    {
                        downstream_material_refs(downstream_tls, &mut refs);
                    }
                }
            },
        }
        refs
    }

    pub(in crate::environment_configuration) fn installation_root_selector(&self) -> Option<&str> {
        match &self.data_plane {
            ListenerDataPlaneTemplate::Http(http) if http.mitm.enabled => {
                http.mitm.root_ca_selector.as_deref()
            }
            _ => None,
        }
    }
}

fn upstream_material_refs<'a>(
    tls: &'a SocketUpstreamTlsTemplate,
    refs: &mut Vec<(&'a str, &'static str, bool)>,
) {
    if let Some(alias) = &tls.server_trust_alias {
        refs.push((alias, "upstream_server_trust", false));
    }
    if let Some(alias) = &tls.client_identity_alias {
        refs.push((alias, "upstream_client_identity", false));
    }
}

fn downstream_material_refs<'a>(
    tls: &'a SocketDownstreamTlsTemplate,
    refs: &mut Vec<(&'a str, &'static str, bool)>,
) {
    refs.push((
        &tls.server_identity_alias,
        "downstream_server_identity",
        false,
    ));
    match &tls.client_authentication {
        ClientAuthenticationTemplate::Optional { trust_alias }
        | ClientAuthenticationTemplate::Required { trust_alias } => {
            refs.push((trust_alias, "downstream_client_trust", false));
        }
        ClientAuthenticationTemplate::Disabled => {}
    }
}
