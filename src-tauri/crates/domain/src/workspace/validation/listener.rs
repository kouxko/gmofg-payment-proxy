//! 单个代理入口、准入策略与 TLS 引用校验。

use std::collections::{BTreeMap, BTreeSet};
use std::net::IpAddr;

use super::{
    push_field_error,
    value::{is_valid_authority_pattern, is_valid_dns_authority_pattern, is_valid_upstream_origin},
};
use crate::{
    CertificateReferenceId, CertificateReferenceKind, DomainError, DownstreamClientAuthentication,
    FixedServerSettings, ForwardProxyAuthentication, HttpListenerSettings, ListenerDataPlane,
    ProxyListener,
};

mod socket;

use socket::validate_socket;

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
        && value.uses_request_target()
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
    if let Some(fixed_server) = value.fixed_server() {
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
    let field = format!("{prefix}.data_plane.settings.topology.settings.fixed_server");
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
