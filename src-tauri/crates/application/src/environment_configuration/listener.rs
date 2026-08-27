use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::{AppError, AppResult};

mod projection;

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub(super) struct ListenerTemplate {
    pub(super) alias: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(super) id: Option<Uuid>,
    name: String,
    enabled: bool,
    bind_address: String,
    port: u16,
    connect_timeout_ms: u64,
    read_timeout_ms: u64,
    write_timeout_ms: u64,
    data_plane: ListenerDataPlaneTemplate,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(
    tag = "kind",
    content = "settings",
    rename_all = "snake_case",
    deny_unknown_fields
)]
enum ListenerDataPlaneTemplate {
    Http(HttpListenerTemplate),
    Socket(SocketListenerTemplate),
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
struct HttpListenerTemplate {
    authentication: AuthenticationTemplate,
    mitm: MitmTemplate,
    downstream_tls: DownstreamTlsTemplate,
    request_body_codec: BodyCodec,
    response_body_codec: BodyCodec,
    body_processing: BodyProcessingTemplate,
    fixed_server: Option<FixedServerTemplate>,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(tag = "mode", rename_all = "snake_case", deny_unknown_fields)]
enum AuthenticationTemplate {
    None,
    Basic { credential_alias: String },
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
struct MitmTemplate {
    enabled: bool,
    authority_allowlist: Vec<String>,
    root_ca_selector: Option<String>,
    maximum_cached_leaf_certificates: u16,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
struct DownstreamTlsTemplate {
    enabled: bool,
    server_identity_alias: Option<String>,
    dynamic_sni_allowlist: Vec<String>,
    client_authentication: ClientAuthenticationTemplate,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(tag = "mode", rename_all = "snake_case", deny_unknown_fields)]
enum ClientAuthenticationTemplate {
    Disabled,
    Optional { trust_alias: String },
    Required { trust_alias: String },
}

#[derive(Clone, Copy, Debug, Deserialize, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
enum BodyCodec {
    Auto,
    Raw,
    Utf8,
    ShiftJis,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(tag = "mode", rename_all = "snake_case", deny_unknown_fields)]
enum BodyProcessingTemplate {
    Plain,
    Protocol { package: ProtocolPackageExactRef },
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
struct FixedServerTemplate {
    upstream_url: String,
    upstream_tls: HttpUpstreamTlsTemplate,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
struct HttpUpstreamTlsTemplate {
    verify_hostname: bool,
    server_trust_alias: Option<String>,
    client_identity_alias: Option<String>,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
struct SocketListenerTemplate {
    topology: SocketTopologyTemplate,
    maximum_connections: u16,
    runtime_limits: SocketRuntimeLimitsTemplate,
    processing: SocketPayloadProcessingTemplate,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(
    tag = "mode",
    content = "settings",
    rename_all = "snake_case",
    deny_unknown_fields
)]
enum SocketTopologyTemplate {
    Relay(SocketRelayTemplate),
    LocalResponder(LocalResponderTemplate),
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
struct SocketRelayTemplate {
    upstream: SocketUpstream,
    security: SocketRelaySecurityTemplate,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
struct SocketUpstream {
    host: String,
    port: u16,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(tag = "mode", rename_all = "snake_case", deny_unknown_fields)]
enum SocketRelaySecurityTemplate {
    Transparent,
    TcpToTls {
        upstream_tls: SocketUpstreamTlsTemplate,
    },
    TlsToTcp {
        downstream_tls: SocketDownstreamTlsTemplate,
    },
    TlsToTls {
        downstream_tls: SocketDownstreamTlsTemplate,
        upstream_tls: SocketUpstreamTlsTemplate,
    },
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
struct LocalResponderTemplate {
    downstream_security: SocketDownstreamSecurityTemplate,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(tag = "mode", rename_all = "snake_case", deny_unknown_fields)]
enum SocketDownstreamSecurityTemplate {
    Tcp,
    Tls {
        downstream_tls: SocketDownstreamTlsTemplate,
    },
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
struct SocketDownstreamTlsTemplate {
    server_identity_alias: String,
    client_authentication: ClientAuthenticationTemplate,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
struct SocketUpstreamTlsTemplate {
    verify_hostname: bool,
    tls_server_name: Option<String>,
    server_trust_alias: Option<String>,
    client_identity_alias: Option<String>,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
struct SocketRuntimeLimitsTemplate {
    read_chunk_bytes: u32,
    diagnostic_event_capacity: u32,
    diagnostic_memory_bytes: u64,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(
    tag = "mode",
    content = "settings",
    rename_all = "snake_case",
    deny_unknown_fields
)]
enum SocketPayloadProcessingTemplate {
    Direct,
    Scripted(ScriptedProcessingTemplate),
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
struct ScriptedProcessingTemplate {
    package: ProtocolPackageExactRef,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub(super) struct ProtocolPackageExactRef {
    pub(super) id: String,
    pub(super) version: String,
}

pub(super) struct ListenerNetworkTarget {
    pub(super) host: String,
    pub(super) port: u16,
    pub(super) uses_tls: bool,
    pub(super) server_name: Option<String>,
    pub(super) upstream_ca_alias: Option<String>,
    pub(super) client_identity_alias: Option<String>,
    pub(super) verify_hostname: bool,
}

impl ListenerTemplate {
    pub(super) const fn accepts_http_rules(&self) -> bool {
        matches!(self.data_plane, ListenerDataPlaneTemplate::Http(_))
    }

    pub(super) const fn accepts_protocol_rules(&self) -> bool {
        matches!(self.data_plane, ListenerDataPlaneTemplate::Socket(_))
    }

    pub(super) fn alias(&self) -> &str {
        &self.alias
    }

    pub(super) fn enabled_endpoint(&self) -> Option<(&str, u16)> {
        self.enabled
            .then_some((self.bind_address.as_str(), self.port))
    }

    pub(super) fn package_refs(&self) -> Vec<&ProtocolPackageExactRef> {
        match &self.data_plane {
            ListenerDataPlaneTemplate::Http(http) => match &http.body_processing {
                BodyProcessingTemplate::Protocol { package } => vec![package],
                BodyProcessingTemplate::Plain => Vec::new(),
            },
            ListenerDataPlaneTemplate::Socket(socket) => match &socket.processing {
                SocketPayloadProcessingTemplate::Scripted(settings) => vec![&settings.package],
                SocketPayloadProcessingTemplate::Direct => Vec::new(),
            },
        }
    }

    pub(super) fn network_target(&self) -> AppResult<Option<ListenerNetworkTarget>> {
        match &self.data_plane {
            ListenerDataPlaneTemplate::Http(http) => {
                let Some(fixed) = http.fixed_server.as_ref() else {
                    return Ok(None);
                };
                let uri = fixed
                    .upstream_url
                    .parse::<http::Uri>()
                    .map_err(|_| domain_error())?;
                if !matches!(uri.scheme_str(), Some("http" | "https")) {
                    return Err(domain_error());
                }
                let host = uri.host().ok_or_else(domain_error)?.to_owned();
                let tls = uri.scheme_str() == Some("https");
                let port = uri.port_u16().unwrap_or(if tls { 443 } else { 80 });
                Ok(Some(ListenerNetworkTarget {
                    host: host.clone(),
                    port,
                    uses_tls: tls,
                    server_name: tls.then_some(host),
                    upstream_ca_alias: fixed.upstream_tls.server_trust_alias.clone(),
                    client_identity_alias: fixed.upstream_tls.client_identity_alias.clone(),
                    verify_hostname: fixed.upstream_tls.verify_hostname,
                }))
            }
            ListenerDataPlaneTemplate::Socket(socket) => match &socket.topology {
                SocketTopologyTemplate::Relay(relay) => {
                    if !intercept_proxy_domain::is_valid_socket_host(&relay.upstream.host)
                        || relay.upstream.port == 0
                    {
                        return Err(domain_error());
                    }
                    let (
                        tls,
                        server_name,
                        upstream_ca_alias,
                        client_identity_alias,
                        verify_hostname,
                    ) = match &relay.security {
                        SocketRelaySecurityTemplate::TcpToTls { upstream_tls }
                        | SocketRelaySecurityTemplate::TlsToTls { upstream_tls, .. } => (
                            true,
                            upstream_tls.tls_server_name.clone(),
                            upstream_tls.server_trust_alias.clone(),
                            upstream_tls.client_identity_alias.clone(),
                            upstream_tls.verify_hostname,
                        ),
                        SocketRelaySecurityTemplate::Transparent
                        | SocketRelaySecurityTemplate::TlsToTcp { .. } => {
                            (false, None, None, None, false)
                        }
                    };
                    Ok(Some(ListenerNetworkTarget {
                        host: relay.upstream.host.clone(),
                        port: relay.upstream.port,
                        uses_tls: tls,
                        server_name,
                        upstream_ca_alias,
                        client_identity_alias,
                        verify_hostname,
                    }))
                }
                SocketTopologyTemplate::LocalResponder(_) => Ok(None),
            },
        }
    }

    pub(super) fn validate_domain(&self) -> AppResult<()> {
        if self.alias.trim().is_empty()
            || self.name.trim().is_empty()
            || self.port == 0
            || self.bind_address.parse::<std::net::IpAddr>().is_err()
            || self.connect_timeout_ms == 0
            || self.read_timeout_ms == 0
            || self.write_timeout_ms == 0
        {
            return Err(domain_error());
        }
        if let ListenerDataPlaneTemplate::Http(http) = &self.data_plane
            && http.fixed_server.as_ref().is_some_and(|fixed| {
                !intercept_proxy_domain::is_valid_upstream_origin(&fixed.upstream_url)
            })
        {
            return Err(domain_error());
        }
        if let ListenerDataPlaneTemplate::Socket(socket) = &self.data_plane
            && (!(1..=5_000).contains(&socket.maximum_connections)
                || socket.runtime_limits.read_chunk_bytes == 0
                || socket.runtime_limits.diagnostic_event_capacity == 0
                || socket.runtime_limits.diagnostic_memory_bytes == 0)
        {
            return Err(domain_error());
        }
        if let ListenerDataPlaneTemplate::Http(http) = &self.data_plane
            && ((http.mitm.enabled
                && http.mitm.root_ca_selector.as_deref() != Some("installation:root-ca"))
                || (!http.mitm.enabled && http.mitm.root_ca_selector.is_some())
                || (http.downstream_tls.enabled
                    && http.downstream_tls.server_identity_alias.is_none())
                || (!http.downstream_tls.enabled
                    && http.downstream_tls.server_identity_alias.is_some()))
        {
            return Err(domain_error());
        }
        self.network_target().map(|_| ())
    }

    pub(super) fn referenced_materials(&self) -> Vec<(&str, &'static str, bool)> {
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
                if let Some(fixed) = &http.fixed_server {
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

    pub(super) fn installation_root_selector(&self) -> Option<&str> {
        match &self.data_plane {
            ListenerDataPlaneTemplate::Http(http) if http.mitm.enabled => {
                http.mitm.root_ca_selector.as_deref()
            }
            _ => None,
        }
    }
}

fn domain_error() -> AppError {
    AppError::new(
        "LISTENER_DOMAIN_INVALID",
        "listener domain validation failed",
    )
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
