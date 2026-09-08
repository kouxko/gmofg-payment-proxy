use http::Uri;
use intercept_proxy_exchange::Error;

use crate::{HttpRequestMetadata, Message};

pub(super) fn request_start_line(message: &Message) -> Result<(String, String), Error> {
    let mut fields = message.start_line.split_ascii_whitespace();
    let method = fields.next().ok_or_else(|| {
        Error::coded(
            "HTTP_REQUEST_METHOD_MISSING",
            "HTTP request start-line has no method",
        )
    })?;
    let url = fields.next().ok_or_else(|| {
        Error::coded(
            "HTTP_REQUEST_TARGET_MISSING",
            "HTTP request start-line has no request target",
        )
    })?;
    Ok((method.to_owned(), url.to_owned()))
}

pub(super) fn finalized_upstream_uri(
    outgoing: &Uri,
    request: &HttpRequestMetadata,
    message: &Message,
) -> Result<Uri, Error> {
    let (method, finalized_url) = request_start_line(message)?;
    if method != request.method {
        return Err(Error::coded(
            "HTTP_FINALIZE_METHOD_CHANGED",
            "HTTP Finalize must not change the request method",
        ));
    }
    let original = request
        .url
        .parse::<Uri>()
        .map_err(|error| Error::coded("HTTP_REQUEST_URL_INVALID", error.to_string()))?;
    let finalized = finalized_url
        .parse::<Uri>()
        .map_err(|error| Error::coded("HTTP_FINALIZE_URL_INVALID", error.to_string()))?;
    if finalized.scheme() != original.scheme() || finalized.authority() != original.authority() {
        return Err(Error::coded(
            "HTTP_FINALIZE_URL_ORIGIN_CHANGED",
            "HTTP Finalize may only change the request path and query",
        ));
    }
    let path_and_query = finalized.path_and_query().cloned().ok_or_else(|| {
        Error::coded(
            "HTTP_FINALIZE_URL_INVALID",
            "HTTP Finalize URL has no path and query",
        )
    })?;
    let mut parts = outgoing.clone().into_parts();
    parts.path_and_query = Some(path_and_query);
    Uri::from_parts(parts)
        .map_err(|error| Error::coded("HTTP_FINALIZE_URL_INVALID", error.to_string()))
}
