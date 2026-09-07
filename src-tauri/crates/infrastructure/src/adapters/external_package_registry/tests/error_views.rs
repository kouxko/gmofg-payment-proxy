use super::*;
use intercept_proxy_domain::{DomainError, ErrorCode};
use intercept_proxy_package_contract::PackageRpcError;

#[test]
fn connection_errors_map_to_stable_redacted_summaries() {
    let remote = PackageRpcError::new(
        -32_001,
        "remote secret".to_owned(),
        ErrorCode::BodyDecodeFailed,
    );
    let cases = [
        (
            PackageTransportError::RegistrationDeadline,
            "EXTERNAL_PACKAGE_TIMEOUT",
        ),
        (
            PackageTransportError::Disconnected,
            "EXTERNAL_PACKAGE_DISCONNECTED",
        ),
        (
            PackageTransportError::Remote {
                request_id: "req-2".to_owned(),
                method: "hooks.upstream.decode",
                error: remote,
            },
            "BODY_DECODE_FAILED",
        ),
        (
            PackageTransportError::MessageTooLarge {
                actual_bytes: 2,
                limit_bytes: 1,
            },
            "EXTERNAL_PACKAGE_MESSAGE_TOO_LARGE",
        ),
        (
            PackageTransportError::Package {
                error: DomainError::new(ErrorCode::ProtocolPackageInvalid, "secret"),
            },
            "PROTOCOL_PACKAGE_INVALID",
        ),
        (
            PackageTransportError::InvalidResponse,
            "EXTERNAL_PACKAGE_PROTOCOL_FATAL",
        ),
        (
            PackageTransportError::Transport("secret".to_owned()),
            "EXTERNAL_PACKAGE_TRANSPORT_ERROR",
        ),
    ];

    for (error, expected_code) in cases {
        let view = recent_error_view(&error);
        assert_eq!(view.code, expected_code);
        assert!(!view.message.contains("secret"));
    }
}
