use intercept_proxy_domain::{ProtocolDirection, ProtocolPackageRef};
use intercept_proxy_exchange::{Error, ExternalPackageCallFailure, ExternalPackageCallStage};

pub(super) fn external_rpc_error(
    package: ProtocolPackageRef,
    direction: ProtocolDirection,
    stage: ExternalPackageCallStage,
    default_method: &'static str,
    error: &crate::adapters::PackageTransportError,
) -> Error {
    let (method, request_id, remote_code, stable_code, remote_message, remote_data_summary) =
        match error {
            crate::adapters::PackageTransportError::Remote {
                request_id,
                method,
                error,
            } => (
                (*method).to_owned(),
                Some(request_id.clone()),
                Some(error.code()),
                Some(error.data().code().as_str().to_owned()),
                Some(error.message().to_owned()),
                Some("object(fields=1)".to_owned()),
            ),
            crate::adapters::PackageTransportError::Package { error } => (
                default_method.to_owned(),
                None,
                None,
                Some(error.code.as_str().to_owned()),
                Some(error.message.clone()),
                None,
            ),
            _ => (default_method.to_owned(), None, None, None, None, None),
        };
    Error::coded("EXTERNAL_PACKAGE_CALL_FAILED", error.to_string()).with_external_package_call(
        ExternalPackageCallFailure {
            package,
            direction,
            stage,
            method,
            request_id,
            remote_code,
            stable_code,
            remote_message,
            remote_data_summary,
        },
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::adapters::PackageTransportError;
    use intercept_proxy_domain::{
        DomainError, ErrorCode, ProtocolPackageId, ProtocolPackageVersion,
    };
    use intercept_proxy_package_contract::PackageRpcError;

    #[test]
    fn shared_package_error_preserves_remote_and_component_details() {
        let package = ProtocolPackageRef {
            id: ProtocolPackageId::new("shared.error").unwrap(),
            version: ProtocolPackageVersion::new("1.0.0").unwrap(),
        };
        let cases = [
            (
                PackageTransportError::Remote {
                    request_id: "request-7".into(),
                    method: "hooks.downstream.encode",
                    error: PackageRpcError::new(
                        -32_001,
                        "remote encode failed",
                        ErrorCode::BodyEncodeFailed,
                    ),
                },
                Some("request-7"),
                Some(-32_001),
                "remote encode failed",
            ),
            (
                PackageTransportError::Package {
                    error: DomainError::new(ErrorCode::BodyEncodeFailed, "component encode failed"),
                },
                None,
                None,
                "component encode failed",
            ),
        ];
        for (input, request_id, remote_code, message) in cases {
            let mapped = external_rpc_error(
                package.clone(),
                ProtocolDirection::Downstream,
                ExternalPackageCallStage::Encode,
                "hooks.downstream.encode",
                &input,
            );
            assert_eq!(mapped.code.as_deref(), Some("EXTERNAL_PACKAGE_CALL_FAILED"));
            let detail = mapped.external_package_call.unwrap();
            assert_eq!(detail.package, package);
            assert_eq!(detail.direction, ProtocolDirection::Downstream);
            assert_eq!(detail.stage, ExternalPackageCallStage::Encode);
            assert_eq!(detail.method, "hooks.downstream.encode");
            assert_eq!(detail.request_id.as_deref(), request_id);
            assert_eq!(detail.remote_code, remote_code);
            assert_eq!(detail.stable_code.as_deref(), Some("BODY_ENCODE_FAILED"));
            assert_eq!(detail.remote_message.as_deref(), Some(message));
            assert_eq!(
                detail.remote_data_summary.as_deref(),
                request_id.map(|_| "object(fields=1)")
            );
        }
    }
}
