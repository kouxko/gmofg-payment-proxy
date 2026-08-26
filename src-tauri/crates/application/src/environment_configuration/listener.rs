use serde::{Deserialize, Serialize};
use uuid::Uuid;

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
