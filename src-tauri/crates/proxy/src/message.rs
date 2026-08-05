//! 保留原始字节的 HTTP 消息模型与重建逻辑。
//!
//! `HeaderMap` 适合语义处理，却不能完整表达大小写、重复顺序和线上空白；因此本模块同时
//! 保存 raw header。只有字段被规则实际修改时才重建对应字节，解析失败或超过限制会显式
//! 报错，不退回可能篡改报文的“最佳猜测”。

use bytes::Bytes;
use http::header::{HeaderName, HeaderValue};
use http::{HeaderMap, Method, StatusCode, Uri};

use crate::{ErrorCode, ProxyError, Result};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RawHeader {
    pub name: Bytes,
    pub value: Bytes,
    leading_ows: Bytes,
    trailing_ows: Bytes,
}

impl RawHeader {
    #[must_use]
    pub fn new(name: impl Into<Bytes>, value: impl Into<Bytes>) -> Self {
        Self {
            name: name.into(),
            value: value.into(),
            leading_ows: Bytes::from_static(b" "),
            trailing_ows: Bytes::new(),
        }
    }

    pub fn with_wire_ows(
        name: impl Into<Bytes>,
        value: impl Into<Bytes>,
        leading_ows: impl Into<Bytes>,
        trailing_ows: impl Into<Bytes>,
    ) -> Result<Self> {
        let leading_ows = leading_ows.into();
        let trailing_ows = trailing_ows.into();
        if leading_ows
            .iter()
            .chain(trailing_ows.iter())
            .any(|byte| !matches!(byte, b' ' | b'\t'))
        {
            return Err(ProxyError::new(
                ErrorCode::ConfigInvalid,
                "HTTP header wire whitespace contains a non-OWS byte",
            ));
        }
        Ok(Self {
            name: name.into(),
            value: value.into(),
            leading_ows,
            trailing_ows,
        })
    }

    #[must_use]
    pub fn leading_ows(&self) -> &[u8] {
        &self.leading_ows
    }

    #[must_use]
    pub fn trailing_ows(&self) -> &[u8] {
        &self.trailing_ows
    }

    fn from_wire(name: &[u8], raw_value: &[u8]) -> Self {
        let leading_end = raw_value
            .iter()
            .position(|byte| !matches!(byte, b' ' | b'\t'))
            .unwrap_or(raw_value.len());
        let semantic_end = raw_value[leading_end..]
            .iter()
            .rposition(|byte| !matches!(byte, b' ' | b'\t'))
            .map_or(leading_end, |index| leading_end + index + 1);
        Self {
            name: Bytes::copy_from_slice(name),
            value: Bytes::copy_from_slice(&raw_value[leading_end..semantic_end]),
            leading_ows: Bytes::copy_from_slice(&raw_value[..leading_end]),
            trailing_ows: Bytes::copy_from_slice(&raw_value[semantic_end..]),
        }
    }
}

#[derive(Debug, Clone, Copy)]
pub struct MessageLimits {
    pub max_body_bytes: usize,
    pub max_headers: usize,
    pub max_header_name_bytes: usize,
    pub max_header_value_bytes: usize,
    pub max_total_header_bytes: usize,
}

impl Default for MessageLimits {
    fn default() -> Self {
        Self {
            max_body_bytes: 4 * 1024 * 1024,
            max_headers: 100,
            max_header_name_bytes: 256,
            max_header_value_bytes: 8 * 1024,
            max_total_header_bytes: 64 * 1024,
        }
    }
}

#[derive(Debug, Clone)]
pub struct Message {
    pub start_line: String,
    pub headers: Vec<RawHeader>,
    pub body: Bytes,
    /// True only when a rule or operator changed the effective body.
    pub body_modified: bool,
}

impl Message {
    /// Builds a message from a captured HTTP/1 head without passing header
    /// names or values through `HeaderMap`.
    ///
    /// Hyper's semantic model normalizes header names and groups duplicates.
    /// This parser is intentionally small because Hyper still owns framing and
    /// protocol validation; it only recovers the already-accepted start line
    /// and ordered field bytes for capture and breakpoint round trips.
    pub fn from_raw_http1_head(head: &[u8], body: Bytes) -> Result<Self> {
        let head = head.strip_suffix(b"\r\n\r\n").ok_or_else(|| {
            ProxyError::new(
                ErrorCode::HeaderLimitExceeded,
                "captured HTTP/1 head is incomplete",
            )
        })?;
        let mut lines = head.split(|byte| *byte == b'\n');
        let start_line = lines.next().ok_or_else(|| {
            ProxyError::new(ErrorCode::HeaderLimitExceeded, "HTTP start-line is missing")
        })?;
        let start_line = start_line.strip_suffix(b"\r").unwrap_or(start_line);
        let start_line = std::str::from_utf8(start_line).map_err(|_| {
            ProxyError::new(
                ErrorCode::HeaderLimitExceeded,
                "HTTP start-line is not valid ASCII/UTF-8",
            )
        })?;
        let mut headers = Vec::new();
        for line in lines {
            let line = line.strip_suffix(b"\r").unwrap_or(line);
            if line.is_empty() {
                continue;
            }
            let colon = line.iter().position(|byte| *byte == b':').ok_or_else(|| {
                ProxyError::new(
                    ErrorCode::HeaderLimitExceeded,
                    "captured HTTP header is missing ':'",
                )
            })?;
            let name = &line[..colon];
            headers.push(RawHeader::from_wire(name, &line[colon + 1..]));
        }
        Ok(Self {
            start_line: start_line.to_owned(),
            headers,
            body,
            body_modified: false,
        })
    }

    pub fn request(method: &Method, uri: &Uri, headers: &HeaderMap, body: Bytes) -> Self {
        Self {
            start_line: format!("{method} {uri} HTTP/1.1"),
            headers: raw_headers(headers),
            body,
            body_modified: false,
        }
    }

    pub fn response(status: StatusCode, headers: &HeaderMap, body: Bytes) -> Self {
        let reason = status.canonical_reason().unwrap_or("");
        Self {
            start_line: format!("HTTP/1.1 {} {reason}", status.as_u16()),
            headers: raw_headers(headers),
            body,
            body_modified: false,
        }
    }

    #[must_use]
    pub fn http_status(&self) -> Option<u16> {
        let mut parts = self.start_line.split_ascii_whitespace();
        let version = parts.next()?;
        if !version.starts_with("HTTP/") {
            return None;
        }
        let status = parts.next()?.parse::<u16>().ok()?;
        StatusCode::from_u16(status)
            .ok()
            .map(|value| value.as_u16())
    }

    pub fn validate(&self, limits: MessageLimits) -> Result<()> {
        if self.body.len() > limits.max_body_bytes {
            return Err(ProxyError::new(
                ErrorCode::BodyTooLarge,
                format!(
                    "body is {} bytes; limit is {}",
                    self.body.len(),
                    limits.max_body_bytes
                ),
            ));
        }
        if self.headers.len() > limits.max_headers {
            return Err(ProxyError::new(
                ErrorCode::HeaderLimitExceeded,
                "too many headers",
            ));
        }
        let mut total = 0usize;
        for header in &self.headers {
            let wire_value_len = header
                .leading_ows
                .len()
                .saturating_add(header.value.len())
                .saturating_add(header.trailing_ows.len());
            total = total.saturating_add(header.name.len() + wire_value_len);
            if header.name.len() > limits.max_header_name_bytes
                || wire_value_len > limits.max_header_value_bytes
                || total > limits.max_total_header_bytes
            {
                return Err(ProxyError::new(
                    ErrorCode::HeaderLimitExceeded,
                    "header size limit exceeded",
                ));
            }
        }
        Ok(())
    }

    /// Returns the original body bytes without any codec round trip.
    pub fn passthrough_body(&self) -> Bytes {
        self.body.clone()
    }

    /// Replaces the body with already-encoded bytes.
    ///
    /// Character decoding, structured-data parsing, and product-specific
    /// serialization belong to the application/product layer. The runtime
    /// only preserves or forwards the resulting wire bytes.
    pub fn replace_body(&mut self, body: Bytes) {
        self.body = body;
        self.body_modified = true;
        self.set_content_length(self.body.len());
    }

    pub fn set_content_length(&mut self, length: usize) {
        self.remove_header("content-length");
        self.headers.push(RawHeader::new(
            Bytes::from_static(b"Content-Length"),
            Bytes::from(length.to_string()),
        ));
    }

    pub fn declared_content_length(&self) -> Option<usize> {
        self.headers
            .iter()
            .find(|h| h.name.eq_ignore_ascii_case(b"content-length"))
            .and_then(|h| std::str::from_utf8(&h.value).ok())
            .and_then(|value| value.parse().ok())
    }

    pub fn remove_header(&mut self, name: &str) {
        self.headers
            .retain(|header| !header.name.eq_ignore_ascii_case(name.as_bytes()));
    }

    /// Removes hop-by-hop fields and rewrites Host (`MESSAGE-005`).
    pub fn normalize_for_forward(&mut self, upstream_host: &str, rewrite_host: bool) {
        for name in [
            "connection",
            "keep-alive",
            "proxy-authenticate",
            "proxy-authorization",
            "te",
            "trailer",
            "transfer-encoding",
            "upgrade",
        ] {
            self.remove_header(name);
        }
        if rewrite_host {
            self.remove_header("host");
            self.headers.push(RawHeader::new(
                Bytes::from_static(b"Host"),
                Bytes::copy_from_slice(upstream_host.as_bytes()),
            ));
        }
        self.remove_header("connection");
        self.headers.push(RawHeader::new(
            Bytes::from_static(b"Connection"),
            Bytes::from_static(b"close"),
        ));
        // Hyper decoded any transfer framing before this model was created.
        // Forwarding therefore always needs one exact length, even when a
        // rule did not modify the decoded body.
        self.set_content_length(self.body.len());
    }

    pub fn header_map(&self) -> Result<HeaderMap> {
        let mut result = HeaderMap::new();
        for raw in &self.headers {
            let name = HeaderName::from_bytes(&raw.name).map_err(|error| {
                ProxyError::new(ErrorCode::HeaderLimitExceeded, error.to_string())
            })?;
            let value = HeaderValue::from_bytes(&raw.value).map_err(|error| {
                ProxyError::new(ErrorCode::HeaderLimitExceeded, error.to_string())
            })?;
            result.append(name, value);
        }
        Ok(result)
    }

    pub fn reconstruct(&self) -> Bytes {
        self.reconstruct_with_header_style(false)
    }

    /// Reconstructs an upstream request using the same conventional header
    /// casing as the normal Hyper HTTP/1.1 client path.
    pub fn reconstruct_title_case_headers(&self) -> Bytes {
        self.reconstruct_with_header_style(true)
    }

    fn reconstruct_with_header_style(&self, title_case_headers: bool) -> Bytes {
        let capacity = self.start_line.len()
            + self
                .headers
                .iter()
                .map(|h| {
                    h.name.len() + h.leading_ows.len() + h.value.len() + h.trailing_ows.len() + 3
                })
                .sum::<usize>()
            + 4
            + self.body.len();
        let mut bytes = Vec::with_capacity(capacity);
        bytes.extend_from_slice(self.start_line.as_bytes());
        bytes.extend_from_slice(b"\r\n");
        for header in &self.headers {
            if title_case_headers {
                let mut capitalize = true;
                for byte in header.name.iter().copied() {
                    bytes.push(if capitalize {
                        byte.to_ascii_uppercase()
                    } else {
                        byte.to_ascii_lowercase()
                    });
                    capitalize = byte == b'-';
                }
            } else {
                bytes.extend_from_slice(&header.name);
            }
            bytes.push(b':');
            bytes.extend_from_slice(&header.leading_ows);
            bytes.extend_from_slice(&header.value);
            bytes.extend_from_slice(&header.trailing_ows);
            bytes.extend_from_slice(b"\r\n");
        }
        bytes.extend_from_slice(b"\r\n");
        bytes.extend_from_slice(&self.body);
        Bytes::from(bytes)
    }
}

pub(crate) fn raw_headers(headers: &HeaderMap) -> Vec<RawHeader> {
    headers
        .iter()
        .map(|(name, value)| {
            RawHeader::new(
                Bytes::copy_from_slice(name.as_str().as_bytes()),
                Bytes::copy_from_slice(value.as_bytes()),
            )
        })
        .collect()
}

#[cfg(test)]
#[path = "message/tests.rs"]
mod tests;
