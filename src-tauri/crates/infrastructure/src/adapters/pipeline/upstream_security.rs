use intercept_proxy_runtime::{UpstreamSecurityEvidence, UpstreamTransportSecurity};

/// Projects transport-owned evidence into a stable, human-readable session detail string.
/// Optional TLS fields are rendered explicitly instead of silently implying mTLS or TLS 1.2.
pub(super) fn describe(evidence: &UpstreamSecurityEvidence) -> String {
    match evidence.transport {
        UpstreamTransportSecurity::PlaintextHttp => format!(
            "明文 HTTP；上游地址：{}；主机名校验：不适用；客户端身份：未配置、未提交",
            evidence.resolved_address
        ),
        UpstreamTransportSecurity::Tls => format!(
            "{}；密码套件：{}；Server subject：{}；Server SHA-256 指纹：{}；主机名校验：{}；客户端身份：{}",
            evidence.tls_version.as_deref().unwrap_or("TLS 版本未知"),
            evidence.cipher_suite.as_deref().unwrap_or("未知"),
            evidence.peer_subject.as_deref().unwrap_or("未知"),
            evidence
                .peer_sha256_fingerprint
                .as_deref()
                .unwrap_or("未知"),
            if evidence.hostname_verification_enabled == Some(true) {
                "启用"
            } else if evidence.hostname_verification_enabled == Some(false) {
                "关闭"
            } else {
                "未知"
            },
            client_identity_text(evidence)
        ),
    }
}

fn client_identity_text(evidence: &UpstreamSecurityEvidence) -> &'static str {
    match (
        evidence.client_identity_configured,
        evidence.client_identity_submitted,
    ) {
        (true, true) => "已配置、已提交",
        (true, false) => "已配置、Server 未请求，未提交",
        (false, _) => "未配置、未提交",
    }
}

#[cfg(test)]
mod tests {
    use std::net::{IpAddr, Ipv4Addr, SocketAddr};

    use super::*;

    fn address() -> SocketAddr {
        SocketAddr::new(IpAddr::V4(Ipv4Addr::LOCALHOST), 16_627)
    }

    #[test]
    fn describes_plaintext_without_tls_claims() {
        let text = describe(&UpstreamSecurityEvidence {
            resolved_address: address(),
            transport: UpstreamTransportSecurity::PlaintextHttp,
            tls_version: None,
            cipher_suite: None,
            peer_subject: None,
            peer_sha256_fingerprint: None,
            hostname_verification_enabled: None,
            client_identity_configured: false,
            client_identity_submitted: false,
        });

        assert!(text.contains("明文 HTTP"));
        assert!(text.contains("主机名校验：不适用"));
        assert!(!text.contains("TLS 1.2"));
    }

    #[test]
    fn distinguishes_configured_identity_from_submitted_identity() {
        let text = describe(&UpstreamSecurityEvidence {
            resolved_address: address(),
            transport: UpstreamTransportSecurity::Tls,
            tls_version: Some("TLS 1.2".into()),
            cipher_suite: Some("TLS_ECDHE_RSA_WITH_AES_128_GCM_SHA256".into()),
            peer_subject: Some("CN=server.test".into()),
            peer_sha256_fingerprint: Some("AA:BB".into()),
            hostname_verification_enabled: Some(true),
            client_identity_configured: true,
            client_identity_submitted: false,
        });

        assert!(text.contains("TLS 1.2"));
        assert!(text.contains("CN=server.test"));
        assert!(text.contains("主机名校验：启用"));
        assert!(text.contains("已配置、Server 未请求，未提交"));
    }
}
