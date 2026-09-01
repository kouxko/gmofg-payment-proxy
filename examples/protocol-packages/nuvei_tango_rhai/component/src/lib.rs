#![cfg_attr(not(any(test, target_arch = "wasm32")), allow(dead_code))]

use serde::{Deserialize, Serialize};
use serde_json::Value;

wit_bindgen::generate!({
    path: "../../../../src-tauri/crates/package-runtime/wit",
    world: "socket-package",
});

const _: &str =
    include_str!("../../../../../src-tauri/crates/package-runtime/wit/protocol-package.wit");

const LENGTH_BYTES: usize = 4;
const CONTROL_BYTES: usize = 4;
const SEQUENCE_BYTES: usize = 8;
const MINIMUM_BODY_BYTES: usize = 14;
const MAXIMUM_BODY_BYTES: usize = 1_048_572;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum Direction {
    Upstream,
    Downstream,
}

#[derive(Debug, Deserialize, PartialEq, Serialize)]
struct NuveiTangoDocument {
    // The host Document serializes all numbers through its JavaScript-compatible f64 type.
    frame_length: f64,
    control_header: Vec<f64>,
    sequence: String,
    message_type: String,
    json_preview: String,
    encoding_context: Vec<f64>,
}

struct NuveiTangoJson;

impl NuveiTangoJson {
    fn frame(buffer: &[u8]) -> Result<FrameResult, String> {
        if buffer.len() < LENGTH_BYTES {
            return Ok(FrameResult::NeedMore(Some(LENGTH_BYTES as u64)));
        }

        let body_bytes = Self::declared_body_bytes(buffer)?;
        if body_bytes < MINIMUM_BODY_BYTES {
            return Ok(FrameResult::Reject(
                "Nuvei Tango length prefix is smaller than the minimum frame".to_owned(),
            ));
        }
        if body_bytes > MAXIMUM_BODY_BYTES {
            return Ok(FrameResult::Reject(
                "Nuvei Tango length prefix exceeds the 1 MiB package limit".to_owned(),
            ));
        }
        let frame_bytes = LENGTH_BYTES
            .checked_add(body_bytes)
            .ok_or_else(|| "Nuvei Tango frame length overflow".to_owned())?;
        if buffer.len() < frame_bytes {
            return Ok(FrameResult::NeedMore(Some(frame_bytes as u64)));
        }
        Ok(FrameResult::Complete(frame_bytes as u64))
    }

    fn decode(input: &[u8], direction: Direction) -> Result<String, String> {
        if input.len() < LENGTH_BYTES {
            return Err("Nuvei Tango frame is missing its length prefix".to_owned());
        }

        let body_bytes = Self::declared_body_bytes(input)?;
        Self::validate_body_length(body_bytes)?;
        if input.len() != LENGTH_BYTES + body_bytes {
            return Err("Nuvei Tango length prefix does not match the complete frame".to_owned());
        }

        let control_start = LENGTH_BYTES;
        let sequence_start = control_start + CONTROL_BYTES;
        let json_start = sequence_start + SEQUENCE_BYTES;
        let sequence_bytes = &input[sequence_start..json_start];
        if !sequence_bytes.iter().all(u8::is_ascii_digit) {
            return Err("Nuvei Tango sequence must contain exactly eight ASCII digits".to_owned());
        }
        let sequence = std::str::from_utf8(sequence_bytes).map_err(|_| {
            "Nuvei Tango sequence must contain exactly eight ASCII digits".to_owned()
        })?;

        let message = serde_json::from_slice::<Value>(&input[json_start..])
            .map_err(|error| format!("Nuvei Tango JSON payload is invalid: {error}"))?;
        Self::validate_integer_json(&message)?;
        let object = message.as_object().ok_or_else(|| {
            "Nuvei Tango JSON payload must contain one top-level message object".to_owned()
        })?;
        if object.len() != 1 {
            return Err(
                "Nuvei Tango JSON payload must contain one top-level message object".to_owned(),
            );
        }
        let message_type = object
            .keys()
            .next()
            .filter(|value| !value.is_empty())
            .ok_or_else(|| "Nuvei Tango message type must be a non-empty string".to_owned())?;

        serde_json::to_string(&NuveiTangoDocument {
            frame_length: body_bytes as f64,
            control_header: input[control_start..sequence_start]
                .iter()
                .map(|value| f64::from(*value))
                .collect(),
            sequence: sequence.to_owned(),
            message_type: message_type.to_owned(),
            json_preview: serde_json::to_string(&message).map_err(|error| error.to_string())?,
            encoding_context: Self::direction_context(direction),
        })
        .map_err(|error| error.to_string())
    }

    fn encode(
        original_input: Vec<u8>,
        document_json: &str,
        direction: Direction,
    ) -> Result<Vec<u8>, String> {
        let actual = serde_json::from_str::<NuveiTangoDocument>(document_json)
            .map_err(|error| format!("Nuvei Tango read-only document is invalid: {error}"))?;
        let expected_json = Self::decode(&original_input, direction)?;
        let expected = serde_json::from_str::<NuveiTangoDocument>(&expected_json)
            .map_err(|error| error.to_string())?;
        if actual != expected {
            return Err("Nuvei Tango read-only document was modified".to_owned());
        }
        Ok(original_input)
    }

    fn display(document_json: &str, direction: Direction) -> Result<String, String> {
        let document = serde_json::from_str::<NuveiTangoDocument>(document_json)
            .map_err(|_| "Nuvei Tango read-only document is missing a display field".to_owned())?;
        let label = match direction {
            Direction::Upstream => "Upstream",
            Direction::Downstream => "Downstream",
        };
        Ok(format!(
            "<section class=\"protocol-document\"><h3>Nuvei Tango JSON</h3><table><tbody><tr><th>Direction</th><td>{label}</td></tr><tr><th>Sequence</th><td>{}</td></tr><tr><th>Message type</th><td>{}</td></tr></tbody></table><pre>{}</pre></section>",
            escape_html(&document.sequence),
            escape_html(&document.message_type),
            escape_html(&document.json_preview),
        ))
    }

    fn declared_body_bytes(buffer: &[u8]) -> Result<usize, String> {
        let prefix: [u8; LENGTH_BYTES] = buffer[..LENGTH_BYTES]
            .try_into()
            .map_err(|_| "Nuvei Tango frame is missing its length prefix".to_owned())?;
        Ok(u32::from_be_bytes(prefix) as usize)
    }

    fn validate_body_length(body_bytes: usize) -> Result<(), String> {
        if body_bytes < MINIMUM_BODY_BYTES {
            return Err("Nuvei Tango length prefix is smaller than the minimum frame".to_owned());
        }
        if body_bytes > MAXIMUM_BODY_BYTES {
            return Err("Nuvei Tango length prefix exceeds the 1 MiB package limit".to_owned());
        }
        Ok(())
    }

    fn direction_context(direction: Direction) -> Vec<f64> {
        let direction = match direction {
            Direction::Upstream => b'U',
            Direction::Downstream => b'D',
        };
        [b'N', b'T', b'R', b'1', direction]
            .into_iter()
            .map(f64::from)
            .collect()
    }

    fn validate_integer_json(value: &Value) -> Result<(), String> {
        match value {
            Value::Number(number) if number.as_i64().is_none() => {
                Err("Nuvei Tango JSON numbers must be signed 64-bit integers".to_owned())
            }
            Value::Array(values) => values.iter().try_for_each(Self::validate_integer_json),
            Value::Object(values) => values.values().try_for_each(Self::validate_integer_json),
            _ => Ok(()),
        }
    }
}

impl Guest for NuveiTangoJson {
    fn upstream_frame(buffer: Vec<u8>) -> Result<FrameResult, PackageError> {
        Self::frame(&buffer).map_err(package_error("PROTOCOL_PACKAGE_INVALID"))
    }

    fn downstream_frame(buffer: Vec<u8>) -> Result<FrameResult, PackageError> {
        Self::frame(&buffer).map_err(package_error("PROTOCOL_PACKAGE_INVALID"))
    }

    fn upstream_decode(input: Vec<u8>) -> Result<String, PackageError> {
        Self::decode(&input, Direction::Upstream).map_err(package_error("BODY_DECODE_FAILED"))
    }

    fn downstream_decode(input: Vec<u8>) -> Result<String, PackageError> {
        Self::decode(&input, Direction::Downstream).map_err(package_error("BODY_DECODE_FAILED"))
    }

    fn upstream_encode(
        original_input: Vec<u8>,
        document_json: String,
    ) -> Result<Vec<u8>, PackageError> {
        Self::encode(original_input, &document_json, Direction::Upstream)
            .map_err(package_error("BODY_ENCODE_FAILED"))
    }

    fn downstream_encode(
        original_input: Vec<u8>,
        document_json: String,
    ) -> Result<Vec<u8>, PackageError> {
        Self::encode(original_input, &document_json, Direction::Downstream)
            .map_err(package_error("BODY_ENCODE_FAILED"))
    }

    fn upstream_display(document_json: String) -> Result<String, PackageError> {
        Self::display(&document_json, Direction::Upstream)
            .map_err(package_error("INTERNAL_ERROR"))
    }

    fn downstream_display(document_json: String) -> Result<String, PackageError> {
        Self::display(&document_json, Direction::Downstream)
            .map_err(package_error("INTERNAL_ERROR"))
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
export!(NuveiTangoJson);

#[cfg(test)]
mod tests {
    use super::*;

    const CONTROL: [u8; CONTROL_BYTES] = [0x01, 0x00, 0x01, 0x00];
    const SEQUENCE: &[u8; SEQUENCE_BYTES] = b"00000020";

    #[test]
    fn frame_handles_fragment_sticky_data_and_boundaries() {
        assert!(matches!(
            NuveiTangoJson::frame(&[0, 0, 0]).unwrap(),
            FrameResult::NeedMore(Some(4))
        ));
        assert!(matches!(
            NuveiTangoJson::frame(&13_u32.to_be_bytes()).unwrap(),
            FrameResult::Reject(message)
                if message == "Nuvei Tango length prefix is smaller than the minimum frame"
        ));
        assert!(matches!(
            NuveiTangoJson::frame(&(MAXIMUM_BODY_BYTES as u32).to_be_bytes()).unwrap(),
            FrameResult::NeedMore(Some(1_048_576))
        ));
        assert!(matches!(
            NuveiTangoJson::frame(&(MAXIMUM_BODY_BYTES as u32 + 1).to_be_bytes()).unwrap(),
            FrameResult::Reject(message)
                if message == "Nuvei Tango length prefix exceeds the 1 MiB package limit"
        ));

        let frame = test_frame(br#"{"Message":{}}"#);
        let sticky = [frame.as_slice(), b"next"].concat();
        assert!(matches!(
            NuveiTangoJson::frame(&sticky).unwrap(),
            FrameResult::Complete(bytes) if bytes == frame.len() as u64
        ));
    }

    #[test]
    fn decode_encode_and_display_preserve_read_only_contract() {
        let frame = test_frame(br#"{"Message":{"value":"<synthetic>"}}"#);
        let upstream = NuveiTangoJson::decode(&frame, Direction::Upstream).unwrap();
        let document: NuveiTangoDocument = serde_json::from_str(&upstream).unwrap();
        assert_eq!(document.frame_length, (frame.len() - LENGTH_BYTES) as f64);
        assert_eq!(document.control_header, vec![1.0, 0.0, 1.0, 0.0]);
        assert_eq!(document.sequence, "00000020");
        assert_eq!(document.message_type, "Message");
        assert_eq!(
            document.encoding_context,
            vec![78.0, 84.0, 82.0, 49.0, 85.0]
        );
        assert_eq!(
            NuveiTangoJson::encode(frame.clone(), &upstream, Direction::Upstream).unwrap(),
            frame
        );

        let html = NuveiTangoJson::display(&upstream, Direction::Upstream).unwrap();
        assert!(html.contains("<td>Upstream</td>"));
        assert!(html.contains("&lt;synthetic&gt;"));
        assert!(!html.contains("<synthetic>"));
    }

    #[test]
    fn repository_fixtures_decode_and_round_trip_without_changing_bytes() {
        for (direction, payload, message_type) in [
            (
                Direction::Upstream,
                include_bytes!("../../tests/fixtures/request.json").as_slice(),
                "AccptrAuthstnReq",
            ),
            (
                Direction::Downstream,
                include_bytes!("../../tests/fixtures/response.json").as_slice(),
                "AccptrAuthstnRspn",
            ),
        ] {
            let frame = test_frame(payload);
            let document_json = NuveiTangoJson::decode(&frame, direction).unwrap();
            let document: NuveiTangoDocument = serde_json::from_str(&document_json).unwrap();
            assert_eq!(document.message_type, message_type);
            assert_eq!(
                serde_json::from_str::<Value>(&document.json_preview).unwrap(),
                serde_json::from_slice::<Value>(payload).unwrap()
            );
            assert_eq!(
                NuveiTangoJson::encode(frame.clone(), &document_json, direction).unwrap(),
                frame
            );
        }
    }

    #[test]
    fn every_field_change_removal_and_cross_direction_reuse_fail_closed() {
        let frame = test_frame(br#"{"Message":{}}"#);
        let upstream = NuveiTangoJson::decode(&frame, Direction::Upstream).unwrap();
        for (field, replacement) in [
            ("frame_length", serde_json::json!(1)),
            ("control_header", serde_json::json!([0, 0, 0, 0])),
            ("sequence", serde_json::json!("99999999")),
            ("message_type", serde_json::json!("Changed")),
            ("json_preview", serde_json::json!("{}")),
            ("encoding_context", serde_json::json!([1, 2, 3, 4, 5])),
        ] {
            let mut changed = serde_json::from_str::<Value>(&upstream).unwrap();
            changed[field] = replacement;
            assert!(
                NuveiTangoJson::encode(
                    frame.clone(),
                    &serde_json::to_string(&changed).unwrap(),
                    Direction::Upstream,
                )
                .is_err(),
                "changed field {field} must fail closed"
            );

            let mut removed = serde_json::from_str::<Value>(&upstream).unwrap();
            removed.as_object_mut().unwrap().remove(field);
            assert!(
                NuveiTangoJson::encode(
                    frame.clone(),
                    &serde_json::to_string(&removed).unwrap(),
                    Direction::Upstream,
                )
                .is_err(),
                "removed field {field} must fail closed"
            );
        }
        assert!(NuveiTangoJson::encode(frame, &upstream, Direction::Downstream).is_err());
    }

    #[test]
    fn invalid_sequence_json_shape_and_non_integer_numbers_fail_closed() {
        let mut invalid_sequence = test_frame(br#"{"Message":{}}"#);
        invalid_sequence[LENGTH_BYTES + CONTROL_BYTES] = b'X';
        assert!(NuveiTangoJson::decode(&invalid_sequence, Direction::Upstream).is_err());
        assert!(NuveiTangoJson::decode(&test_frame(b"[]"), Direction::Upstream).is_err());
        assert!(NuveiTangoJson::decode(&test_frame(b"{}"), Direction::Upstream).is_err());
        assert!(
            NuveiTangoJson::decode(
                &test_frame(br#"{"Message":{"amount":1.5}}"#),
                Direction::Upstream,
            )
            .is_err()
        );
    }

    fn test_frame(payload: &[u8]) -> Vec<u8> {
        let body_bytes = CONTROL_BYTES + SEQUENCE_BYTES + payload.len();
        [
            (body_bytes as u32).to_be_bytes().as_slice(),
            CONTROL.as_slice(),
            SEQUENCE.as_slice(),
            payload,
        ]
        .concat()
    }
}
