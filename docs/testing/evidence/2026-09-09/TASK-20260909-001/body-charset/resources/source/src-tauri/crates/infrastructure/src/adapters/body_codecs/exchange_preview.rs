//! HTTP Exchange 的只读正文预览；不参与业务 Decode 或网络报文重建。

use bytes::Bytes;
use intercept_proxy_domain::BodyCodecKind;
use intercept_proxy_product_api::ProductError;
use intercept_proxy_runtime::Message;

use super::resolve_message_codec;

/// 为已有非 UTF-8 有损观测生成字符集明确的文本。
///
/// `None` 表示当前合同不提供文本投影（Raw、未知字符集或压缩正文），调用方保留原观测。
/// 支持的编码遇到非法字节时返回错误，不能把替换字符当作成功解码。
pub fn decode_http_observation_body(
    header: &str,
    wire_body: &[u8],
) -> Result<Option<String>, ProductError> {
    let message =
        Message::from_raw_http1_head(header.as_bytes(), Bytes::copy_from_slice(wire_body))
            .map_err(|error| ProductError::new(error.code, error.message))?;
    if message.headers.iter().any(|header| {
        header.name.eq_ignore_ascii_case(b"content-encoding")
            && !header
                .value
                .split(|byte| *byte == b',')
                .all(|value| value.trim_ascii().eq_ignore_ascii_case(b"identity"))
    }) {
        return Ok(None);
    }
    let codec = resolve_message_codec(BodyCodecKind::Auto, &message);
    match codec.id() {
        "auto:utf-8" | "auto:shift-jis" => codec.decode(wire_body).map(Some),
        _ => Ok(None),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn exchange_preview_reuses_shift_jis_aliases_without_changing_wire_bytes() {
        let text = "売上票（署名）保管正常完了。";
        let (bytes, _, had_errors) = encoding_rs::SHIFT_JIS.encode(text);
        assert!(!had_errors);
        let original = bytes.to_vec();
        for charset in [
            "Shift_JIS",
            "shift-jis",
            "sjis",
            "windows-31j",
            "MS932",
            "cp932",
        ] {
            let header =
                format!("HTTP/1.1 200 OK\r\ncOnTeNt-TyPe: text/csv; CHARSET=\"{charset}\"\r\n\r\n");
            assert_eq!(
                decode_http_observation_body(&header, &bytes)
                    .unwrap()
                    .as_deref(),
                Some(text)
            );
            assert_eq!(bytes.as_ref(), original);
        }
    }

    #[test]
    fn exchange_preview_does_not_guess_missing_unsupported_or_compressed_encoding() {
        for headers in [
            "",
            "Content-Type: text/plain\r\n",
            "Content-Type: application/octet-stream\r\n",
            "Content-Type: text/plain; charset=unknown\r\n",
            "Content-Type: text/plain; charset=Shift_JIS\r\nContent-Encoding: gzip\r\n",
        ] {
            let header = format!("HTTP/1.1 200 OK\r\n{headers}\r\n");
            assert_eq!(
                decode_http_observation_body(&header, &[0x82]).unwrap(),
                None
            );
        }
    }

    #[test]
    fn exchange_preview_propagates_invalid_text_and_header_errors() {
        for (charset, code) in [
            ("Shift_JIS", "SHIFT_JIS_DECODE_FAILED"),
            ("UTF-8", "UTF8_DECODE_FAILED"),
        ] {
            let header =
                format!("HTTP/1.1 200 OK\r\nContent-Type: text/plain;charset={charset}\r\n\r\n");
            assert_eq!(
                decode_http_observation_body(&header, &[0x82])
                    .unwrap_err()
                    .code,
                code
            );
        }
        assert!(decode_http_observation_body("not a header", &[0x82]).is_err());
    }
}
