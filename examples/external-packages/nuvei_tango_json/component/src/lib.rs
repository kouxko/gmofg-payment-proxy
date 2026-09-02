use std::{
    cell::RefCell,
    collections::{HashMap, VecDeque},
};

use base64::{Engine as _, engine::general_purpose::STANDARD as BASE64};
use hmac::{Hmac, Mac};
use serde::{Deserialize, Deserializer, Serialize, de};
use serde_json::{Map, Number, Value};
use sha2::Sha256;

wit_bindgen::generate!({
    path: "../../../../src-tauri/crates/package-runtime/wit",
    world: "socket-package",
});

const _: &str =
    include_str!("../../../../../src-tauri/crates/package-runtime/wit/protocol-package.wit");

const LENGTH_BYTES: usize = 4;
const CONTROL_BYTES: usize = 4;
const SEQUENCE_BYTES: usize = 8;
const MINIMUM_BODY_BYTES: usize = CONTROL_BYTES + SEQUENCE_BYTES + 2;
const MAXIMUM_BODY_BYTES: usize = 1024 * 1024 - LENGTH_BYTES;
const CONTEXT_MAGIC: &[u8; 4] = b"NTJ1";
const CONTEXT_TOKEN_BYTES: usize = 32;
const CONTEXT_TAG_BYTES: usize = 32;
const MAXIMUM_CONTEXTS: usize = 4096;
const MAXIMUM_CONTEXT_BYTES: usize = 16 * 1024 * 1024;
const SENSITIVE_KEY_PARTS: [&str; 10] = [
    "pan",
    "track2",
    "track1",
    "pin",
    "mac",
    "key",
    "ksn",
    "cryptogram",
    "iccrltddata",
    "iccrelateddata",
];

type HmacSha256 = Hmac<Sha256>;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum Direction {
    Upstream,
    Downstream,
}

impl Direction {
    const fn as_bytes(self) -> &'static [u8] {
        match self {
            Self::Upstream => b"upstream",
            Self::Downstream => b"downstream",
        }
    }

    const fn label(self) -> &'static str {
        match self {
            Self::Upstream => "Upstream",
            Self::Downstream => "Downstream",
        }
    }
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
struct TaggedString {
    r#type: String,
    value: String,
}

impl TaggedString {
    fn new(r#type: &str, value: impl Into<String>) -> Self {
        Self {
            r#type: r#type.to_owned(),
            value: value.into(),
        }
    }

    fn require_type(&self, expected: &str, name: &str) -> Result<(), String> {
        if self.r#type == expected {
            Ok(())
        } else {
            Err(format!("Nuvei Tango {name} has an invalid tagged type"))
        }
    }
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
struct TaggedBlob {
    r#type: String,
    value_base64: String,
}

impl TaggedBlob {
    fn new(value: &[u8]) -> Self {
        Self {
            r#type: "blob".to_owned(),
            value_base64: BASE64.encode(value),
        }
    }

    fn decode(&self, name: &str) -> Result<Vec<u8>, String> {
        if self.r#type != "blob" {
            return Err(format!("Nuvei Tango {name} must be a canonical blob"));
        }
        let raw = BASE64
            .decode(&self.value_base64)
            .map_err(|_| format!("Nuvei Tango {name} contains invalid Base64"))?;
        if BASE64.encode(&raw) != self.value_base64 {
            return Err(format!("Nuvei Tango {name} Base64 must be canonical"));
        }
        Ok(raw)
    }
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
struct PublicDocument {
    frame_length: TaggedString,
    control_header: TaggedBlob,
    sequence: TaggedString,
    message_type: TaggedString,
    json_preview: TaggedString,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
struct TangoDocument {
    frame_length: TaggedString,
    control_header: TaggedBlob,
    sequence: TaggedString,
    message_type: TaggedString,
    json_preview: TaggedString,
    encoding_context: TaggedBlob,
}

impl TangoDocument {
    fn public(&self) -> PublicDocument {
        PublicDocument {
            frame_length: self.frame_length.clone(),
            control_header: self.control_header.clone(),
            sequence: self.sequence.clone(),
            message_type: self.message_type.clone(),
            json_preview: self.json_preview.clone(),
        }
    }

    fn validate_tags(&self) -> Result<(), String> {
        self.frame_length.require_type("int", "frame_length")?;
        if self.control_header.r#type != "blob" {
            return Err("Nuvei Tango control_header must be a canonical blob".to_owned());
        }
        self.sequence.require_type("string", "sequence")?;
        self.message_type.require_type("string", "message_type")?;
        self.json_preview.require_type("string", "json_preview")?;
        Ok(())
    }
}

#[derive(Clone)]
struct StoredContext {
    direction: Direction,
    frame: Vec<u8>,
    public: PublicDocument,
}

struct CodecState {
    key: [u8; 32],
    contexts: HashMap<[u8; CONTEXT_TOKEN_BYTES], StoredContext>,
    order: VecDeque<[u8; CONTEXT_TOKEN_BYTES]>,
    context_bytes: usize,
}

impl CodecState {
    fn random() -> Result<Self, String> {
        let mut key = [0_u8; 32];
        getrandom::fill(&mut key)
            .map_err(|error| format!("Nuvei Tango context key generation failed: {error}"))?;
        Ok(Self::new(key))
    }

    fn new(key: [u8; 32]) -> Self {
        Self {
            key,
            contexts: HashMap::new(),
            order: VecDeque::new(),
            context_bytes: 0,
        }
    }

    fn store(
        &mut self,
        direction: Direction,
        frame: &[u8],
        public: PublicDocument,
    ) -> Result<TaggedBlob, String> {
        let mut token = [0_u8; CONTEXT_TOKEN_BYTES];
        getrandom::fill(&mut token)
            .map_err(|error| format!("Nuvei Tango context token generation failed: {error}"))?;
        let tag = self.tag(direction, &token)?;
        self.context_bytes += frame.len();
        self.contexts.insert(
            token,
            StoredContext {
                direction,
                frame: frame.to_vec(),
                public,
            },
        );
        self.touch(token);
        while self.contexts.len() > MAXIMUM_CONTEXTS || self.context_bytes > MAXIMUM_CONTEXT_BYTES {
            let Some(oldest) = self.order.pop_front() else {
                break;
            };
            if let Some(evicted) = self.contexts.remove(&oldest) {
                self.context_bytes = self.context_bytes.saturating_sub(evicted.frame.len());
            }
        }
        let mut envelope =
            Vec::with_capacity(CONTEXT_MAGIC.len() + CONTEXT_TOKEN_BYTES + CONTEXT_TAG_BYTES);
        envelope.extend_from_slice(CONTEXT_MAGIC);
        envelope.extend_from_slice(&token);
        envelope.extend_from_slice(&tag);
        Ok(TaggedBlob::new(&envelope))
    }

    fn load(
        &mut self,
        direction: Direction,
        context: &TaggedBlob,
    ) -> Result<StoredContext, String> {
        let raw = context.decode("encoding context")?;
        if raw.len() != CONTEXT_MAGIC.len() + CONTEXT_TOKEN_BYTES + CONTEXT_TAG_BYTES
            || !raw.starts_with(CONTEXT_MAGIC)
        {
            return Err("Nuvei Tango encoding context has an invalid envelope".to_owned());
        }
        let token: [u8; CONTEXT_TOKEN_BYTES] = raw
            [CONTEXT_MAGIC.len()..CONTEXT_MAGIC.len() + CONTEXT_TOKEN_BYTES]
            .try_into()
            .map_err(|_| "Nuvei Tango encoding context has an invalid token".to_owned())?;
        let supplied_tag = &raw[raw.len() - CONTEXT_TAG_BYTES..];
        self.verify_tag(direction, &token, supplied_tag)?;
        let stored = self
            .contexts
            .get(&token)
            .filter(|stored| stored.direction == direction)
            .cloned()
            .ok_or_else(|| "Nuvei Tango encoding context is unavailable".to_owned())?;
        self.touch(token);
        Ok(stored)
    }

    fn tag(
        &self,
        direction: Direction,
        token: &[u8; CONTEXT_TOKEN_BYTES],
    ) -> Result<[u8; CONTEXT_TAG_BYTES], String> {
        let mut mac = HmacSha256::new_from_slice(&self.key)
            .map_err(|_| "Nuvei Tango context key is invalid".to_owned())?;
        mac.update(CONTEXT_MAGIC);
        mac.update(direction.as_bytes());
        mac.update(token);
        Ok(mac.finalize().into_bytes().into())
    }

    fn verify_tag(
        &self,
        direction: Direction,
        token: &[u8; CONTEXT_TOKEN_BYTES],
        supplied_tag: &[u8],
    ) -> Result<(), String> {
        let mut mac = HmacSha256::new_from_slice(&self.key)
            .map_err(|_| "Nuvei Tango context key is invalid".to_owned())?;
        mac.update(CONTEXT_MAGIC);
        mac.update(direction.as_bytes());
        mac.update(token);
        mac.verify_slice(supplied_tag)
            .map_err(|_| "Nuvei Tango encoding context authentication failed".to_owned())
    }

    fn touch(&mut self, token: [u8; CONTEXT_TOKEN_BYTES]) {
        self.order.retain(|candidate| candidate != &token);
        self.order.push_back(token);
    }
}

thread_local! {
    static CODEC_STATE: RefCell<Result<CodecState, String>> = RefCell::new(CodecState::random());
}

struct NuveiTangoJson;

impl NuveiTangoJson {
    fn frame(buffer: &[u8]) -> Result<FrameResult, String> {
        if buffer.len() < LENGTH_BYTES {
            return Ok(FrameResult::NeedMore(None));
        }
        let body_bytes = u32::from_be_bytes(buffer[..LENGTH_BYTES].try_into().unwrap()) as usize;
        validate_declared_length(body_bytes)?;
        let frame_bytes = LENGTH_BYTES
            .checked_add(body_bytes)
            .ok_or_else(|| "Nuvei Tango frame length overflow".to_owned())?;
        if buffer.len() < frame_bytes {
            return Ok(FrameResult::NeedMore(None));
        }
        Ok(FrameResult::Complete(frame_bytes as u64))
    }

    fn decode(direction: Direction, frame: &[u8]) -> Result<String, String> {
        let (control, sequence, message_type, message) = decode_frame(frame)?;
        let preview = serde_json::to_string_pretty(&redact(message))
            .map_err(|error| format!("Nuvei Tango JSON preview failed: {error}"))?;
        let public = PublicDocument {
            frame_length: TaggedString::new("int", (frame.len() - LENGTH_BYTES).to_string()),
            control_header: TaggedBlob::new(control),
            sequence: TaggedString::new("string", sequence),
            message_type: TaggedString::new("string", message_type),
            json_preview: TaggedString::new("string", preview),
        };
        let encoding_context = CODEC_STATE.with(|state| {
            let mut state = state.borrow_mut();
            state
                .as_mut()
                .map_err(|error| error.clone())?
                .store(direction, frame, public.clone())
        })?;
        serde_json::to_string(&TangoDocument {
            frame_length: public.frame_length,
            control_header: public.control_header,
            sequence: public.sequence,
            message_type: public.message_type,
            json_preview: public.json_preview,
            encoding_context,
        })
        .map_err(|error| format!("Nuvei Tango Document serialization failed: {error}"))
    }

    fn encode(
        direction: Direction,
        original_input: Vec<u8>,
        document_json: &str,
    ) -> Result<Vec<u8>, String> {
        let document: TangoDocument = serde_json::from_str(document_json)
            .map_err(|error| format!("Nuvei Tango Document is invalid: {error}"))?;
        document.validate_tags()?;
        let stored = CODEC_STATE.with(|state| {
            let mut state = state.borrow_mut();
            state
                .as_mut()
                .map_err(|error| error.clone())?
                .load(direction, &document.encoding_context)
        })?;
        if document.public() != stored.public || original_input != stored.frame {
            return Err("Nuvei Tango read-only document was modified".to_owned());
        }
        Ok(stored.frame)
    }

    fn display(direction: Direction, document_json: &str) -> Result<String, String> {
        let document: TangoDocument = serde_json::from_str(document_json)
            .map_err(|error| format!("Nuvei Tango Document is invalid: {error}"))?;
        document.validate_tags()?;
        let stored = CODEC_STATE.with(|state| {
            let mut state = state.borrow_mut();
            state
                .as_mut()
                .map_err(|error| error.clone())?
                .load(direction, &document.encoding_context)
        })?;
        if document.public() != stored.public {
            return Err("Nuvei Tango read-only document was modified".to_owned());
        }
        let preview = serde_json::from_str::<Value>(&document.json_preview.value)
            .map_err(|error| format!("Nuvei Tango JSON preview is invalid: {error}"))?;
        Ok(format!(
            "<section class=\"protocol-document\"><h3>Nuvei Tango JSON</h3><table><tbody><tr><th>Direction</th><td>{}</td></tr><tr><th>Sequence</th><td>{}</td></tr><tr><th>Message type</th><td>{}</td></tr></tbody></table>{}</section>",
            escape_html(direction.label()),
            escape_html(&document.sequence.value),
            escape_html(&document.message_type.value),
            render_json(&preview),
        ))
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
        Self::decode(Direction::Upstream, &input).map_err(package_error("BODY_DECODE_FAILED"))
    }

    fn downstream_decode(input: Vec<u8>) -> Result<String, PackageError> {
        Self::decode(Direction::Downstream, &input).map_err(package_error("BODY_DECODE_FAILED"))
    }

    fn upstream_encode(
        original_input: Vec<u8>,
        document_json: String,
    ) -> Result<Vec<u8>, PackageError> {
        Self::encode(Direction::Upstream, original_input, &document_json)
            .map_err(package_error("BODY_ENCODE_FAILED"))
    }

    fn downstream_encode(
        original_input: Vec<u8>,
        document_json: String,
    ) -> Result<Vec<u8>, PackageError> {
        Self::encode(Direction::Downstream, original_input, &document_json)
            .map_err(package_error("BODY_ENCODE_FAILED"))
    }

    fn upstream_display(document_json: String) -> Result<String, PackageError> {
        Self::display(Direction::Upstream, &document_json).map_err(package_error("INTERNAL_ERROR"))
    }

    fn downstream_display(document_json: String) -> Result<String, PackageError> {
        Self::display(Direction::Downstream, &document_json)
            .map_err(package_error("INTERNAL_ERROR"))
    }
}

fn package_error(code: &'static str) -> impl FnOnce(String) -> PackageError {
    move |message| PackageError {
        code: code.to_owned(),
        message,
    }
}

fn decode_frame(frame: &[u8]) -> Result<(&[u8], String, String, Value), String> {
    if frame.len() < LENGTH_BYTES {
        return Err("Nuvei Tango frame is missing its length prefix".to_owned());
    }
    let declared = u32::from_be_bytes(frame[..LENGTH_BYTES].try_into().unwrap()) as usize;
    validate_declared_length(declared)?;
    if frame.len() != LENGTH_BYTES + declared {
        return Err("Nuvei Tango length prefix does not match the complete frame".to_owned());
    }
    let body = &frame[LENGTH_BYTES..];
    let control = &body[..CONTROL_BYTES];
    let sequence_bytes = &body[CONTROL_BYTES..CONTROL_BYTES + SEQUENCE_BYTES];
    if !sequence_bytes.iter().all(u8::is_ascii_digit) {
        return Err("Nuvei Tango sequence must contain exactly eight ASCII digits".to_owned());
    }
    let sequence = std::str::from_utf8(sequence_bytes)
        .map_err(|_| "Nuvei Tango sequence must contain exactly eight ASCII digits".to_owned())?
        .to_owned();
    let json_text = std::str::from_utf8(&body[CONTROL_BYTES + SEQUENCE_BYTES..])
        .map_err(|_| "Nuvei Tango JSON payload must be UTF-8".to_owned())?;
    let message = serde_json::from_str::<NoDuplicateValue>(json_text)
        .map_err(|_| "Nuvei Tango JSON payload is invalid".to_owned())?
        .0;
    let object = message.as_object().ok_or_else(|| {
        "Nuvei Tango JSON payload must contain one top-level message object".to_owned()
    })?;
    if object.len() != 1 {
        return Err(
            "Nuvei Tango JSON payload must contain one top-level message object".to_owned(),
        );
    }
    let message_type = object.keys().next().unwrap();
    if message_type.is_empty() {
        return Err("Nuvei Tango message type must be a non-empty string".to_owned());
    }
    Ok((control, sequence, message_type.clone(), message))
}

fn validate_declared_length(body_bytes: usize) -> Result<(), String> {
    if body_bytes < MINIMUM_BODY_BYTES {
        return Err("Nuvei Tango length prefix is smaller than the minimum frame".to_owned());
    }
    if body_bytes > MAXIMUM_BODY_BYTES {
        return Err("Nuvei Tango length prefix exceeds the 1 MiB package limit".to_owned());
    }
    Ok(())
}

fn redact(value: Value) -> Value {
    redact_at(value, "")
}

fn redact_at(value: Value, key: &str) -> Value {
    let normalized: String = key
        .chars()
        .flat_map(char::to_lowercase)
        .filter(|character| character.is_alphanumeric())
        .collect();
    if SENSITIVE_KEY_PARTS
        .iter()
        .any(|part| normalized.contains(part))
    {
        return Value::String("[redacted]".to_owned());
    }
    match value {
        Value::Object(values) => Value::Object(
            values
                .into_iter()
                .map(|(item_key, item)| {
                    let redacted = redact_at(item, &item_key);
                    (item_key, redacted)
                })
                .collect(),
        ),
        Value::Array(values) => {
            Value::Array(values.into_iter().map(|item| redact_at(item, "")).collect())
        }
        scalar => scalar,
    }
}

fn escape_html(value: &str) -> String {
    value
        .replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&#x27;")
}

fn render_json(value: &Value) -> String {
    match value {
        Value::Object(values) => {
            let rows = values
                .iter()
                .map(|(name, value)| {
                    format!(
                        "<tr><th>{}</th><td>{}</td></tr>",
                        escape_html(name),
                        render_json(value)
                    )
                })
                .collect::<String>();
            format!("<table class=\"protocol-document-nested\"><tbody>{rows}</tbody></table>")
        }
        Value::Array(values) => {
            let rows = values
                .iter()
                .enumerate()
                .map(|(index, value)| {
                    format!("<tr><th>[{index}]</th><td>{}</td></tr>", render_json(value))
                })
                .collect::<String>();
            format!("<table class=\"protocol-document-nested\"><tbody>{rows}</tbody></table>")
        }
        Value::String(value) => escape_html(value),
        Value::Number(value) => value.to_string(),
        Value::Bool(value) => value.to_string(),
        Value::Null => "null".to_owned(),
    }
}

struct NoDuplicateValue(Value);

impl<'de> Deserialize<'de> for NoDuplicateValue {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        struct Visitor;

        impl<'de> de::Visitor<'de> for Visitor {
            type Value = NoDuplicateValue;

            fn expecting(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                formatter.write_str("a JSON value without duplicate object keys")
            }

            fn visit_unit<E>(self) -> Result<Self::Value, E> {
                Ok(NoDuplicateValue(Value::Null))
            }

            fn visit_none<E>(self) -> Result<Self::Value, E>
            where
                E: de::Error,
            {
                Ok(NoDuplicateValue(Value::Null))
            }

            fn visit_bool<E>(self, value: bool) -> Result<Self::Value, E> {
                Ok(NoDuplicateValue(Value::Bool(value)))
            }

            fn visit_i64<E>(self, value: i64) -> Result<Self::Value, E> {
                Ok(NoDuplicateValue(Value::Number(Number::from(value))))
            }

            fn visit_u64<E>(self, value: u64) -> Result<Self::Value, E> {
                Ok(NoDuplicateValue(Value::Number(Number::from(value))))
            }

            fn visit_f64<E>(self, value: f64) -> Result<Self::Value, E>
            where
                E: de::Error,
            {
                Number::from_f64(value)
                    .map(|value| NoDuplicateValue(Value::Number(value)))
                    .ok_or_else(|| E::custom("non-finite JSON number"))
            }

            fn visit_str<E>(self, value: &str) -> Result<Self::Value, E> {
                Ok(NoDuplicateValue(Value::String(value.to_owned())))
            }

            fn visit_string<E>(self, value: String) -> Result<Self::Value, E> {
                Ok(NoDuplicateValue(Value::String(value)))
            }

            fn visit_seq<A>(self, mut sequence: A) -> Result<Self::Value, A::Error>
            where
                A: de::SeqAccess<'de>,
            {
                let mut values = Vec::new();
                while let Some(value) = sequence.next_element::<NoDuplicateValue>()? {
                    values.push(value.0);
                }
                Ok(NoDuplicateValue(Value::Array(values)))
            }

            fn visit_map<A>(self, mut object: A) -> Result<Self::Value, A::Error>
            where
                A: de::MapAccess<'de>,
            {
                let mut values = Map::new();
                while let Some((key, value)) = object.next_entry::<String, NoDuplicateValue>()? {
                    if values.insert(key.clone(), value.0).is_some() {
                        return Err(de::Error::custom(format!(
                            "duplicate JSON object key {key}"
                        )));
                    }
                }
                Ok(NoDuplicateValue(Value::Object(values)))
            }
        }

        deserializer.deserialize_any(Visitor)
    }
}

#[cfg(target_arch = "wasm32")]
export!(NuveiTangoJson);

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn synthetic_frame(payload: Value) -> Vec<u8> {
        let json = serde_json::to_vec(&payload).unwrap();
        let mut body = Vec::new();
        body.extend_from_slice(&[1, 0, 1, 0]);
        body.extend_from_slice(b"00000020");
        body.extend_from_slice(&json);
        let mut frame = Vec::new();
        frame.extend_from_slice(&(body.len() as u32).to_be_bytes());
        frame.extend_from_slice(&body);
        frame
    }

    fn with_test_state<T>(operation: impl FnOnce() -> T) -> T {
        CODEC_STATE.with(|state| {
            *state.borrow_mut() = Ok(CodecState::new([b'c'; 32]));
        });
        operation()
    }

    #[test]
    fn frame_handles_fragmentation_and_sticky_buffers() {
        let frame = synthetic_frame(json!({"Message": {"value": 1}}));
        assert!(matches!(
            NuveiTangoJson::frame(&frame[..3]).unwrap(),
            FrameResult::NeedMore(None)
        ));
        assert!(matches!(
            NuveiTangoJson::frame(&frame[..frame.len() - 1]).unwrap(),
            FrameResult::NeedMore(None)
        ));
        let mut sticky = frame.clone();
        sticky.extend_from_slice(b"next");
        assert!(matches!(
            NuveiTangoJson::frame(&sticky).unwrap(),
            FrameResult::Complete(consumed) if consumed == frame.len() as u64
        ));
    }

    #[test]
    fn decode_masks_sensitive_values_and_encode_returns_the_exact_frame() {
        with_test_state(|| {
            let frame = synthetic_frame(json!({
                "AccptrAuthstnReq": {
                    "PAN": "synthetic-pan",
                    "Track2": "synthetic-track-data",
                    "MAC": "secret-mac",
                    "KeyId": "secret-key",
                    "ICCRltdData": "sensitive-icc-data"
                }
            }));
            let document = NuveiTangoJson::decode(Direction::Upstream, &frame).unwrap();
            let value: Value = serde_json::from_str(&document).unwrap();
            assert_eq!(
                value["frame_length"]["value"],
                (frame.len() - LENGTH_BYTES).to_string()
            );
            assert_eq!(value["sequence"]["value"], "00000020");
            assert_eq!(value["message_type"]["value"], "AccptrAuthstnReq");
            let preview = value["json_preview"]["value"].as_str().unwrap();
            for sensitive in [
                "synthetic-pan",
                "synthetic-track-data",
                "secret-mac",
                "secret-key",
                "sensitive-icc-data",
            ] {
                assert!(!preview.contains(sensitive));
            }
            assert!(preview.contains("[redacted]"));
            assert_eq!(
                NuveiTangoJson::encode(Direction::Upstream, frame.clone(), &document).unwrap(),
                frame
            );
        });
    }

    #[test]
    fn encode_rejects_document_changes_context_tampering_and_cross_direction_use() {
        with_test_state(|| {
            let frame = synthetic_frame(json!({"Message": {"value": 1}}));
            let document = NuveiTangoJson::decode(Direction::Upstream, &frame).unwrap();
            let mut changed: Value = serde_json::from_str(&document).unwrap();
            changed["sequence"]["value"] = Value::String("changed".to_owned());
            assert!(
                NuveiTangoJson::encode(
                    Direction::Upstream,
                    frame.clone(),
                    &serde_json::to_string(&changed).unwrap()
                )
                .unwrap_err()
                .contains("modified")
            );
            assert!(
                NuveiTangoJson::encode(Direction::Downstream, frame.clone(), &document).is_err()
            );
            let mut tampered: Value = serde_json::from_str(&document).unwrap();
            tampered["encoding_context"]["value_base64"] = Value::String("AA==".to_owned());
            assert!(
                NuveiTangoJson::encode(
                    Direction::Upstream,
                    frame,
                    &serde_json::to_string(&tampered).unwrap()
                )
                .is_err()
            );
        });
    }

    #[test]
    fn decode_rejects_invalid_sequence_duplicate_keys_and_non_object_json() {
        let mut invalid_sequence = synthetic_frame(json!({"Message": {}}));
        invalid_sequence[LENGTH_BYTES + CONTROL_BYTES] = b'A';
        assert!(NuveiTangoJson::decode(Direction::Upstream, &invalid_sequence).is_err());

        for json in [br#"{"A":1,"A":2}"#.as_slice(), b"[]", br#"{"A":NaN}"#] {
            let mut body = Vec::from([1, 0, 1, 0]);
            body.extend_from_slice(b"00000020");
            body.extend_from_slice(json);
            let mut frame = Vec::new();
            frame.extend_from_slice(&(body.len() as u32).to_be_bytes());
            frame.extend_from_slice(&body);
            assert!(NuveiTangoJson::decode(Direction::Upstream, &frame).is_err());
        }
    }

    #[test]
    fn display_escapes_html_and_never_exposes_sensitive_values() {
        with_test_state(|| {
            let frame = synthetic_frame(json!({"<Message>": {
                "PAN": "synthetic-pan",
                "note": "<script>"
            }}));
            let document = NuveiTangoJson::decode(Direction::Downstream, &frame).unwrap();
            let rendered = NuveiTangoJson::display(Direction::Downstream, &document).unwrap();
            assert!(rendered.contains("&lt;Message&gt;"));
            assert!(rendered.contains("&lt;script&gt;"));
            assert!(rendered.contains("<table class=\"protocol-document-nested\">"));
            assert!(!rendered.contains("<pre>"));
            assert!(!rendered.contains("synthetic-pan"));
            assert!(!rendered.contains("<script>"));
        });
    }
}
