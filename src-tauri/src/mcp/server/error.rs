use bytes::Bytes;
use http_body_util::Full;
use hyper::{Response, StatusCode, header::CONTENT_TYPE};
use serde_json::json;

#[derive(Debug, Clone, Copy)]
pub(super) enum TransportError {
    MethodNotAllowed,
    PathNotFound,
    BodyTooLarge,
    HttpMalformed,
    ProtocolInvalid,
    RequestLimitReached,
    RequestDeadlineExceeded,
    ResponseTooLarge,
}

impl TransportError {
    const fn status(self) -> StatusCode {
        match self {
            Self::MethodNotAllowed => StatusCode::METHOD_NOT_ALLOWED,
            Self::PathNotFound => StatusCode::NOT_FOUND,
            Self::BodyTooLarge => StatusCode::PAYLOAD_TOO_LARGE,
            Self::HttpMalformed | Self::ProtocolInvalid => StatusCode::BAD_REQUEST,
            Self::RequestLimitReached => StatusCode::SERVICE_UNAVAILABLE,
            Self::RequestDeadlineExceeded => StatusCode::GATEWAY_TIMEOUT,
            Self::ResponseTooLarge => StatusCode::INTERNAL_SERVER_ERROR,
        }
    }

    const fn code(self) -> &'static str {
        match self {
            Self::MethodNotAllowed => "HTTP_METHOD_NOT_ALLOWED",
            Self::PathNotFound => "HTTP_PATH_NOT_FOUND",
            Self::BodyTooLarge => "HTTP_BODY_TOO_LARGE",
            Self::HttpMalformed => "HTTP_MALFORMED",
            Self::ProtocolInvalid
            | Self::RequestLimitReached
            | Self::RequestDeadlineExceeded
            | Self::ResponseTooLarge => "MCP_PROTOCOL_INVALID",
        }
    }

    const fn message(self) -> &'static str {
        match self {
            Self::MethodNotAllowed => "MCP HTTP method is not supported",
            Self::PathNotFound => "MCP HTTP endpoint was not found",
            Self::BodyTooLarge => "MCP HTTP request body exceeds the transport limit",
            Self::HttpMalformed => "MCP HTTP request is malformed",
            Self::ProtocolInvalid => "MCP protocol request is invalid",
            Self::RequestLimitReached => "MCP request concurrency limit reached",
            Self::RequestDeadlineExceeded => "MCP transport request deadline exceeded",
            Self::ResponseTooLarge => "MCP response exceeds the transport limit",
        }
    }
}

pub(super) fn response(error: TransportError) -> Response<Full<Bytes>> {
    let body = serde_json::to_vec(&json!({
        "code": error.code(),
        "message": error.message(),
        "details": null,
    }))
    .expect("static MCP transport error is serializable");
    Response::builder()
        .status(error.status())
        .header(CONTENT_TYPE, "application/json")
        .body(Full::new(Bytes::from(body)))
        .expect("static MCP transport error response is valid")
}
