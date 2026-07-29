//! Byte-preserving HTTP message representation and reconstruction.

use bytes::Bytes;
use http::header::{CONNECTION, CONTENT_LENGTH, HeaderName, HeaderValue};
use http::{HeaderMap, Method, StatusCode, Uri};
use serde_json::Value;

use crate::codec;
use crate::{ErrorCode, ProxyError, Result};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RawHeader {
    pub name: Bytes,
    pub value: Bytes,
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
            total = total.saturating_add(header.name.len() + header.value.len());
            if header.name.len() > limits.max_header_name_bytes
                || header.value.len() > limits.max_header_value_bytes
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

    pub fn decoded_shift_jis(&self) -> Result<String> {
        codec::decode_strict(&self.body)
    }

    pub fn parse_shift_jis_json(&self) -> Result<Value> {
        let text = self.decoded_shift_jis()?;
        serde_json::from_str(&text)
            .map_err(|error| ProxyError::new(ErrorCode::JsonInvalid, error.to_string()))
    }

    pub fn replace_shift_jis_text(&mut self, text: &str) -> Result<()> {
        self.body = Bytes::from(codec::encode_strict(text)?);
        self.body_modified = true;
        self.set_content_length(self.body.len());
        Ok(())
    }

    pub fn replace_json(&mut self, value: &Value) -> Result<()> {
        let text = serde_json::to_string(value)
            .map_err(|error| ProxyError::new(ErrorCode::JsonInvalid, error.to_string()))?;
        self.replace_shift_jis_text(&text)
    }

    pub fn set_content_length(&mut self, length: usize) {
        self.remove_header("content-length");
        self.headers.push(RawHeader {
            name: Bytes::from_static(b"content-length"),
            value: Bytes::from(length.to_string()),
        });
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
            self.headers.push(RawHeader {
                name: Bytes::from_static(b"host"),
                value: Bytes::copy_from_slice(upstream_host.as_bytes()),
            });
        }
        self.remove_header("connection");
        self.headers.push(RawHeader {
            name: Bytes::from_static(b"connection"),
            value: Bytes::from_static(b"close"),
        });
        if self.body_modified {
            self.set_content_length(self.body.len());
        }
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
        let capacity = self.start_line.len()
            + self
                .headers
                .iter()
                .map(|h| h.name.len() + h.value.len() + 4)
                .sum::<usize>()
            + 4
            + self.body.len();
        let mut bytes = Vec::with_capacity(capacity);
        bytes.extend_from_slice(self.start_line.as_bytes());
        bytes.extend_from_slice(b"\r\n");
        for header in &self.headers {
            bytes.extend_from_slice(&header.name);
            bytes.extend_from_slice(b": ");
            bytes.extend_from_slice(&header.value);
            bytes.extend_from_slice(b"\r\n");
        }
        bytes.extend_from_slice(b"\r\n");
        bytes.extend_from_slice(&self.body);
        Bytes::from(bytes)
    }
}

pub(crate) fn force_connection_close(headers: &mut HeaderMap) {
    headers.remove(CONNECTION);
    headers.insert(CONNECTION, HeaderValue::from_static("close"));
}

pub(crate) fn content_length(headers: &mut HeaderMap, length: usize) -> Result<()> {
    let value = HeaderValue::from_str(&length.to_string())
        .map_err(|error| ProxyError::new(ErrorCode::Internal, error.to_string()))?;
    headers.remove(CONTENT_LENGTH);
    headers.insert(CONTENT_LENGTH, value);
    Ok(())
}

pub(crate) fn raw_headers(headers: &HeaderMap) -> Vec<RawHeader> {
    headers
        .iter()
        .map(|(name, value)| RawHeader {
            name: Bytes::copy_from_slice(name.as_str().as_bytes()),
            value: Bytes::copy_from_slice(value.as_bytes()),
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn preserves_unmodified_raw_body_and_recalculates_modified_length() {
        let body = Bytes::from_static(&[0x81, 0x00, 0xff]);
        let mut message = Message {
            start_line: "POST / HTTP/1.1".into(),
            headers: vec![RawHeader {
                name: Bytes::from_static(b"Content-Length"),
                value: Bytes::from_static(b"999"),
            }],
            body: body.clone(),
            body_modified: false,
        };
        assert_eq!(message.passthrough_body(), body);
        message.replace_shift_jis_text("OK").unwrap();
        assert_eq!(message.declared_content_length(), Some(2));
    }

    #[test]
    fn reconstruction_uses_crlf_and_exact_body() {
        let message = Message {
            start_line: "HTTP/1.1 200 OK".into(),
            headers: vec![RawHeader {
                name: Bytes::from_static(b"x-test"),
                value: Bytes::from_static(b"yes"),
            }],
            body: Bytes::from_static(b"\0raw"),
            body_modified: false,
        };
        assert_eq!(
            &message.reconstruct()[..],
            b"HTTP/1.1 200 OK\r\nx-test: yes\r\n\r\n\0raw"
        );
    }

    #[test]
    fn shift_jis_json_is_structured_only_when_valid() {
        let mut message = Message::response(
            StatusCode::OK,
            &HeaderMap::new(),
            Bytes::from(codec::encode_strict(r#"{"result":"成功"}"#).unwrap()),
        );
        assert_eq!(message.parse_shift_jis_json().unwrap()["result"], "成功");
        message.body = Bytes::from_static(b"{broken");
        assert_eq!(
            message.parse_shift_jis_json().unwrap_err().code,
            "JSON_INVALID"
        );
    }
}
