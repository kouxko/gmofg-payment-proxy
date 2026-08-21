//! JSON-RPC 2.0 响应的严格解析。

use intercept_proxy_domain::ExternalPackageRegistration;
use serde::Deserialize;
use serde_json::Value;

use super::super::error::{ExternalPackageFatalProtocolError, ExternalPackageRemoteError};

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RemoteErrorWire {
    code: i64,
    message: String,
    #[serde(default)]
    data: Option<Value>,
}

pub(in crate::adapters::external_packages) enum ParsedResponse {
    Result {
        request_id: String,
        result: Value,
    },
    Error {
        request_id: String,
        error: ExternalPackageRemoteError,
    },
}

pub(in crate::adapters::external_packages) fn parse_response(
    text: &str,
) -> Result<ParsedResponse, ExternalPackageFatalProtocolError> {
    let value: Value =
        serde_json::from_str(text).map_err(|_| ExternalPackageFatalProtocolError::InvalidJson)?;
    let object = value
        .as_object()
        .ok_or(ExternalPackageFatalProtocolError::InvalidResponse)?;
    let jsonrpc = object.get("jsonrpc").and_then(Value::as_str);
    let request_id = object.get("id").and_then(Value::as_str);
    if jsonrpc != Some("2.0") || request_id.is_none() {
        return Err(ExternalPackageFatalProtocolError::InvalidResponse);
    }
    let has_result = object.contains_key("result");
    let has_error = object.contains_key("error");
    if object.len() != 3 || has_result == has_error {
        return Err(ExternalPackageFatalProtocolError::InvalidResponse);
    }
    let request_id = request_id.expect("validated string id").to_owned();
    if has_result {
        Ok(ParsedResponse::Result {
            request_id,
            result: object.get("result").cloned().expect("result key exists"),
        })
    } else {
        let wire: RemoteErrorWire =
            serde_json::from_value(object.get("error").cloned().expect("error key exists"))
                .map_err(|_| ExternalPackageFatalProtocolError::InvalidResponse)?;
        Ok(ParsedResponse::Error {
            request_id,
            error: ExternalPackageRemoteError::new(wire.code, wire.message, wire.data),
        })
    }
}

pub(super) fn parse_registration(
    value: Value,
) -> Result<ExternalPackageRegistration, ExternalPackageFatalProtocolError> {
    serde_json::from_value(value)
        .map_err(|_| ExternalPackageFatalProtocolError::InvalidRegistration)
}
