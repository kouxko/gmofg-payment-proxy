use super::{
    AppError, AppResult, ApplicationSocketMode, DomainSocketSecurity, DomainSocketTopology,
    ListenerDataPlaneKind, ListenerRuntimeState, ListenerStatusViewModel,
    ListenerUpstreamConnectionTestViewModel, ListenerUpstreamTlsEvidenceViewModel, ProxyListener,
    SocketRelayMetricsSnapshot, StatusSnapshot, UiTone, UpstreamScheme, UpstreamTransport,
};

pub(super) fn status_from_snapshot(
    snapshot: StatusSnapshot,
    metrics: SocketRelayMetricsSnapshot,
) -> ListenerStatusViewModel {
    let faulted = snapshot.finished || snapshot.fault_reason.is_some();
    ListenerStatusViewModel {
        listener_id: snapshot.listener_id,
        state: if faulted {
            ListenerRuntimeState::Faulted
        } else {
            ListenerRuntimeState::Running
        },
        state_text: if faulted { "故障" } else { "运行中" }.into(),
        ui_tone: if faulted {
            UiTone::Danger
        } else {
            UiTone::Positive
        },
        listen_address: snapshot.listen_address,
        fault_reason: snapshot
            .fault_reason
            .or_else(|| faulted.then(|| "Listener 任务已意外结束。".into())),
        can_start: false,
        can_stop: true,
        active_connections: u32::try_from(metrics.active_connections).unwrap_or(u32::MAX),
        client_to_server_bytes: metrics.client_to_server_bytes,
        server_to_client_bytes: metrics.server_to_client_bytes,
        retained_diagnostic_evictions: metrics.retained_diagnostic_evictions,
    }
}

pub(super) fn http_probe_view(
    listener: &ProxyListener,
    result: intercept_proxy_runtime::UpstreamConnectionTestResult,
) -> ListenerUpstreamConnectionTestViewModel {
    let tls = result.tls.map(tls_evidence_view);
    let scheme = match result.scheme {
        UpstreamScheme::Http => "http",
        UpstreamScheme::Https => "https",
    };
    let transport = match result.transport {
        UpstreamTransport::Tcp => "tcp",
        UpstreamTransport::Tls => "tls",
    };
    let origin = listener
        .http()
        .and_then(|http| http.fixed_server.as_ref())
        .map_or_else(String::new, |fixed| fixed.upstream_url.clone());
    ListenerUpstreamConnectionTestViewModel {
        listener_id: listener.id,
        data_plane: ListenerDataPlaneKind::Http,
        upstream_origin: origin,
        resolved_address: result.resolved_address.to_string(),
        scheme: scheme.into(),
        transport: transport.into(),
        tls,
        socket_transport_mode: None,
        tls_server_name_candidates: Vec::new(),
        elapsed_millis: result.elapsed_millis,
        message: format!("上游 Server {transport} 连接成功。"),
        ui_tone: UiTone::Positive,
    }
}

pub(super) fn socket_probe_view(
    listener: &ProxyListener,
    result: intercept_proxy_runtime::SocketUpstreamConnectionTestResult,
) -> AppResult<ListenerUpstreamConnectionTestViewModel> {
    let Some(settings) = listener.socket() else {
        return Err(AppError::new(
            "LISTENER_DATA_PLANE_MISMATCH",
            "Socket 上游测试结果与 Listener 数据面不匹配。",
        )
        .entity(listener.id.to_string()));
    };
    let DomainSocketTopology::Relay(relay) = &settings.topology else {
        return Err(AppError::new(
            "LISTENER_UPSTREAM_NOT_APPLICABLE",
            "本地应答没有可测试的 Server 上游。",
        )
        .entity(listener.id.to_string()));
    };
    let transport = match result.transport {
        intercept_proxy_runtime::SocketUpstreamTransport::Tcp => "tcp",
        intercept_proxy_runtime::SocketUpstreamTransport::Tls => "tls",
    };
    let tls_server_name_candidates = result.tls_server_name_candidates;
    let discovered_server_name = !tls_server_name_candidates.is_empty();
    Ok(ListenerUpstreamConnectionTestViewModel {
        listener_id: listener.id,
        data_plane: ListenerDataPlaneKind::Socket,
        upstream_origin: format!("{}:{}", relay.upstream.host, relay.upstream.port),
        resolved_address: result.resolved_address.to_string(),
        scheme: "socket".into(),
        transport: transport.into(),
        tls: result.tls.map(socket_tls_evidence_view),
        socket_transport_mode: Some(socket_mode(&relay.security)),
        tls_server_name_candidates,
        elapsed_millis: result.elapsed_millis,
        message: if discovered_server_name {
            "Server 证书链验证成功，已自动填写 TLS Server Name；请再次测试以完成严格主机名校验。"
                .into()
        } else {
            format!("上游 Socket {transport} 连接成功。")
        },
        ui_tone: if discovered_server_name {
            UiTone::Warning
        } else {
            UiTone::Positive
        },
    })
}

fn tls_evidence_view(
    evidence: intercept_proxy_runtime::UpstreamTlsHandshakeResult,
) -> ListenerUpstreamTlsEvidenceViewModel {
    ListenerUpstreamTlsEvidenceViewModel {
        tls_version: evidence.tls_version,
        cipher_suite: evidence.cipher_suite,
        peer_subject: evidence.peer_subject,
        peer_sha256_fingerprint: evidence.peer_sha256_fingerprint,
        hostname_verification_enabled: evidence.hostname_verification_enabled,
        client_identity_configured: evidence.client_identity_configured,
    }
}

fn socket_tls_evidence_view(
    evidence: intercept_proxy_runtime::SocketTlsEvidence,
) -> ListenerUpstreamTlsEvidenceViewModel {
    ListenerUpstreamTlsEvidenceViewModel {
        tls_version: evidence.tls_version,
        cipher_suite: evidence.cipher_suite,
        peer_subject: evidence.peer_subject,
        peer_sha256_fingerprint: evidence.peer_sha256_fingerprint,
        hostname_verification_enabled: evidence.hostname_verification_enabled,
        client_identity_configured: evidence.client_identity_configured,
    }
}

fn socket_mode(security: &DomainSocketSecurity) -> ApplicationSocketMode {
    match security {
        DomainSocketSecurity::Transparent => ApplicationSocketMode::Transparent,
        DomainSocketSecurity::TcpToTls { .. } => ApplicationSocketMode::TcpToTls,
        DomainSocketSecurity::TlsToTcp { .. } => ApplicationSocketMode::TlsToTcp,
        DomainSocketSecurity::TlsToTls { .. } => ApplicationSocketMode::TlsToTls,
    }
}

#[cfg(test)]
mod tests {
    use intercept_proxy_application::ListenerId;

    use super::*;

    #[test]
    fn socket_diagnostic_drop_count_is_exposed_in_listener_status() {
        let listener_id = ListenerId::new();
        let status = status_from_snapshot(
            StatusSnapshot {
                listener_id,
                finished: false,
                listen_address: "127.0.0.1:1234".into(),
                fault_reason: None,
                socket_service: None,
            },
            SocketRelayMetricsSnapshot {
                retained_diagnostic_evictions: 7,
                ..SocketRelayMetricsSnapshot::default()
            },
        );
        assert_eq!(status.listener_id, listener_id);
        assert_eq!(status.retained_diagnostic_evictions, 7);
    }
}
