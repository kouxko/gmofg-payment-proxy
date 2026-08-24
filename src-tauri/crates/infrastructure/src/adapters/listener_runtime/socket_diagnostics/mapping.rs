use intercept_proxy_application::{
    DiagnosticLogStage, SocketConnectionRouteViewModel, SocketDiagnosticDirection,
    SocketDiagnosticStage, SocketFailureDiagnostic, SocketFailureStage,
    SocketRelayRouteEvidenceViewModel, SocketTlsEvidenceViewModel,
};
use intercept_proxy_runtime::{
    SocketConnectionTarget, SocketRelayDirection, SocketRelayFailure, SocketRelayStage,
};

pub(super) fn route_from_target(target: &SocketConnectionTarget) -> SocketConnectionRouteViewModel {
    match target {
        SocketConnectionTarget::Relay(configured_address) => {
            SocketConnectionRouteViewModel::Relay(Box::new(SocketRelayRouteEvidenceViewModel {
                configured_address: Some(configured_address.clone()),
                resolved_address: None,
                downstream_tls_peer: None,
                upstream_tls: None,
                connection_test: None,
            }))
        }
        SocketConnectionTarget::LocalResponder => SocketConnectionRouteViewModel::LocalResponder {
            // Admission has no completed handshake evidence yet.
            downstream_tls_peer: None,
        },
    }
}

pub(super) fn tls_evidence(
    evidence: &intercept_proxy_runtime::SocketTlsEvidence,
) -> SocketTlsEvidenceViewModel {
    SocketTlsEvidenceViewModel {
        tls_version: evidence.tls_version.clone(),
        cipher_suite: evidence.cipher_suite.clone(),
        peer_subject: evidence.peer_subject.clone(),
        peer_sha256_fingerprint: evidence.peer_sha256_fingerprint.clone(),
        hostname_verification_enabled: evidence.hostname_verification_enabled,
        client_identity_configured: evidence.client_identity_configured,
    }
}

pub(super) fn socket_failure(failure: &SocketRelayFailure) -> Option<SocketFailureDiagnostic> {
    let stage = match failure.stage {
        SocketRelayStage::FrameInspect => SocketFailureStage::Frame,
        SocketRelayStage::Decode => SocketFailureStage::Decode,
        SocketRelayStage::Rule => SocketFailureStage::Rule,
        SocketRelayStage::Encode => SocketFailureStage::Encode,
        SocketRelayStage::RelayWrite => SocketFailureStage::Write,
        SocketRelayStage::Admission
        | SocketRelayStage::DownstreamTls
        | SocketRelayStage::Dns
        | SocketRelayStage::Connect
        | SocketRelayStage::UpstreamTls
        | SocketRelayStage::RelayRead
        | SocketRelayStage::FrameProcess
        | SocketRelayStage::Shutdown => return None,
    };
    Some(SocketFailureDiagnostic {
        stage,
        code: sanitized_failure_code(stage, failure.code),
    })
}

fn sanitized_failure_code(stage: SocketFailureStage, code: &str) -> String {
    let allowed = match stage {
        SocketFailureStage::Frame => matches!(
            code,
            "INVALID_LIMITS"
                | "INVALID_FRAME_BOUNDARY"
                | "FRAME_REJECTED"
                | "BUFFER_LIMIT_EXCEEDED"
                | "TRUNCATED_FRAME"
        ),
        SocketFailureStage::Decode => code == "DECODE_FAILED",
        SocketFailureStage::Rule => code == "RULE_FAILED",
        SocketFailureStage::Encode => matches!(
            code,
            "ENCODE_FAILED" | "EMPTY_OUTPUT" | "OUTPUT_LIMIT_EXCEEDED"
        ),
        SocketFailureStage::Write => matches!(
            code,
            "WRITE_FAILED" | "WRITE_TIMEOUT" | "SOCKET_WRITE_FAILED" | "SOCKET_WRITE_TIMEOUT"
        ),
    };
    if allowed {
        code.to_owned()
    } else {
        "SOCKET_FAILURE".to_owned()
    }
}

pub(super) const fn application_stage(stage: SocketRelayStage) -> SocketDiagnosticStage {
    match stage {
        SocketRelayStage::Admission => SocketDiagnosticStage::Admission,
        SocketRelayStage::DownstreamTls => SocketDiagnosticStage::DownstreamTls,
        SocketRelayStage::Dns => SocketDiagnosticStage::Dns,
        SocketRelayStage::Connect => SocketDiagnosticStage::Connect,
        SocketRelayStage::UpstreamTls => SocketDiagnosticStage::UpstreamTls,
        SocketRelayStage::RelayRead => SocketDiagnosticStage::RelayRead,
        SocketRelayStage::FrameInspect => SocketDiagnosticStage::FrameInspect,
        SocketRelayStage::Decode => SocketDiagnosticStage::Decode,
        SocketRelayStage::Rule => SocketDiagnosticStage::Rule,
        SocketRelayStage::Encode => SocketDiagnosticStage::Encode,
        SocketRelayStage::FrameProcess => SocketDiagnosticStage::FrameProcess,
        SocketRelayStage::RelayWrite => SocketDiagnosticStage::RelayWrite,
        SocketRelayStage::Shutdown => SocketDiagnosticStage::Shutdown,
    }
}

pub(super) const fn application_direction(
    direction: SocketRelayDirection,
) -> SocketDiagnosticDirection {
    match direction {
        SocketRelayDirection::Downstream => SocketDiagnosticDirection::Downstream,
        SocketRelayDirection::Upstream => SocketDiagnosticDirection::Upstream,
        SocketRelayDirection::ClientToServer => SocketDiagnosticDirection::ClientToServer,
        SocketRelayDirection::ServerToClient => SocketDiagnosticDirection::ServerToClient,
        SocketRelayDirection::LocalExchange => SocketDiagnosticDirection::LocalExchange,
    }
}

pub(super) fn diagnostic_stage(failure: SocketRelayFailure) -> DiagnosticLogStage {
    match failure.stage {
        SocketRelayStage::DownstreamTls => DiagnosticLogStage::DownstreamTls,
        SocketRelayStage::UpstreamTls => DiagnosticLogStage::UpstreamTls,
        SocketRelayStage::Admission
        | SocketRelayStage::Dns
        | SocketRelayStage::Connect
        | SocketRelayStage::RelayRead
        | SocketRelayStage::FrameInspect
        | SocketRelayStage::Decode
        | SocketRelayStage::Rule
        | SocketRelayStage::Encode
        | SocketRelayStage::FrameProcess
        | SocketRelayStage::RelayWrite
        | SocketRelayStage::Shutdown => DiagnosticLogStage::Socket,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dynamic_or_malformed_codes_are_not_exposed() {
        let failure = SocketRelayFailure {
            stage: SocketRelayStage::Decode,
            direction: None,
            code: "PAN_4111111111111111",
        };
        assert_eq!(socket_failure(&failure).unwrap().code, "SOCKET_FAILURE");
    }
}
