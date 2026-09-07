#![cfg_attr(not(any(test, target_arch = "wasm32")), allow(dead_code))]

use serde::{Deserialize, Serialize};

wit_bindgen::generate!({
    path: "../../../src-tauri/crates/package-runtime/wit",
    world: "socket-package",
});

const _: &str =
    include_str!("../../../../src-tauri/crates/package-runtime/wit/protocol-package.wit");

const HEADER_BYTES: usize = 2;

#[derive(Deserialize, Serialize)]
struct Iso8583Document {
    message_type: String,
}

struct Iso8583AsciiStandard;

impl Iso8583AsciiStandard {
    fn frame(buffer: &[u8]) -> Result<FrameResult, String> {
        if buffer.len() < HEADER_BYTES {
            return Ok(FrameResult::NeedMore(Some(HEADER_BYTES as u64)));
        }
        let payload_bytes = usize::from(u16::from_be_bytes([buffer[0], buffer[1]]));
        let consumed_bytes = HEADER_BYTES
            .checked_add(payload_bytes)
            .ok_or_else(|| "ISO 8583 frame length overflow".to_owned())?;
        if buffer.len() < consumed_bytes {
            return Ok(FrameResult::NeedMore(Some(consumed_bytes as u64)));
        }
        Ok(FrameResult::Complete(consumed_bytes as u64))
    }

    fn decode(input: &[u8]) -> Result<String, String> {
        if input.len() < HEADER_BYTES + 4 {
            return Err("ISO 8583 frame is shorter than its MTI".to_owned());
        }
        let declared = usize::from(u16::from_be_bytes([input[0], input[1]]));
        if declared != input.len() - HEADER_BYTES {
            return Err("ISO 8583 frame length header does not match the input".to_owned());
        }
        let message_type = std::str::from_utf8(&input[HEADER_BYTES..HEADER_BYTES + 4])
            .map_err(|_| "ISO 8583 message_type must be ASCII".to_owned())?;
        if !message_type.is_ascii() {
            return Err("ISO 8583 message_type must be ASCII".to_owned());
        }
        serde_json::to_string(&Iso8583Document {
            message_type: message_type.to_owned(),
        })
        .map_err(|error| error.to_string())
    }

    fn encode(mut original_input: Vec<u8>, document_json: &str) -> Result<Vec<u8>, String> {
        let document = serde_json::from_str::<Iso8583Document>(document_json)
            .map_err(|error| error.to_string())?;
        let message_type = document.message_type.as_bytes();
        if message_type.len() != 4 || !message_type.is_ascii() {
            return Err(
                "ISO 8583 message_type must contain exactly four ASCII characters".to_owned(),
            );
        }
        if original_input.len() < HEADER_BYTES + 4 {
            return Err("ISO 8583 original input is shorter than its MTI".to_owned());
        }
        original_input[HEADER_BYTES..HEADER_BYTES + 4].copy_from_slice(message_type);
        Ok(original_input)
    }

    fn display(document_json: &str) -> Result<String, String> {
        let document = serde_json::from_str::<Iso8583Document>(document_json)
            .map_err(|error| error.to_string())?;
        Ok(format!(
            "<section class=\"protocol-document\"><h3>ISO 8583:1987 Message</h3><table><tbody><tr><th>MTI</th><td>{}</td></tr></tbody></table></section>",
            escape_html(&document.message_type)
        ))
    }
}

impl Guest for Iso8583AsciiStandard {
    fn upstream_frame(buffer: Vec<u8>) -> Result<FrameResult, PackageError> {
        Self::frame(&buffer).map_err(package_error("PROTOCOL_PACKAGE_INVALID"))
    }

    fn downstream_frame(buffer: Vec<u8>) -> Result<FrameResult, PackageError> {
        Self::frame(&buffer).map_err(package_error("PROTOCOL_PACKAGE_INVALID"))
    }

    fn upstream_decode(input: Vec<u8>) -> Result<String, PackageError> {
        Self::decode(&input).map_err(package_error("BODY_DECODE_FAILED"))
    }

    fn downstream_decode(input: Vec<u8>) -> Result<String, PackageError> {
        Self::decode(&input).map_err(package_error("BODY_DECODE_FAILED"))
    }

    fn upstream_encode(
        original_input: Vec<u8>,
        document_json: String,
    ) -> Result<Vec<u8>, PackageError> {
        Self::encode(original_input, &document_json).map_err(package_error("BODY_ENCODE_FAILED"))
    }

    fn downstream_encode(
        original_input: Vec<u8>,
        document_json: String,
    ) -> Result<Vec<u8>, PackageError> {
        Self::encode(original_input, &document_json).map_err(package_error("BODY_ENCODE_FAILED"))
    }

    fn upstream_display(document_json: String) -> Result<String, PackageError> {
        Self::display(&document_json).map_err(package_error("INTERNAL_ERROR"))
    }

    fn downstream_display(document_json: String) -> Result<String, PackageError> {
        Self::display(&document_json).map_err(package_error("INTERNAL_ERROR"))
    }
}

fn package_error(code: &'static str) -> impl FnOnce(String) -> PackageError {
    move |message| PackageError {
        code: code.to_owned(),
        message,
    }
}

fn escape_html(value: &str) -> String {
    value
        .replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&#39;")
}

#[cfg(target_arch = "wasm32")]
export!(Iso8583AsciiStandard);

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn frame_decode_encode_and_display_preserve_template_contract() {
        let request = [0_u8, 4, b'0', b'2', b'0', b'0'];
        assert!(matches!(
            Iso8583AsciiStandard::frame(&request),
            Ok(FrameResult::Complete(6))
        ));
        assert_eq!(
            Iso8583AsciiStandard::decode(&request).unwrap(),
            r#"{"message_type":"0200"}"#
        );
        assert_eq!(
            Iso8583AsciiStandard::encode(request.to_vec(), r#"{"message_type":"0210"}"#).unwrap(),
            [0_u8, 4, b'0', b'2', b'1', b'0']
        );
        assert!(
            Iso8583AsciiStandard::display(r#"{"message_type":"<0210>"}"#)
                .unwrap()
                .contains("&lt;0210&gt;")
        );
    }

    #[test]
    fn malformed_frames_and_documents_fail_closed() {
        assert!(matches!(
            Iso8583AsciiStandard::frame(&[0]),
            Ok(FrameResult::NeedMore(Some(2)))
        ));
        assert!(Iso8583AsciiStandard::decode(&[0, 5, b'0', b'2', b'0', b'0']).is_err());
        assert!(Iso8583AsciiStandard::encode(vec![0, 4, b'0', b'2', b'0', b'0'], "{}").is_err());
    }
}
