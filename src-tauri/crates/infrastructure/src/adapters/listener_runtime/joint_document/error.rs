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
    Error::new(format!("EXTERNAL_PACKAGE_CALL_FAILED\n{error}")).with_external_package_call(
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
