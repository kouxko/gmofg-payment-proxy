//! 单个代理入口、准入策略与 TLS 引用校验。

use std::collections::{BTreeMap, BTreeSet};
use std::net::IpAddr;

use super::{
    push_field_error,
    value::{
        is_valid_authority_pattern, is_valid_cidr, is_valid_dns_authority_pattern,
        is_valid_socket_host, is_valid_upstream_origin,
    },
};
use crate::{
    CertificateReferenceId, CertificateReferenceKind, DomainError, DownstreamClientAuthentication,
    FixedServerSettings, ForwardProxyAuthentication, HttpListenerSettings, ListenerDataPlane,
    MAX_SOCKET_MAXIMUM_CONNECTIONS, ProxyListener, SocketDownstreamSecurity,
    SocketDownstreamTlsSettings, SocketLocalResponderTopology, SocketPayloadProcessing,
    SocketRelaySecurity, SocketRelaySettings, SocketRelayTopology, SocketTopology,
    SocketUpstreamTlsSettings,
};

pub(crate) fn validate_listener(
    listener: &ProxyListener,
    index: usize,
    certificate_ids: &BTreeSet<CertificateReferenceId>,
    certificate_kinds: &BTreeMap<CertificateReferenceId, CertificateReferenceKind>,
    error: &mut DomainError,
) {
    let prefix = format!("listeners.{index}");
    validate_common(listener, &prefix, error);
    match &listener.data_plane {
        ListenerDataPlane::Http(settings) => validate_http(
            settings,
            listener.bind_address.parse::<IpAddr>().ok(),
            certificate_ids,
            certificate_kinds,
            &prefix,
            error,
        ),
        ListenerDataPlane::Socket(settings) => {
            validate_socket(settings, certificate_ids, certificate_kinds, &prefix, error);
        }
    }
}

fn validate_common(listener: &ProxyListener, prefix: &str, error: &mut DomainError) {
    if listener.name.trim().is_empty() {
        push_field_error(error, format!("{prefix}.name"), "监听器名称不能为空");
    }
    if listener.bind_address.parse::<IpAddr>().is_err() {
        push_field_error(
            error,
            format!("{prefix}.bind_address"),
            "绑定地址必须是有效 IP",
        );
    }
    if listener.port == 0 {
        push_field_error(error, format!("{prefix}.port"), "监听端口必须大于 0");
    }
    if listener.connect_timeout_ms == 0
        || listener.read_timeout_ms == 0
        || listener.write_timeout_ms == 0
    {
        push_field_error(error, format!("{prefix}.timeouts"), "超时必须大于 0 毫秒");
    }
    for (index, cidr) in listener.allowed_client_cidrs.iter().enumerate() {
        if !is_valid_cidr(cidr) {
            push_field_error(
                error,
                format!("{prefix}.allowed_client_cidrs.{index}"),
                "必须是有效 IPv4/IPv6 CIDR",
            );
        }
    }
}

fn validate_http(
    value: &HttpListenerSettings,
    bind_ip: Option<IpAddr>,
    certificate_ids: &BTreeSet<CertificateReferenceId>,
    certificate_kinds: &BTreeMap<CertificateReferenceId, CertificateReferenceKind>,
    prefix: &str,
    error: &mut DomainError,
) {
    if bind_ip.is_some_and(|ip| !ip.is_loopback())
        && value.fixed_server.is_none()
        && matches!(value.authentication, ForwardProxyAuthentication::None)
    {
        push_field_error(
            error,
            format!("{prefix}.data_plane.settings.authentication"),
            "非回环正向代理必须启用代理认证",
        );
    }
    validate_http_authentication(value, prefix, error);
    validate_mitm(value, certificate_ids, certificate_kinds, prefix, error);
    validate_http_downstream_tls(value, certificate_ids, certificate_kinds, prefix, error);
    if let Some(fixed_server) = &value.fixed_server {
        validate_fixed_server(
            fixed_server,
            certificate_ids,
            certificate_kinds,
            prefix,
            error,
        );
    }
}

fn validate_http_authentication(
    value: &HttpListenerSettings,
    prefix: &str,
    error: &mut DomainError,
) {
    if let ForwardProxyAuthentication::Basic { credential } = &value.authentication
        && (credential.provider.trim().is_empty() || credential.key.trim().is_empty())
    {
        push_field_error(
            error,
            format!("{prefix}.data_plane.settings.authentication.credential"),
            "认证秘密引用不能为空",
        );
    }
}

fn validate_mitm(
    value: &HttpListenerSettings,
    certificate_ids: &BTreeSet<CertificateReferenceId>,
    certificate_kinds: &BTreeMap<CertificateReferenceId, CertificateReferenceKind>,
    prefix: &str,
    error: &mut DomainError,
) {
    if !value.mitm.enabled {
        return;
    }
    let field = format!("{prefix}.data_plane.settings.mitm");
    if value.mitm.authority_allowlist.is_empty() {
        push_field_error(
            error,
            format!("{field}.authority_allowlist"),
            "启用 MITM 时必须配置显式允许列表",
        );
    }
    if !(1..=256).contains(&value.mitm.maximum_cached_leaf_certificates) {
        push_field_error(
            error,
            format!("{field}.maximum_cached_leaf_certificates"),
            "MITM 叶子证书缓存必须在 1..=256",
        );
    }
    validate_existing_reference(
        value.mitm.root_ca,
        CertificateReferenceKind::MitmRootCa,
        certificate_ids,
        certificate_kinds,
        format!("{field}.root_ca"),
        "MITM Root CA 引用不存在或类型不匹配",
        error,
    );
    for (index, authority) in value.mitm.authority_allowlist.iter().enumerate() {
        if !is_valid_authority_pattern(authority) {
            push_field_error(
                error,
                format!("{field}.authority_allowlist.{index}"),
                "必须是精确 DNS/IP 或 *.example.test 形式",
            );
        }
    }
}

fn validate_http_downstream_tls(
    value: &HttpListenerSettings,
    certificate_ids: &BTreeSet<CertificateReferenceId>,
    certificate_kinds: &BTreeMap<CertificateReferenceId, CertificateReferenceKind>,
    prefix: &str,
    error: &mut DomainError,
) {
    let tls = &value.downstream_tls;
    let field = format!("{prefix}.data_plane.settings.downstream_tls");
    validate_existing_reference(
        tls.server_identity,
        CertificateReferenceKind::ReverseServerIdentity,
        certificate_ids,
        certificate_kinds,
        format!("{field}.server_identity"),
        "下游 TLS 服务端身份引用不存在或类型不匹配",
        error,
    );
    for (index, authority) in tls.dynamic_sni_allowlist.iter().enumerate() {
        if !is_valid_dns_authority_pattern(authority) {
            push_field_error(
                error,
                format!("{field}.dynamic_sni_allowlist.{index}"),
                "必须是精确 DNS 或 *.example.test 形式；IP 请使用固定证书 SAN",
            );
        }
    }
    validate_client_authentication(
        &tls.client_authentication,
        certificate_ids,
        certificate_kinds,
        format!("{field}.client_authentication"),
        error,
    );
}

fn validate_fixed_server(
    value: &FixedServerSettings,
    certificate_ids: &BTreeSet<CertificateReferenceId>,
    certificate_kinds: &BTreeMap<CertificateReferenceId, CertificateReferenceKind>,
    prefix: &str,
    error: &mut DomainError,
) {
    let field = format!("{prefix}.data_plane.settings.fixed_server");
    if !is_valid_upstream_origin(&value.upstream_url) {
        push_field_error(
            error,
            format!("{field}.upstream_url"),
            "固定 Server 必须是 HTTP/HTTPS origin，不能包含路径、查询、片段或用户信息",
        );
    }
    let uses_https = value
        .upstream_url
        .get(..8)
        .is_some_and(|scheme| scheme.eq_ignore_ascii_case("https://"));
    if !uses_https
        && (value.upstream_tls.server_trust.is_some()
            || value.upstream_tls.client_identity.is_some())
    {
        push_field_error(
            error,
            format!("{field}.upstream_tls"),
            "Server CA 和 mTLS 客户端身份只能用于 HTTPS Server",
        );
    }
    validate_upstream_tls_references(
        value.upstream_tls.server_trust,
        value.upstream_tls.client_identity,
        certificate_ids,
        certificate_kinds,
        &format!("{field}.upstream_tls"),
        error,
    );
}

fn validate_socket(
    value: &SocketRelaySettings,
    certificate_ids: &BTreeSet<CertificateReferenceId>,
    certificate_kinds: &BTreeMap<CertificateReferenceId, CertificateReferenceKind>,
    prefix: &str,
    error: &mut DomainError,
) {
    let field = format!("{prefix}.data_plane.settings");
    if !(1..=MAX_SOCKET_MAXIMUM_CONNECTIONS).contains(&value.maximum_connections) {
        push_field_error(
            error,
            format!("{field}.maximum_connections"),
            "Socket 最大连接数必须在 1..=5000",
        );
    }
    match &value.topology {
        SocketTopology::Relay(relay) => {
            validate_socket_relay(relay, certificate_ids, certificate_kinds, &field, error);
        }
        SocketTopology::LocalResponder(local) => validate_socket_local_responder(
            local,
            &value.processing,
            certificate_ids,
            certificate_kinds,
            &field,
            error,
        ),
    }
}

fn validate_socket_relay(
    value: &SocketRelayTopology,
    certificate_ids: &BTreeSet<CertificateReferenceId>,
    certificate_kinds: &BTreeMap<CertificateReferenceId, CertificateReferenceKind>,
    field: &str,
    error: &mut DomainError,
) {
    if !is_valid_socket_host(&value.upstream.host) {
        push_field_error(
            error,
            format!("{field}.topology.settings.upstream.host"),
            "Socket 上游必须是 DNS 主机名或 IP，不能包含 URL、路径、查询或用户信息",
        );
    }
    if value.upstream.port == 0 {
        push_field_error(
            error,
            format!("{field}.topology.settings.upstream.port"),
            "Socket 上游端口必须大于 0",
        );
    }
    let security_field = format!("{field}.topology.settings.security");
    match &value.security {
        SocketRelaySecurity::Transparent => {}
        SocketRelaySecurity::TcpToTls { upstream_tls } => validate_socket_upstream_tls(
            upstream_tls,
            certificate_ids,
            certificate_kinds,
            &security_field,
            error,
        ),
        SocketRelaySecurity::TlsToTcp { downstream_tls } => validate_socket_downstream_tls(
            downstream_tls,
            certificate_ids,
            certificate_kinds,
            &security_field,
            error,
        ),
        SocketRelaySecurity::TlsToTls {
            downstream_tls,
            upstream_tls,
        } => {
            validate_socket_downstream_tls(
                downstream_tls,
                certificate_ids,
                certificate_kinds,
                &security_field,
                error,
            );
            validate_socket_upstream_tls(
                upstream_tls,
                certificate_ids,
                certificate_kinds,
                &security_field,
                error,
            );
        }
    }
}

fn validate_socket_local_responder(
    value: &SocketLocalResponderTopology,
    processing: &SocketPayloadProcessing,
    certificate_ids: &BTreeSet<CertificateReferenceId>,
    certificate_kinds: &BTreeMap<CertificateReferenceId, CertificateReferenceKind>,
    field: &str,
    error: &mut DomainError,
) {
    if let SocketDownstreamSecurity::Tls { downstream_tls } = &value.downstream_security {
        validate_socket_downstream_tls(
            downstream_tls,
            certificate_ids,
            certificate_kinds,
            &format!("{field}.topology.settings.downstream_security"),
            error,
        );
    }

    let SocketPayloadProcessing::Scripted(_) = processing else {
        push_field_error(
            error,
            format!("{field}.processing"),
            "LocalResponder 只支持 Scripted 数据处理模式",
        );
        return;
    };
}

fn validate_socket_downstream_tls(
    value: &SocketDownstreamTlsSettings,
    certificate_ids: &BTreeSet<CertificateReferenceId>,
    certificate_kinds: &BTreeMap<CertificateReferenceId, CertificateReferenceKind>,
    prefix: &str,
    error: &mut DomainError,
) {
    validate_existing_reference(
        Some(value.server_identity),
        CertificateReferenceKind::ReverseServerIdentity,
        certificate_ids,
        certificate_kinds,
        format!("{prefix}.downstream_tls.server_identity"),
        "Socket 下游服务端身份引用不存在或类型不匹配",
        error,
    );
    validate_client_authentication(
        &value.client_authentication,
        certificate_ids,
        certificate_kinds,
        format!("{prefix}.downstream_tls.client_authentication"),
        error,
    );
}

fn validate_socket_upstream_tls(
    value: &SocketUpstreamTlsSettings,
    certificate_ids: &BTreeSet<CertificateReferenceId>,
    certificate_kinds: &BTreeMap<CertificateReferenceId, CertificateReferenceKind>,
    prefix: &str,
    error: &mut DomainError,
) {
    validate_upstream_tls_references(
        value.server_trust,
        value.client_identity,
        certificate_ids,
        certificate_kinds,
        &format!("{prefix}.upstream_tls"),
        error,
    );
}

fn validate_client_authentication(
    value: &DownstreamClientAuthentication,
    certificate_ids: &BTreeSet<CertificateReferenceId>,
    certificate_kinds: &BTreeMap<CertificateReferenceId, CertificateReferenceKind>,
    field: String,
    error: &mut DomainError,
) {
    let reference = match value {
        DownstreamClientAuthentication::Disabled => None,
        DownstreamClientAuthentication::Optional { trust }
        | DownstreamClientAuthentication::Required { trust } => Some(*trust),
    };
    validate_existing_reference(
        reference,
        CertificateReferenceKind::DownstreamClientTrust,
        certificate_ids,
        certificate_kinds,
        field,
        "下游客户端信任引用不存在或类型不匹配",
        error,
    );
}

fn validate_upstream_tls_references(
    server_trust: Option<CertificateReferenceId>,
    client_identity: Option<CertificateReferenceId>,
    certificate_ids: &BTreeSet<CertificateReferenceId>,
    certificate_kinds: &BTreeMap<CertificateReferenceId, CertificateReferenceKind>,
    prefix: &str,
    error: &mut DomainError,
) {
    for (field, reference, kind, message) in [
        (
            "server_trust",
            server_trust,
            CertificateReferenceKind::UpstreamServerTrust,
            "上游 Server CA 引用不存在或类型不匹配",
        ),
        (
            "client_identity",
            client_identity,
            CertificateReferenceKind::UpstreamClientIdentity,
            "上游 mTLS 客户端身份引用不存在或类型不匹配",
        ),
    ] {
        validate_existing_reference(
            reference,
            kind,
            certificate_ids,
            certificate_kinds,
            format!("{prefix}.{field}"),
            message,
            error,
        );
    }
}

fn validate_existing_reference(
    reference: Option<CertificateReferenceId>,
    expected: CertificateReferenceKind,
    certificate_ids: &BTreeSet<CertificateReferenceId>,
    certificate_kinds: &BTreeMap<CertificateReferenceId, CertificateReferenceKind>,
    field: String,
    message: &str,
    error: &mut DomainError,
) {
    if reference.is_some_and(|id| {
        !certificate_ids.contains(&id)
            || certificate_kinds
                .get(&id)
                .is_some_and(|kind| *kind != expected)
    }) {
        push_field_error(error, field, message);
    }
}
