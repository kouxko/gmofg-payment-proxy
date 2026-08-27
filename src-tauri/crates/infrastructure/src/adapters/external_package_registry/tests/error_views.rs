use super::*;
use crate::adapters::external_packages::{
    ExternalPackageFatalProtocolError, ExternalPackageRemoteError,
};

#[test]
fn connection_errors_map_to_stable_redacted_summaries() {
    let remote = ExternalPackageRemoteError::new(
        -32_001,
        "remote secret".to_owned(),
        Some(serde_json::json!({"api_key": "secret"})),
    );
    let cases = [
        (
            ExternalPackageConnectionError::Busy,
            "EXTERNAL_PACKAGE_BUSY",
        ),
        (
            ExternalPackageConnectionError::Timeout {
                request_id: "req-1".to_owned(),
                method: "hooks.upstream.frame".to_owned(),
            },
            "EXTERNAL_PACKAGE_TIMEOUT",
        ),
        (
            ExternalPackageConnectionError::Disconnected,
            "EXTERNAL_PACKAGE_DISCONNECTED",
        ),
        (
            ExternalPackageConnectionError::Remote {
                request_id: "req-2".to_owned(),
                method: "hooks.upstream.decode".to_owned(),
                error: remote,
            },
            "EXTERNAL_PACKAGE_REMOTE_ERROR",
        ),
        (
            ExternalPackageConnectionError::MessageTooLarge {
                actual_bytes: 2,
                limit_bytes: 1,
            },
            "EXTERNAL_PACKAGE_MESSAGE_TOO_LARGE",
        ),
        (
            ExternalPackageConnectionError::InvalidPayload("secret".to_owned()),
            "EXTERNAL_PACKAGE_INVALID_PAYLOAD",
        ),
        (
            ExternalPackageConnectionError::Fatal(
                ExternalPackageFatalProtocolError::InvalidResponse,
            ),
            "EXTERNAL_PACKAGE_PROTOCOL_FATAL",
        ),
        (
            ExternalPackageConnectionError::Transport("secret".to_owned()),
            "EXTERNAL_PACKAGE_TRANSPORT_ERROR",
        ),
    ];

    for (error, expected_code) in cases {
        let view = recent_error_view(&error);
        assert_eq!(view.code, expected_code);
        assert!(!view.message.contains("secret"));
    }
}
