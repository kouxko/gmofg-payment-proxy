use super::{
    AppResult, DownstreamClientAuthentication, ListenerRuntimePlanBuilder, ProxyWorkspace,
    SocketDownstreamTlsConfig, SocketDownstreamTlsSettings, SocketTlsIdentity,
    SocketUpstreamTlsConfig, SocketUpstreamTlsSettings,
};

impl ListenerRuntimePlanBuilder<'_> {
    pub(super) fn socket_downstream_tls(
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

    pub(super) fn socket_upstream_tls(
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
            tls_server_name: settings.tls_server_name.clone(),
        })
    }
}
