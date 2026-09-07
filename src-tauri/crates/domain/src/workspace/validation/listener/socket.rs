//! Socket topology, resource limit, and TLS reference validation.

use std::collections::{BTreeMap, BTreeSet};

use crate::{
    CertificateReferenceId, CertificateReferenceKind, DomainError, MAX_SOCKET_MAXIMUM_CONNECTIONS,
    SocketDownstreamSecurity, SocketDownstreamTlsSettings, SocketLocalResponderTopology,
    SocketRelaySecurity, SocketRelaySettings, SocketRelayTopology, SocketTopology,
    SocketUpstreamTlsSettings,
};

use super::super::value::is_valid_socket_host;
use super::{
    push_field_error, validate_client_authentication, validate_existing_reference,
    validate_upstream_tls_references,
};

pub(super) fn validate_socket(
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
    if value.runtime_limits.read_chunk_bytes == 0 {
        push_field_error(
            error,
            format!("{field}.runtime_limits.read_chunk_bytes"),
            "Socket 单次读取字节数必须大于 0",
        );
    }
    if value.runtime_limits.diagnostic_event_capacity == 0 {
        push_field_error(
            error,
            format!("{field}.runtime_limits.diagnostic_event_capacity"),
            "Socket 诊断事件容量必须大于 0",
        );
    }
    if value.runtime_limits.diagnostic_memory_bytes == 0 {
        push_field_error(
            error,
            format!("{field}.runtime_limits.diagnostic_memory_bytes"),
            "Socket 诊断内存容量必须大于 0",
        );
    }
    match &value.topology {
        SocketTopology::Relay(relay) => {
            validate_socket_relay(relay, certificate_ids, certificate_kinds, &field, error);
        }
        SocketTopology::LocalResponder(local) => {
            validate_socket_local_responder(
                local,
                certificate_ids,
                certificate_kinds,
                &field,
                error,
            );
        }
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
    if value
        .tls_server_name
        .as_deref()
        .is_some_and(|server_name| !is_valid_socket_host(server_name))
    {
        push_field_error(
            error,
            format!("{prefix}.upstream_tls.tls_server_name"),
            "TLS Server Name 必须是精确 DNS 主机名或 IP，不能包含端口、URL 或路径",
        );
    }
    validate_upstream_tls_references(
        value.server_trust,
        value.client_identity,
        certificate_ids,
        certificate_kinds,
        &format!("{prefix}.upstream_tls"),
        error,
    );
}
