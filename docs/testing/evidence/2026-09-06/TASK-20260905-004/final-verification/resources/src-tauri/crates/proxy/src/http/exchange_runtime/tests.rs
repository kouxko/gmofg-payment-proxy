use intercept_proxy_exchange::{
    Error, ExternalPackageCallFailure, ExternalPackageCallStage, ProtocolDirection,
    ProtocolPackageId, ProtocolPackageRef, ProtocolPackageVersion,
};

use super::exchange_error;
use crate::ErrorCode;

#[test]
fn exchange_error_preserves_adapter_error_code() {
    let error = exchange_error(Error::coded(
        "UPSTREAM_READ_TIMEOUT",
        "server reply timed out",
    ));

    assert_eq!(error.code, ErrorCode::UpstreamReadTimeout.as_str());
    assert!(error.message.contains("server reply timed out"));
}

#[test]
fn exchange_error_maps_core_failure_to_internal() {
    let error = exchange_error(Error::new("Server disconnected before replying"));

    assert_eq!(error.code, ErrorCode::Internal.as_str());
    assert!(
        error
            .message
            .contains("Server disconnected before replying")
    );
}

#[test]
fn exchange_error_preserves_typed_external_package_failure() {
    let failure = ExternalPackageCallFailure {
        package: ProtocolPackageRef {
            id: ProtocolPackageId::new("phase10.http").unwrap(),
            version: ProtocolPackageVersion::new("1.0.0").unwrap(),
        },
        direction: ProtocolDirection::Upstream,
        stage: ExternalPackageCallStage::Display,
        method: "document.upstream.display".into(),
        request_id: Some("proxy-4".into()),
        remote_code: Some(-32412),
        stable_code: Some("DISPLAY_FAILED".into()),
        remote_message: Some("display rejected".into()),
        remote_data_summary: Some("object(fields=1)".into()),
    };
    let error = exchange_error(
        Error::coded("EXTERNAL_PACKAGE_CALL_FAILED", "display rejected")
            .with_external_package_call(failure.clone()),
    );
    assert_eq!(error.code, "EXTERNAL_PACKAGE_CALL_FAILED");
    assert_ne!(error.code, ErrorCode::Internal.as_str());
    assert_eq!(error.external_package_call.as_deref(), Some(&failure));
}
