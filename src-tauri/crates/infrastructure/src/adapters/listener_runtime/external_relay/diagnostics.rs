//! 外部 JSON-RPC 调用的统一脱敏诊断投影。

use intercept_proxy_application::{
    ExternalPackageCallDiagnosticViewModel, ExternalPackageCallStage,
};
use intercept_proxy_domain::{ProtocolDirection, ProtocolPackageRef};
use intercept_proxy_runtime::SocketConnectionIdentity;

use crate::adapters::PackageTransportError;

pub(in crate::adapters::listener_runtime) fn trace_external_rpc_failure(
    package: &ProtocolPackageRef,
    connection: &SocketConnectionIdentity,
    direction: ProtocolDirection,
    stage: ExternalPackageCallStage,
    method: &str,
    error: &PackageTransportError,
) -> ExternalPackageCallDiagnosticViewModel {
    let (request_id, remote_code, stable_code, remote_message, remote_data) = match error {
        PackageTransportError::Remote {
            request_id, error, ..
        } => (
            Some(request_id.as_str()),
            Some(error.code()),
            Some(error.data().code().as_str()),
            Some(error.message()),
            "object(fields=1)".to_owned(),
        ),
        _ => (None, None, None, None, "none".to_owned()),
    };
    let direction_text = match direction {
        ProtocolDirection::Upstream => "upstream",
        ProtocolDirection::Downstream => "downstream",
    };
    let diagnostic = ExternalPackageCallDiagnosticViewModel {
        package: package.clone(),
        direction,
        stage,
        method: method.to_owned(),
        request_id: request_id.map(ToOwned::to_owned),
        remote_code,
        stable_code: stable_code.map(ToOwned::to_owned),
        remote_message: remote_message.map(ToOwned::to_owned),
        remote_data_summary: (remote_data != "none").then_some(remote_data.clone()),
    }
    .sanitized();
    tracing::warn!(
        package_id = %package.id,
        package_version = %package.version,
        runtime_epoch = %connection.runtime_epoch,
        business_connection_id = %connection.connection_id,
        peer_address = %connection.peer_addr,
        direction = direction_text,
        stage = ?stage,
        method,
        request_id = request_id.unwrap_or("none"),
        remote_code = ?remote_code,
        remote_message = remote_message.unwrap_or("none"),
        remote_data_summary = %remote_data,
        error = ?error,
        "external package RPC stage failed"
    );
    diagnostic
}

#[cfg(test)]
pub(super) fn redacted_data_summary(data: Option<&serde_json::Value>) -> String {
    match data {
        None => "none".to_owned(),
        Some(serde_json::Value::Null) => "null".to_owned(),
        Some(serde_json::Value::Bool(_)) => "bool".to_owned(),
        Some(serde_json::Value::Number(_)) => "number".to_owned(),
        Some(serde_json::Value::String(value)) => format!("string(bytes={})", value.len()),
        Some(serde_json::Value::Array(values)) => format!("array(items={})", values.len()),
        Some(serde_json::Value::Object(values)) => format!("object(fields={})", values.len()),
    }
}
