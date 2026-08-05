//! 单个代理入口、准入策略与 TLS 引用校验。

use std::collections::{BTreeMap, BTreeSet};
use std::net::IpAddr;

use super::{
    push_field_error,
    value::{
        is_valid_authority_pattern, is_valid_cidr, is_valid_dns_authority_pattern,
        is_valid_upstream_origin,
    },
};
use crate::{
    CertificateReferenceId, CertificateReferenceKind, DomainError, DownstreamClientAuthentication,
    FixedServerSettings, ForwardProxyAuthentication, ProxyListener,
};

pub(crate) fn validate_listener(
    listener: &ProxyListener,
    index: usize,
    certificate_ids: &BTreeSet<CertificateReferenceId>,
    certificate_kinds: &BTreeMap<CertificateReferenceId, CertificateReferenceKind>,
    error: &mut DomainError,
) {
    let prefix = format!("listeners.{index}");
    if listener.name.trim().is_empty() {
        push_field_error(error, format!("{prefix}.name"), "监听器名称不能为空");
    }
    let bind_ip = listener.bind_address.parse::<IpAddr>();
    if bind_ip.is_err() {
        push_field_error(
            error,
            format!("{prefix}.bind_address"),
            "绑定地址必须是有效 IP",
        );
    }
    if listener.port == 0 {
        push_field_error(error, format!("{prefix}.port"), "监听端口必须大于 0");
    }

    validate_listener_access(
        listener,
        bind_ip.ok(),
        certificate_ids,
        certificate_kinds,
        &prefix,
        error,
    );
    validate_downstream_tls(listener, certificate_ids, certificate_kinds, &prefix, error);
    if let Some(fixed_server) = &listener.fixed_server {
        validate_fixed_server(
            fixed_server,
            certificate_ids,
            certificate_kinds,
            &prefix,
            error,
        );
    }
}

fn validate_listener_access(
    value: &ProxyListener,
    bind_ip: Option<IpAddr>,
    certificate_ids: &BTreeSet<CertificateReferenceId>,
    certificate_kinds: &BTreeMap<CertificateReferenceId, CertificateReferenceKind>,
    prefix: &str,
    error: &mut DomainError,
) {
    if value.connect_timeout_ms == 0 || value.read_timeout_ms == 0 || value.write_timeout_ms == 0 {
        push_field_error(error, format!("{prefix}.timeouts"), "超时必须大于 0 毫秒");
    }
    for (cidr_index, cidr) in value.allowed_client_cidrs.iter().enumerate() {
        if !is_valid_cidr(cidr) {
            push_field_error(
                error,
                format!("{prefix}.allowed_client_cidrs.{cidr_index}"),
                "必须是有效 IPv4/IPv6 CIDR",
            );
        }
    }
    // 任何非回环监听都必须有 CIDR 准入。动态正向代理还要求 HTTP 代理认证。
    // 固定 Server 入口允许使用明文、普通 TLS 或 mTLS；客户端证书验证仅在用户
    // 显式配置 Optional/Required 时启用，不能由绑定地址隐式改变协议语义。
    if bind_ip.is_some_and(|ip| !ip.is_loopback()) {
        if value.allowed_client_cidrs.is_empty() {
            push_field_error(
                error,
                format!("{prefix}.allowed_client_cidrs"),
                "非回环监听必须配置客户端 CIDR 白名单",
            );
        }
        if value.fixed_server.is_none()
            && matches!(value.authentication, ForwardProxyAuthentication::None)
        {
            push_field_error(
                error,
                format!("{prefix}.authentication"),
                "非回环正向代理必须启用代理认证",
            );
        }
    }
    if let ForwardProxyAuthentication::Basic { credential } = &value.authentication
        && (credential.provider.trim().is_empty() || credential.key.trim().is_empty())
    {
        push_field_error(
            error,
            format!("{prefix}.authentication.credential"),
            "认证秘密引用不能为空",
        );
    }
    if value.mitm.enabled {
        if value.mitm.authority_allowlist.is_empty() {
            push_field_error(
                error,
                format!("{prefix}.mitm.authority_allowlist"),
                "启用 MITM 时必须配置显式允许列表",
            );
        }
        if value.mitm.maximum_cached_leaf_certificates == 0
            || value.mitm.maximum_cached_leaf_certificates > 256
        {
            push_field_error(
                error,
                format!("{prefix}.mitm.maximum_cached_leaf_certificates"),
                "MITM 叶子证书缓存必须在 1..=256",
            );
        }
        // `None` 表示使用当前安装实例首次启动时生成并受系统密钥保护的 Root CA。
        // 只有用户显式提供 Workspace 证书引用时才校验该引用，避免为了使用默认安装级
        // Root 而伪造文件路径或把私钥材料写入 Workspace。
        if value
            .mitm
            .root_ca
            .is_some_and(|id| !certificate_ids.contains(&id))
        {
            push_field_error(
                error,
                format!("{prefix}.mitm.root_ca"),
                "MITM Root CA 引用不存在；留空可使用当前安装实例 Root CA",
            );
        } else {
            validate_certificate_role(
                value.mitm.root_ca,
                CertificateReferenceKind::MitmRootCa,
                certificate_kinds,
                format!("{prefix}.mitm.root_ca"),
                "MITM Root CA 引用类型不匹配",
                error,
            );
        }
        for (allow_index, authority) in value.mitm.authority_allowlist.iter().enumerate() {
            if !is_valid_authority_pattern(authority) {
                push_field_error(
                    error,
                    format!("{prefix}.mitm.authority_allowlist.{allow_index}"),
                    "必须是精确 DNS/IP 或 *.example.test 形式",
                );
            }
        }
    }
}

fn validate_downstream_tls(
    value: &ProxyListener,
    certificate_ids: &BTreeSet<CertificateReferenceId>,
    certificate_kinds: &BTreeMap<CertificateReferenceId, CertificateReferenceKind>,
    prefix: &str,
    error: &mut DomainError,
) {
    // `None` 明确表示使用当前安装实例的 Root CA 按客户端 SNI 动态签发叶子证书。
    // 只有用户改选固定 Workspace 身份时才要求该引用存在。
    if value.downstream_tls.enabled
        && value
            .downstream_tls
            .server_identity
            .is_some_and(|id| !certificate_ids.contains(&id))
    {
        push_field_error(
            error,
            format!("{prefix}.downstream_tls.server_identity"),
            "下游 TLS 服务端身份引用不存在；留空可使用证书管理页签发的本机叶子证书",
        );
    } else {
        validate_certificate_role(
            value.downstream_tls.server_identity,
            CertificateReferenceKind::ReverseServerIdentity,
            certificate_kinds,
            format!("{prefix}.downstream_tls.server_identity"),
            "下游 TLS 服务端身份引用类型不匹配",
            error,
        );
    }
    for (allow_index, authority) in value
        .downstream_tls
        .dynamic_sni_allowlist
        .iter()
        .enumerate()
    {
        if !is_valid_dns_authority_pattern(authority) {
            push_field_error(
                error,
                format!("{prefix}.downstream_tls.dynamic_sni_allowlist.{allow_index}"),
                "必须是精确 DNS 或 *.example.test 形式；IP 请使用固定证书 SAN",
            );
        }
    }
    let downstream_trust = match value.downstream_tls.client_authentication {
        DownstreamClientAuthentication::Disabled => None,
        DownstreamClientAuthentication::Optional { trust }
        | DownstreamClientAuthentication::Required { trust } => Some(trust),
    };
    if downstream_trust.is_some_and(|id| !certificate_ids.contains(&id)) {
        push_field_error(
            error,
            format!("{prefix}.downstream_tls.client_authentication"),
            "下游客户端信任引用不存在",
        );
    } else {
        validate_certificate_role(
            downstream_trust,
            CertificateReferenceKind::DownstreamClientTrust,
            certificate_kinds,
            format!("{prefix}.downstream_tls.client_authentication"),
            "下游客户端信任引用类型不匹配",
            error,
        );
    }
}

fn validate_fixed_server(
    value: &FixedServerSettings,
    certificate_ids: &BTreeSet<CertificateReferenceId>,
    certificate_kinds: &BTreeMap<CertificateReferenceId, CertificateReferenceKind>,
    prefix: &str,
    error: &mut DomainError,
) {
    let fixed_prefix = format!("{prefix}.fixed_server");
    if !is_valid_upstream_origin(&value.upstream_url) {
        push_field_error(
            error,
            format!("{fixed_prefix}.upstream_url"),
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
            format!("{fixed_prefix}.upstream_tls"),
            "Server CA 和 mTLS 客户端身份只能用于 HTTPS Server",
        );
    }
    for (field, reference) in [
        ("server_trust", value.upstream_tls.server_trust),
        ("client_identity", value.upstream_tls.client_identity),
    ] {
        if reference.is_some_and(|id| !certificate_ids.contains(&id)) {
            push_field_error(
                error,
                format!("{fixed_prefix}.upstream_tls.{field}"),
                "Server TLS 证书引用不存在",
            );
        }
    }
    validate_certificate_role(
        value.upstream_tls.server_trust,
        CertificateReferenceKind::UpstreamServerTrust,
        certificate_kinds,
        format!("{fixed_prefix}.upstream_tls.server_trust"),
        "上游 Server CA 引用类型不匹配",
        error,
    );
    validate_certificate_role(
        value.upstream_tls.client_identity,
        CertificateReferenceKind::UpstreamClientIdentity,
        certificate_kinds,
        format!("{fixed_prefix}.upstream_tls.client_identity"),
        "上游 mTLS 客户端身份引用类型不匹配",
        error,
    );
}

fn validate_certificate_role(
    reference: Option<CertificateReferenceId>,
    expected: CertificateReferenceKind,
    certificate_kinds: &BTreeMap<CertificateReferenceId, CertificateReferenceKind>,
    field: String,
    message: &str,
    error: &mut DomainError,
) {
    if reference.is_some_and(|id| {
        certificate_kinds
            .get(&id)
            .is_some_and(|kind| *kind != expected)
    }) {
        push_field_error(error, field, message);
    }
}
