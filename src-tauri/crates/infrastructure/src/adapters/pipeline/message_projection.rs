//! 传输消息与应用视图模型之间的无损投影。
//!
//! 展示用 header 与线上原始 header 刻意分离：UI 可以编辑规范化文本，但未修改字段必须
//! 保留 HTTP/1.1 原有大小写、顺序、重复项和可选空白。无法安全解码的正文按二进制呈现，
//! 不猜测编码，也不为展示方便改写线上字节。

use intercept_proxy_application::{MessageContentViewModel, RawHttpHeaderViewModel};
use intercept_proxy_product_api::{
    BodyCodec, ProductHeader, ProductMessageContext, RequestClassifier,
};
use intercept_proxy_runtime::{ChannelId, Message, ProxyError, Result as ProxyResult};
use serde_json::Value;
use std::collections::BTreeMap;

use crate::adapters::body_codecs::decode_message_body;

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
    let (metadata, body_text, codec_id, mut decode_error) =
        decode_message_body(message, body_codec);
    if codec_id
        .as_deref()
        .is_some_and(|id| id.ends_with("unsupported"))
    {
        decode_error = Some(format!(
            "unsupported charset: {}",
            metadata.charset.as_deref().unwrap_or("unknown")
        ));
    }
    let json = if metadata.content_kind == intercept_proxy_application::MessageContentKind::Json {
        body_text
            .as_deref()
            .filter(|text| !text.trim().is_empty())
            .and_then(|text| match serde_json::from_str(text) {
                Ok(value) => Some(value),
                Err(error) => {
                    decode_error = Some(format!("invalid JSON body: {error}"));
                    None
                }
            })
    } else {
        None
    };
    MessageContentViewModel {
        http_status: message.http_status(),
        start_line_bytes: message.start_line.as_bytes().to_vec(),
        raw_headers,
        headers,
        body_text,
        body_bytes: message.body.to_vec(),
        json,
        content_length: message.body.len(),
        media_type: metadata.media_type,
        charset: metadata.charset,
        content_kind: metadata.content_kind,
        codec_id,
        decode_error,
        query_string: query_string(&message.start_line),
        protocol: None,
        protocol_failure: None,
    }
}

fn query_string(start_line: &str) -> Option<String> {
    let mut fields = start_line.split_ascii_whitespace();
    let first = fields.next()?;
    if first.starts_with("HTTP/") {
        return None;
    }
    let target = fields.next()?;
    let (_, query) = target.split_once('?')?;
    Some(query.split('#').next().unwrap_or_default().to_owned())
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

pub(super) fn decode_body(body_codec: &dyn BodyCodec, bytes: &[u8]) -> ProxyResult<String> {
    body_codec.decode(bytes).map_err(|error| ProxyError {
        code: error.code,
        message: error.message,
        external_package_call: None,
    })
}

pub(super) fn encode_body(body_codec: &dyn BodyCodec, text: &str) -> ProxyResult<Vec<u8>> {
    body_codec.encode(text).map_err(|error| ProxyError {
        code: error.code,
        message: error.message,
        external_package_call: None,
    })
}

pub(super) fn decode_json(body_codec: &dyn BodyCodec, bytes: &[u8]) -> ProxyResult<Value> {
    let text = decode_body(body_codec, bytes)?;
    serde_json::from_str(&text).map_err(|error| ProxyError {
        code: "JSON_INVALID",
        message: format!("decoded body is not valid JSON: {error}"),
        external_package_call: None,
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
