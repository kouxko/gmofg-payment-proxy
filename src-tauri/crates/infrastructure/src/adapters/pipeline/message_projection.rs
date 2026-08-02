//! 传输消息与应用视图模型之间的无损投影。
//!
//! 展示用 header 与线上原始 header 刻意分离：UI 可以编辑规范化文本，但未修改字段必须
//! 保留 HTTP/1.1 原有大小写、顺序、重复项和可选空白。无法安全解码的正文按二进制呈现，
//! 不猜测编码，也不为展示方便改写线上字节。

use std::collections::{BTreeMap, BTreeSet};

use bytes::Bytes;
use intercept_proxy_application::{MessageContentViewModel, RawHttpHeaderViewModel};
use intercept_proxy_product_api::{
    BodyCodec, ProductHeader, ProductMessageContext, RequestClassifier,
};
use intercept_proxy_runtime::{
    ChannelId, ErrorCode, Message, ProxyError, RawHeader, Result as ProxyResult,
};
use serde_json::Value;

pub(super) fn content_view(
    body_codec: &dyn BodyCodec,
    message: &Message,
) -> MessageContentViewModel {
    let raw_headers = message
        .headers
        .iter()
        .map(|header| RawHttpHeaderViewModel {
            name_bytes: header.name.to_vec(),
            value_bytes: header.value.to_vec(),
            leading_ows_bytes: header.leading_ows().to_vec(),
            trailing_ows_bytes: header.trailing_ows().to_vec(),
        })
        .collect::<Vec<_>>();
    let headers = display_headers(&raw_headers);
    let body_text = decode_body(body_codec, &message.body).ok();
    let json = body_text
        .as_deref()
        .and_then(|text| serde_json::from_str(text).ok());
    MessageContentViewModel {
        http_status: message.http_status(),
        start_line_bytes: message.start_line.as_bytes().to_vec(),
        raw_headers,
        headers,
        body_text,
        body_bytes: message.body.to_vec(),
        json,
        content_length: message.body.len(),
    }
}

pub(super) fn display_headers(
    raw_headers: &[RawHttpHeaderViewModel],
) -> BTreeMap<String, Vec<String>> {
    let mut groups = BTreeMap::<String, (String, Vec<String>)>::new();
    for header in raw_headers {
        let display_name = String::from_utf8_lossy(&header.name_bytes).into_owned();
        let normalized = display_name.to_ascii_lowercase();
        let (_, values) = groups
            .entry(normalized)
            .or_insert_with(|| (display_name, Vec::new()));
        values.push(String::from_utf8_lossy(&header.value_bytes).into_owned());
    }
    groups.into_values().collect()
}

fn group_edited_headers(
    headers: &BTreeMap<String, Vec<String>>,
) -> BTreeMap<String, (String, Vec<String>)> {
    let mut groups = BTreeMap::<String, (String, Vec<String>)>::new();
    for (name, values) in headers {
        groups.insert(name.to_ascii_lowercase(), (name.clone(), values.clone()));
    }
    groups
}

pub(super) fn proxy_message(
    view: &MessageContentViewModel,
    fallback_start_line: &str,
) -> ProxyResult<Message> {
    // The start-line is immutable transport metadata. Breakpoint IPC may
    // display its exact bytes, but it can only change the response status
    // through the separately validated `http_status` field.
    let mut start_line = fallback_start_line.to_owned();
    if let Some(status) = view.http_status {
        let current = Message {
            start_line: start_line.clone(),
            headers: Vec::new(),
            body: Bytes::new(),
            body_modified: false,
        }
        .http_status();
        if current != Some(status) {
            if !(100..=599).contains(&status) {
                return Err(ProxyError::new(
                    ErrorCode::ConfigInvalid,
                    format!("invalid modified HTTP status: {status}"),
                ));
            }
            let version = start_line
                .split_ascii_whitespace()
                .next()
                .filter(|version| version.starts_with("HTTP/"))
                .unwrap_or("HTTP/1.1");
            let reason = start_line.splitn(3, ' ').nth(2).unwrap_or_default();
            let modified = format!("{version} {status} {reason}");
            start_line.clear();
            start_line.push_str(modified.trim_end());
        }
    }
    let headers = merge_edited_headers(&view.raw_headers, &view.headers)?;
    Ok(Message {
        start_line,
        headers,
        body: view.body_bytes.clone().into(),
        body_modified: true,
    })
}

pub(super) fn merge_edited_headers(
    raw_headers: &[RawHttpHeaderViewModel],
    edited: &BTreeMap<String, Vec<String>>,
) -> ProxyResult<Vec<RawHeader>> {
    if raw_headers.is_empty() {
        return Ok(edited
            .iter()
            .flat_map(|(name, values)| {
                values.iter().map(|value| {
                    RawHeader::new(
                        Bytes::copy_from_slice(name.as_bytes()),
                        Bytes::copy_from_slice(value.as_bytes()),
                    )
                })
            })
            .collect());
    }

    let original = group_edited_headers(&display_headers(raw_headers));
    let edited = group_edited_headers(edited);
    let mut emitted_edited_keys = BTreeSet::<String>::new();
    let mut headers = Vec::new();
    for raw in raw_headers {
        let key = String::from_utf8_lossy(&raw.name_bytes).to_ascii_lowercase();
        let original_values = original.get(&key).map(|(_, values)| values);
        let edited_values = edited.get(&key).map(|(_, values)| values);
        if original_values == edited_values {
            headers.push(RawHeader::with_wire_ows(
                Bytes::copy_from_slice(&raw.name_bytes),
                Bytes::copy_from_slice(&raw.value_bytes),
                Bytes::copy_from_slice(&raw.leading_ows_bytes),
                Bytes::copy_from_slice(&raw.trailing_ows_bytes),
            )?);
        } else if emitted_edited_keys.insert(key.clone())
            && let Some((edited_name, values)) = edited.get(&key)
        {
            headers.extend(values.iter().map(|value| {
                RawHeader::new(
                    Bytes::copy_from_slice(edited_name.as_bytes()),
                    Bytes::copy_from_slice(value.as_bytes()),
                )
            }));
        }
    }
    for (normalized, (name, values)) in edited {
        if !original.contains_key(&normalized) {
            headers.extend(values.iter().map(|value| {
                RawHeader::new(
                    Bytes::copy_from_slice(name.as_bytes()),
                    Bytes::copy_from_slice(value.as_bytes()),
                )
            }));
        }
    }
    Ok(headers)
}

pub(super) fn decode_body(body_codec: &dyn BodyCodec, bytes: &[u8]) -> ProxyResult<String> {
    body_codec.decode(bytes).map_err(|error| ProxyError {
        code: error.code,
        message: error.message,
    })
}

pub(super) fn encode_body(body_codec: &dyn BodyCodec, text: &str) -> ProxyResult<Vec<u8>> {
    body_codec.encode(text).map_err(|error| ProxyError {
        code: error.code,
        message: error.message,
    })
}

pub(super) fn decode_json(body_codec: &dyn BodyCodec, bytes: &[u8]) -> ProxyResult<Value> {
    let text = decode_body(body_codec, bytes)?;
    serde_json::from_str(&text).map_err(|error| ProxyError {
        code: "JSON_INVALID",
        message: format!("decoded body is not valid JSON: {error}"),
    })
}

pub(super) fn message_method(start_line: &str) -> Option<&str> {
    start_line.split_ascii_whitespace().next()
}

pub(super) fn message_target(start_line: &str) -> Option<&str> {
    start_line.split_ascii_whitespace().nth(1)
}

pub(super) fn header_value(message: &Message, name: &str) -> Option<String> {
    message
        .headers
        .iter()
        .find(|header| header.name.eq_ignore_ascii_case(name.as_bytes()))
        .map(|header| String::from_utf8_lossy(&header.value).into_owned())
}

pub(super) fn classify_request(
    classifier: &dyn RequestClassifier,
    channel: &ChannelId,
    message: &Message,
) -> intercept_proxy_product_api::ClassifiedRequest {
    let headers = message
        .headers
        .iter()
        .map(|header| ProductHeader {
            name: &header.name,
            value: &header.value,
        })
        .collect::<Vec<_>>();
    classifier.classify(ProductMessageContext {
        channel_id: channel.as_str(),
        start_line: message.start_line.as_bytes(),
        headers: &headers,
        body: &message.body,
    })
}
