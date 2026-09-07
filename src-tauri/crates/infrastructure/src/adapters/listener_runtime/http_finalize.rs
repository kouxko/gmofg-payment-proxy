use std::{collections::BTreeMap, fmt, sync::Arc};

use http::{HeaderName, HeaderValue, StatusCode, Uri};
use intercept_proxy_domain::{ProtocolDirection, ProtocolPackageRef};
use intercept_proxy_exchange::{Error, ExternalPackageCallFailure, ExternalPackageCallStage};
use intercept_proxy_product_api::BodyCodec;
use intercept_proxy_runtime::{HttpRequestMetadata, Message, RawHeader};
use serde::{Deserialize, Deserializer, Serialize, Serializer, de::MapAccess, ser::SerializeMap};

use crate::adapters::ProtocolPackageRuntime;

use super::package_error::external_rpc_error;

pub(super) struct ExternalHttpFinalizeCall {
    runtime: Arc<dyn ProtocolPackageRuntime>,
    package: ProtocolPackageRef,
    direction: ProtocolDirection,
    codec: Arc<dyn BodyCodec>,
}

impl ExternalHttpFinalizeCall {
    pub(super) fn new(
        runtime: Arc<dyn ProtocolPackageRuntime>,
        package: ProtocolPackageRef,
        direction: ProtocolDirection,
        codec: Arc<dyn BodyCodec>,
    ) -> Self {
        Self {
            runtime,
            package,
            direction,
            codec,
        }
    }

    pub(super) async fn finalize(
        &mut self,
        request: &HttpRequestMetadata,
        message: &mut Message,
    ) -> Result<(), Error> {
        finalize_http_message(
            &self.runtime,
            &self.package,
            self.direction,
            self.codec.as_ref(),
            request,
            message,
        )
        .await
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct HttpFinalizeDocument {
    url: String,
    method: String,
    #[serde(deserialize_with = "deserialize_required_code")]
    code: Option<u16>,
    header: FinalizeHeaders,
    body: String,
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
struct FinalizeHeaders(Vec<(String, String)>);

impl FinalizeHeaders {
    fn from_message(message: &Message) -> Result<Self, Error> {
        let mut headers = BTreeMap::<String, (String, String)>::new();
        for header in &message.headers {
            let name = std::str::from_utf8(&header.name)
                .map_err(|error| Error::coded("HTTP_FINALIZE_HEADER_INVALID", error.to_string()))?
                .to_owned();
            let value = std::str::from_utf8(&header.value)
                .map_err(|error| Error::coded("HTTP_FINALIZE_HEADER_INVALID", error.to_string()))?
                .to_owned();
            headers.insert(name.to_ascii_lowercase(), (name, value));
        }
        Ok(Self(headers.into_values().collect()))
    }

    fn normalized(&self) -> BTreeMap<String, (String, String)> {
        let mut headers = BTreeMap::new();
        for (name, value) in &self.0 {
            headers.insert(
                name.to_ascii_lowercase(),
                (name.to_owned(), value.to_owned()),
            );
        }
        headers
    }
}

fn deserialize_required_code<'de, D>(deserializer: D) -> Result<Option<u16>, D::Error>
where
    D: Deserializer<'de>,
{
    Option::<u16>::deserialize(deserializer)
}

impl Serialize for FinalizeHeaders {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        let mut map = serializer.serialize_map(Some(self.0.len()))?;
        for (name, value) in &self.0 {
            map.serialize_entry(name, value)?;
        }
        map.end()
    }
}

impl<'de> Deserialize<'de> for FinalizeHeaders {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        struct HeaderVisitor;

        impl<'de> serde::de::Visitor<'de> for HeaderVisitor {
            type Value = FinalizeHeaders;

            fn expecting(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
                formatter.write_str("an HTTP header object with string values")
            }

            fn visit_map<A>(self, mut map: A) -> Result<Self::Value, A::Error>
            where
                A: MapAccess<'de>,
            {
                let mut headers = Vec::new();
                while let Some((name, value)) = map.next_entry::<String, String>()? {
                    headers.push((name, value));
                }
                Ok(FinalizeHeaders(headers))
            }
        }

        deserializer.deserialize_map(HeaderVisitor)
    }
}

pub(super) async fn finalize_http_message(
    runtime: &Arc<dyn ProtocolPackageRuntime>,
    package: &ProtocolPackageRef,
    direction: ProtocolDirection,
    codec: &dyn BodyCodec,
    request: &HttpRequestMetadata,
    message: &mut Message,
) -> Result<(), Error> {
    let method = finalize_method(direction);
    let has_finalize = runtime.has_http_finalize(direction).map_err(|error| {
        external_rpc_error(
            package.clone(),
            direction,
            ExternalPackageCallStage::Finalize,
            method,
            &error,
        )
    })?;
    if !has_finalize {
        return Ok(());
    }
    let input = document_from_message(codec, request, message)
        .map_err(|error| local_finalize_error(package, direction, method, error))?;
    let input_json = serde_json::to_string(&input)
        .map_err(|error| Error::coded("HTTP_FINALIZE_INPUT_INVALID", error.to_string()))
        .map_err(|error| local_finalize_error(package, direction, method, error))?;
    let output_json = runtime
        .finalize_http(direction, input_json)
        .await
        .map_err(|error| {
            external_rpc_error(
                package.clone(),
                direction,
                ExternalPackageCallStage::Finalize,
                method,
                &error,
            )
        })?
        .ok_or_else(|| {
            Error::coded(
                "HTTP_FINALIZE_CAPABILITY_CHANGED",
                format!("HTTP package advertised `{method}` but returned no capability"),
            )
        })
        .map_err(|error| local_finalize_error(package, direction, method, error))?;
    let output = serde_json::from_str::<HttpFinalizeDocument>(&output_json)
        .map_err(|error| Error::coded("HTTP_FINALIZE_OUTPUT_INVALID", error.to_string()))
        .map_err(|error| local_finalize_error(package, direction, method, error))?;
    apply_output(direction, codec, request, message, &input, &output)
        .map_err(|error| local_finalize_error(package, direction, method, error))
}

pub(super) fn local_finalize_error(
    package: &ProtocolPackageRef,
    direction: ProtocolDirection,
    method: &'static str,
    error: Error,
) -> Error {
    if error.external_package_call.is_some() {
        return error;
    }
    let stable_code = error.code.clone();
    let remote_message = Some(error.message.clone());
    error.with_external_package_call(ExternalPackageCallFailure {
        package: package.clone(),
        direction,
        stage: ExternalPackageCallStage::Finalize,
        method: method.into(),
        request_id: None,
        remote_code: None,
        stable_code,
        remote_message,
        remote_data_summary: None,
    })
}

fn finalize_method(direction: ProtocolDirection) -> &'static str {
    match direction {
        ProtocolDirection::Upstream => "upstream-finalize",
        ProtocolDirection::Downstream => "downstream-finalize",
    }
}

fn document_from_message(
    codec: &dyn BodyCodec,
    request: &HttpRequestMetadata,
    message: &Message,
) -> Result<HttpFinalizeDocument, Error> {
    let body = codec
        .decode(&message.body)
        .map_err(|error| Error::coded(error.code, error.message))?;
    Ok(HttpFinalizeDocument {
        url: request.url.clone(),
        method: request.method.clone(),
        code: message.http_status(),
        header: FinalizeHeaders::from_message(message)?,
        body,
    })
}

fn apply_output(
    direction: ProtocolDirection,
    codec: &dyn BodyCodec,
    request: &HttpRequestMetadata,
    message: &mut Message,
    input: &HttpFinalizeDocument,
    output: &HttpFinalizeDocument,
) -> Result<(), Error> {
    if output.method != request.method {
        return Err(Error::coded(
            "HTTP_FINALIZE_METHOD_CHANGED",
            "HTTP Finalize must not change the request method",
        ));
    }
    validate_url(direction, &request.url, &output.url)?;
    match direction {
        ProtocolDirection::Upstream if output.code.is_some() => {
            return Err(Error::coded(
                "HTTP_FINALIZE_CODE_INVALID",
                "upstream HTTP Finalize code must remain null",
            ));
        }
        ProtocolDirection::Downstream if output.code.is_none() => {
            return Err(Error::coded(
                "HTTP_FINALIZE_CODE_INVALID",
                "downstream HTTP Finalize code must be an HTTP status",
            ));
        }
        _ => {}
    }

    let mut candidate = message.clone();
    apply_start_line(direction, input, output, &mut candidate)?;
    apply_header_delta(&input.header, &output.header, &mut candidate)?;
    if output.body != input.body {
        let body = codec
            .encode(&output.body)
            .map_err(|error| Error::coded(error.code, error.message))?;
        candidate.replace_body(body.into());
    }
    candidate
        .header_map()
        .map_err(|error| Error::coded(error.code, error.message))?;
    *message = candidate;
    Ok(())
}

fn validate_url(
    direction: ProtocolDirection,
    original: &str,
    finalized: &str,
) -> Result<(), Error> {
    if direction == ProtocolDirection::Downstream {
        return (finalized == original).then_some(()).ok_or_else(|| {
            Error::coded(
                "HTTP_FINALIZE_URL_CHANGED",
                "downstream HTTP Finalize must not change the associated request URL",
            )
        });
    }
    let original = original
        .parse::<Uri>()
        .map_err(|error| Error::coded("HTTP_REQUEST_URL_INVALID", error.to_string()))?;
    let finalized = finalized
        .parse::<Uri>()
        .map_err(|error| Error::coded("HTTP_FINALIZE_URL_INVALID", error.to_string()))?;
    if original.scheme() != finalized.scheme() || original.authority() != finalized.authority() {
        return Err(Error::coded(
            "HTTP_FINALIZE_URL_ORIGIN_CHANGED",
            "HTTP Finalize may only change the request path and query",
        ));
    }
    Ok(())
}

fn apply_start_line(
    direction: ProtocolDirection,
    input: &HttpFinalizeDocument,
    output: &HttpFinalizeDocument,
    message: &mut Message,
) -> Result<(), Error> {
    match direction {
        ProtocolDirection::Upstream if output.url != input.url => {
            let version = message
                .start_line
                .split_ascii_whitespace()
                .nth(2)
                .ok_or_else(|| {
                    Error::coded(
                        "HTTP_REQUEST_VERSION_MISSING",
                        "HTTP request start-line has no version",
                    )
                })?;
            message.start_line = format!("{} {} {version}", output.method, output.url);
        }
        ProtocolDirection::Downstream if output.code != input.code => {
            let status = StatusCode::from_u16(output.code.expect("validated downstream code"))
                .map_err(|error| Error::coded("HTTP_FINALIZE_CODE_INVALID", error.to_string()))?;
            let reason = status.canonical_reason().unwrap_or("");
            message.start_line = format!("HTTP/1.1 {} {reason}", status.as_u16());
        }
        _ => {}
    }
    Ok(())
}

fn apply_header_delta(
    before: &FinalizeHeaders,
    after: &FinalizeHeaders,
    message: &mut Message,
) -> Result<(), Error> {
    let before = before.normalized();
    let after = after.normalized();
    for normalized in before.keys().filter(|name| !after.contains_key(*name)) {
        message.remove_header(normalized);
    }
    for (normalized, (name, value)) in after {
        if before
            .get(&normalized)
            .is_some_and(|(_, current)| current == &value)
        {
            continue;
        }
        HeaderName::from_bytes(name.as_bytes())
            .map_err(|error| Error::coded("HTTP_FINALIZE_HEADER_INVALID", error.to_string()))?;
        HeaderValue::from_bytes(value.as_bytes())
            .map_err(|error| Error::coded("HTTP_FINALIZE_HEADER_INVALID", error.to_string()))?;
        message.remove_header(&normalized);
        message.headers.push(RawHeader::new(name, value));
    }
    Ok(())
}

#[cfg(test)]
mod tests;
