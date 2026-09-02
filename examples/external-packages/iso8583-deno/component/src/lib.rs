#![cfg_attr(not(any(test, target_arch = "wasm32")), allow(dead_code))]

use base64::{Engine as _, engine::general_purpose::STANDARD as BASE64};
use serde_json::{Map, Number, Value};

wit_bindgen::generate!({
    path: "../../../../src-tauri/crates/package-runtime/wit",
    world: "socket-package",
});

const HEADER_BYTES: usize = 2;
const MAX_FRAME_BYTES: usize = 65_535;

#[derive(Clone, Copy, Eq, PartialEq)]
enum FieldKind {
    FixedAscii,
    FixedDigits,
    FixedAmount,
    FixedBlob,
    LlAscii,
    LlDigits,
    LllAscii,
    LllBlob,
}

#[derive(Clone, Copy, Eq, PartialEq)]
enum DocumentKind {
    String,
    Integer,
    Blob,
}

#[derive(Clone, Copy)]
struct FieldSpec {
    number: u8,
    name: &'static str,
    label: &'static str,
    kind: FieldKind,
    length: usize,
    document_kind: DocumentKind,
}

const fn field(
    number: u8,
    name: &'static str,
    label: &'static str,
    kind: FieldKind,
    length: usize,
) -> FieldSpec {
    FieldSpec {
        number,
        name,
        label,
        kind,
        length,
        document_kind: DocumentKind::String,
    }
}

const fn typed_field(
    number: u8,
    name: &'static str,
    label: &'static str,
    kind: FieldKind,
    length: usize,
    document_kind: DocumentKind,
) -> FieldSpec {
    FieldSpec {
        number,
        name,
        label,
        kind,
        length,
        document_kind,
    }
}

const FIELD_SPECS: &[FieldSpec] = &[
    field(
        2,
        "primary_account_number",
        "DE2 Primary Account Number",
        FieldKind::LlDigits,
        19,
    ),
    field(
        3,
        "processing_code",
        "DE3 Processing Code",
        FieldKind::FixedDigits,
        6,
    ),
    typed_field(
        4,
        "amount",
        "DE4 Transaction Amount",
        FieldKind::FixedAmount,
        12,
        DocumentKind::Integer,
    ),
    field(
        7,
        "transmission_time",
        "DE7 Transmission Date And Time",
        FieldKind::FixedDigits,
        10,
    ),
    field(
        11,
        "stan",
        "DE11 System Trace Audit Number",
        FieldKind::FixedDigits,
        6,
    ),
    field(
        12,
        "local_transaction_time",
        "DE12 Local Transaction Time",
        FieldKind::FixedDigits,
        6,
    ),
    field(
        13,
        "local_transaction_date",
        "DE13 Local Transaction Date",
        FieldKind::FixedDigits,
        4,
    ),
    field(
        14,
        "expiration_date",
        "DE14 Expiration Date",
        FieldKind::FixedDigits,
        4,
    ),
    field(
        18,
        "merchant_type",
        "DE18 Merchant Type",
        FieldKind::FixedDigits,
        4,
    ),
    field(
        22,
        "pos_entry_mode",
        "DE22 POS Entry Mode",
        FieldKind::FixedDigits,
        3,
    ),
    field(
        23,
        "card_sequence_number",
        "DE23 Card Sequence Number",
        FieldKind::FixedDigits,
        3,
    ),
    field(
        25,
        "pos_condition_code",
        "DE25 POS Condition Code",
        FieldKind::FixedDigits,
        2,
    ),
    field(
        32,
        "acquiring_institution_id",
        "DE32 Acquiring Institution ID",
        FieldKind::LlDigits,
        11,
    ),
    field(
        35,
        "track_2_data",
        "DE35 Track 2 Data",
        FieldKind::LlAscii,
        37,
    ),
    field(
        37,
        "retrieval_reference_number",
        "DE37 Retrieval Reference Number",
        FieldKind::FixedAscii,
        12,
    ),
    field(
        38,
        "authorization_id_response",
        "DE38 Authorization ID Response",
        FieldKind::FixedAscii,
        6,
    ),
    field(
        39,
        "response_code",
        "DE39 Response Code",
        FieldKind::FixedAscii,
        2,
    ),
    field(
        41,
        "terminal_id",
        "DE41 Terminal ID",
        FieldKind::FixedAscii,
        8,
    ),
    field(
        42,
        "card_acceptor_id",
        "DE42 Card Acceptor ID",
        FieldKind::FixedAscii,
        15,
    ),
    field(
        43,
        "card_acceptor_name_location",
        "DE43 Card Acceptor Name And Location",
        FieldKind::FixedAscii,
        40,
    ),
    field(
        49,
        "currency",
        "DE49 Transaction Currency",
        FieldKind::FixedDigits,
        3,
    ),
    typed_field(
        52,
        "pin_data",
        "DE52 PIN Data",
        FieldKind::FixedBlob,
        8,
        DocumentKind::Blob,
    ),
    field(
        53,
        "security_control_information",
        "DE53 Security Control Information",
        FieldKind::FixedDigits,
        16,
    ),
    field(
        54,
        "additional_amounts",
        "DE54 Additional Amounts",
        FieldKind::LllAscii,
        120,
    ),
    typed_field(
        55,
        "icc_data",
        "DE55 ICC Data",
        FieldKind::LllBlob,
        255,
        DocumentKind::Blob,
    ),
    field(
        60,
        "reserved_private_60",
        "DE60 Reserved Private",
        FieldKind::LllAscii,
        999,
    ),
    field(
        61,
        "reserved_private_61",
        "DE61 Reserved Private",
        FieldKind::LllAscii,
        999,
    ),
    field(
        62,
        "reserved_private_62",
        "DE62 Reserved Private",
        FieldKind::LllAscii,
        999,
    ),
    field(
        63,
        "reserved_private_63",
        "DE63 Reserved Private",
        FieldKind::LllAscii,
        999,
    ),
    typed_field(
        64,
        "message_authentication_code",
        "DE64 Message Authentication Code",
        FieldKind::FixedBlob,
        8,
        DocumentKind::Blob,
    ),
    field(
        70,
        "network_management_code",
        "DE70 Network Management Code",
        FieldKind::FixedDigits,
        3,
    ),
    field(
        90,
        "original_data_elements",
        "DE90 Original Data Elements",
        FieldKind::FixedDigits,
        42,
    ),
    field(
        100,
        "receiving_institution_id",
        "DE100 Receiving Institution ID",
        FieldKind::LlDigits,
        11,
    ),
    field(
        102,
        "account_id_1",
        "DE102 Account ID 1",
        FieldKind::LlAscii,
        28,
    ),
    field(
        103,
        "account_id_2",
        "DE103 Account ID 2",
        FieldKind::LlAscii,
        28,
    ),
    typed_field(
        128,
        "message_authentication_code_2",
        "DE128 Message Authentication Code",
        FieldKind::FixedBlob,
        8,
        DocumentKind::Blob,
    ),
];

struct Iso8583DenoAscii;

impl Iso8583DenoAscii {
    fn frame(buffer: &[u8]) -> Result<FrameResult, String> {
        if buffer.len() < HEADER_BYTES {
            return Ok(FrameResult::NeedMore(None));
        }
        let payload = usize::from(u16::from_be_bytes([buffer[0], buffer[1]]));
        let frame_bytes = HEADER_BYTES + payload;
        if frame_bytes > MAX_FRAME_BYTES {
            return Err("frame exceeds profile maximum length".to_owned());
        }
        if buffer.len() < frame_bytes {
            Ok(FrameResult::NeedMore(None))
        } else {
            Ok(FrameResult::Complete(frame_bytes as u64))
        }
    }

    fn decode(frame: &[u8]) -> Result<String, String> {
        if frame.len() < HEADER_BYTES {
            return Err("frame is shorter than its length header".to_owned());
        }
        let declared = usize::from(u16::from_be_bytes([frame[0], frame[1]]));
        if declared != frame.len() - HEADER_BYTES {
            return Err("frame length header does not match the received bytes".to_owned());
        }
        serde_json::to_string(&decode_message(&frame[HEADER_BYTES..])?)
            .map_err(|error| error.to_string())
    }

    fn encode(document_json: &str) -> Result<Vec<u8>, String> {
        let document = parse_document(document_json)?;
        let message = encode_message(&document)?;
        if message.len() + HEADER_BYTES > MAX_FRAME_BYTES {
            return Err("encoded ISO 8583 message is too large".to_owned());
        }
        let payload =
            u16::try_from(message.len()).map_err(|_| "encoded payload exceeds u16".to_owned())?;
        let mut frame = Vec::with_capacity(message.len() + HEADER_BYTES);
        frame.extend_from_slice(&payload.to_be_bytes());
        frame.extend_from_slice(&message);
        Ok(frame)
    }

    fn display(document_json: &str) -> Result<String, String> {
        let document = parse_document(document_json)?;
        validate_document_keys(&document)?;
        let mut rows = String::new();
        if let Some(value) = document.get("message_type") {
            rows.push_str(&row("MTI", &printable_value(value, DocumentKind::String)?));
        }
        for spec in FIELD_SPECS {
            if let Some(value) = document.get(spec.name) {
                rows.push_str(&row(
                    spec.label,
                    &printable_value(value, spec.document_kind)?,
                ));
            }
        }
        Ok(format!(
            "<section class=\"protocol-document\"><h3>ISO 8583:1987 Message</h3><table><tbody>{rows}</tbody></table></section>"
        ))
    }
}

impl Guest for Iso8583DenoAscii {
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
        _original_input: Vec<u8>,
        document_json: String,
    ) -> Result<Vec<u8>, PackageError> {
        Self::encode(&document_json).map_err(package_error("BODY_ENCODE_FAILED"))
    }
    fn downstream_encode(
        _original_input: Vec<u8>,
        document_json: String,
    ) -> Result<Vec<u8>, PackageError> {
        Self::encode(&document_json).map_err(package_error("BODY_ENCODE_FAILED"))
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

fn parse_document(document_json: &str) -> Result<Map<String, Value>, String> {
    serde_json::from_str::<Value>(document_json)
        .map_err(|error| error.to_string())?
        .as_object()
        .cloned()
        .ok_or_else(|| "document must be an object".to_owned())
}

fn decode_message(message: &[u8]) -> Result<Map<String, Value>, String> {
    if message.len() < 12 {
        return Err("ISO 8583 message must contain MTI and primary bitmap".to_owned());
    }
    let mut document = Map::new();
    document.insert(
        "message_type".to_owned(),
        Value::String(read_digits(message, 0, 4, "MTI")?.to_owned()),
    );
    let mut bitmap_bytes = 8;
    if bitmap_has(&message[4..12], 1) {
        if message.len() < 20 {
            return Err("secondary bitmap indicator is set but missing".to_owned());
        }
        bitmap_bytes = 16;
        if bitmap_has(&message[4..20], 65) {
            return Err("tertiary bitmap is outside this profile".to_owned());
        }
    }
    let bitmap = &message[4..4 + bitmap_bytes];
    let mut offset = 4 + bitmap_bytes;
    let last_field = if bitmap_bytes == 16 { 128 } else { 64 };
    for number in 2..=last_field {
        if !bitmap_has(bitmap, number) {
            continue;
        }
        let spec = FIELD_SPECS
            .iter()
            .find(|spec| usize::from(spec.number) == number)
            .ok_or_else(|| format!("DE{number} is not supported by this example profile"))?;
        let (value, next_offset) = read_field(message, offset, spec)?;
        document.insert(spec.name.to_owned(), value);
        offset = next_offset;
    }
    if offset != message.len() {
        return Err("message has trailing bytes not described by bitmap".to_owned());
    }
    Ok(document)
}

fn encode_message(document: &Map<String, Value>) -> Result<Vec<u8>, String> {
    validate_document_keys(document)?;
    let mti = document
        .get("message_type")
        .and_then(Value::as_str)
        .ok_or_else(|| "message_type must be a string".to_owned())?;
    require_digits(mti, "message_type")?;
    if mti.len() != 4 {
        return Err("message_type must contain exactly 4 digits".to_owned());
    }
    let present = FIELD_SPECS
        .iter()
        .filter(|spec| document.contains_key(spec.name))
        .collect::<Vec<_>>();
    let secondary = present.iter().any(|spec| spec.number > 64);
    let mut bitmap = vec![0_u8; if secondary { 16 } else { 8 }];
    if secondary {
        bitmap_set(&mut bitmap, 1)?;
    }
    let mut fields = Vec::new();
    for spec in present {
        bitmap_set(&mut bitmap, usize::from(spec.number))?;
        fields.extend_from_slice(&encode_field(
            document.get(spec.name).expect("present field"),
            spec,
        )?);
    }
    let mut message = Vec::with_capacity(4 + bitmap.len() + fields.len());
    message.extend_from_slice(mti.as_bytes());
    message.extend_from_slice(&bitmap);
    message.extend_from_slice(&fields);
    Ok(message)
}

fn read_field(
    message: &[u8],
    mut offset: usize,
    spec: &FieldSpec,
) -> Result<(Value, usize), String> {
    let mut length = spec.length;
    let prefix = match spec.kind {
        FieldKind::LlAscii | FieldKind::LlDigits => 2,
        FieldKind::LllAscii | FieldKind::LllBlob => 3,
        _ => 0,
    };
    if prefix > 0 {
        length = read_digits(message, offset, prefix, &format!("{} length", spec.name))?
            .parse::<usize>()
            .map_err(|error| error.to_string())?;
        offset += prefix;
        if length > spec.length {
            return Err(format!("{} length exceeds profile maximum", spec.name));
        }
    }
    let bytes = read_bytes(message, offset, length, spec.name)?;
    let value = match spec.document_kind {
        DocumentKind::Blob => Value::String(BASE64.encode(bytes)),
        DocumentKind::String => {
            let text = read_ascii(bytes, spec.name)?;
            if matches!(spec.kind, FieldKind::FixedDigits | FieldKind::LlDigits) {
                require_digits(text, spec.name)?;
            }
            Value::String(text.to_owned())
        }
        DocumentKind::Integer => {
            let text = read_ascii(bytes, spec.name)?;
            require_digits(text, spec.name)?;
            let value = text
                .parse::<u64>()
                .map_err(|error| format!("{}: {error}", spec.name))?;
            Value::Number(Number::from(value))
        }
    };
    Ok((value, offset + length))
}

fn encode_field(value: &Value, spec: &FieldSpec) -> Result<Vec<u8>, String> {
    let payload = match spec.document_kind {
        DocumentKind::Blob => BASE64
            .decode(
                value
                    .as_str()
                    .ok_or_else(|| format!("{} must be a Base64 string", spec.name))?,
            )
            .map_err(|error| format!("{} contains invalid Base64: {error}", spec.name))?,
        DocumentKind::String => value
            .as_str()
            .ok_or_else(|| format!("{} must be a string", spec.name))?
            .as_bytes()
            .to_vec(),
        DocumentKind::Integer => {
            let digits = non_negative_integer_digits(value)
                .ok_or_else(|| format!("{} must be a non-negative integer", spec.name))?;
            format!("{digits:0>width$}", width = spec.length).into_bytes()
        }
    };
    if spec.document_kind != DocumentKind::Blob && !payload.is_ascii() {
        return Err(format!("{} must contain ASCII characters only", spec.name));
    }
    if matches!(
        spec.kind,
        FieldKind::FixedDigits | FieldKind::LlDigits | FieldKind::FixedAmount
    ) {
        require_digits(
            std::str::from_utf8(&payload).map_err(|_| format!("{} must be ASCII", spec.name))?,
            spec.name,
        )?;
    }
    prefix_variable(payload, spec)
}

fn prefix_variable(payload: Vec<u8>, spec: &FieldSpec) -> Result<Vec<u8>, String> {
    if payload.len() > spec.length {
        return Err(format!("{} length exceeds profile maximum", spec.name));
    }
    let prefix = match spec.kind {
        FieldKind::LlAscii | FieldKind::LlDigits => Some(format!("{:02}", payload.len())),
        FieldKind::LllAscii | FieldKind::LllBlob => Some(format!("{:03}", payload.len())),
        _ => None,
    };
    if prefix.is_none() && payload.len() != spec.length {
        return Err(format!(
            "{} must contain exactly {} bytes",
            spec.name, spec.length
        ));
    }
    let mut encoded = prefix.map_or_else(Vec::new, String::into_bytes);
    encoded.extend_from_slice(&payload);
    Ok(encoded)
}

fn validate_document_keys(document: &Map<String, Value>) -> Result<(), String> {
    for name in document.keys() {
        if name != "message_type" && !FIELD_SPECS.iter().any(|spec| spec.name == name) {
            return Err(format!("unknown Document field: {name}"));
        }
    }
    Ok(())
}

fn bitmap_has(bitmap: &[u8], field: usize) -> bool {
    let index = field - 1;
    bitmap
        .get(index / 8)
        .is_some_and(|byte| byte & (1 << (7 - index % 8)) != 0)
}

fn bitmap_set(bitmap: &mut [u8], field: usize) -> Result<(), String> {
    let index = field - 1;
    let byte = bitmap
        .get_mut(index / 8)
        .ok_or_else(|| format!("DE{field} requires a secondary bitmap"))?;
    *byte |= 1 << (7 - index % 8);
    Ok(())
}

fn read_digits<'a>(
    bytes: &'a [u8],
    offset: usize,
    length: usize,
    name: &str,
) -> Result<&'a str, String> {
    let value = read_ascii(read_bytes(bytes, offset, length, name)?, name)?;
    require_digits(value, name)?;
    Ok(value)
}

fn require_digits(value: &str, name: &str) -> Result<(), String> {
    if value.is_empty() || !value.bytes().all(|byte| byte.is_ascii_digit()) {
        Err(format!("{name} must contain ASCII digits only"))
    } else {
        Ok(())
    }
}

fn read_bytes<'a>(
    bytes: &'a [u8],
    offset: usize,
    length: usize,
    name: &str,
) -> Result<&'a [u8], String> {
    bytes
        .get(offset..offset + length)
        .ok_or_else(|| format!("{name} exceeds message boundary"))
}

fn read_ascii<'a>(bytes: &'a [u8], name: &str) -> Result<&'a str, String> {
    if !bytes.is_ascii() {
        return Err(format!("{name} must contain ASCII bytes only"));
    }
    std::str::from_utf8(bytes).map_err(|_| format!("{name} must contain ASCII bytes only"))
}

fn printable_value(value: &Value, kind: DocumentKind) -> Result<String, String> {
    match kind {
        DocumentKind::Blob => Ok(format!(
            "[binary {} bytes]",
            BASE64
                .decode(
                    value
                        .as_str()
                        .ok_or_else(|| "blob must be a Base64 string".to_owned())?
                )
                .map_err(|error| error.to_string())?
                .len()
        )),
        DocumentKind::String => Ok(value
            .as_str()
            .ok_or_else(|| "value must be a string".to_owned())?
            .to_owned()),
        DocumentKind::Integer => {
            non_negative_integer_digits(value).ok_or_else(|| "value must be an integer".to_owned())
        }
    }
}

fn non_negative_integer_digits(value: &Value) -> Option<String> {
    let number = value.as_f64()?;
    (number.is_finite() && number >= 0.0 && number.fract() == 0.0).then(|| format!("{number:.0}"))
}

fn row(label: &str, value: &str) -> String {
    format!(
        "<tr><th>{}</th><td>{}</td></tr>",
        escape_html(label),
        escape_html(value)
    )
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
export!(Iso8583DenoAscii);

#[cfg(test)]
mod tests {
    use super::*;

    const REQUEST_HEX: &str = "0039303230303220000000808000303030303030303030303030303031303030303831333134333035393132333435365445524d30303031333932";

    fn request() -> Vec<u8> {
        REQUEST_HEX
            .as_bytes()
            .chunks_exact(2)
            .map(|pair| u8::from_str_radix(std::str::from_utf8(pair).unwrap(), 16).unwrap())
            .collect()
    }

    #[test]
    fn supported_profile_roundtrips() {
        let frame = request();
        let document = Iso8583DenoAscii::decode(&frame).unwrap();
        assert_eq!(
            serde_json::from_str::<Value>(&document).unwrap()["message_type"],
            "0200"
        );
        assert_eq!(
            serde_json::from_str::<Value>(&document).unwrap()["amount"],
            1000
        );
        assert_eq!(Iso8583DenoAscii::encode(&document).unwrap(), frame);
    }

    #[test]
    fn frame_and_display_preserve_boundaries() {
        let frame = request();
        assert!(
            matches!(Iso8583DenoAscii::frame(&frame), Ok(FrameResult::Complete(value)) if value == frame.len() as u64)
        );
        let html = Iso8583DenoAscii::display(r#"{"message_type":"<script>"}"#).unwrap();
        assert!(!html.contains("<script>"));
        assert!(html.contains("&lt;script&gt;"));
    }

    #[test]
    fn display_and_encode_accept_host_serialized_integral_numbers() {
        let document = Iso8583DenoAscii::decode(&request()).unwrap();
        let mut host_document = serde_json::from_str::<Value>(&document).unwrap();
        host_document["amount"] = serde_json::json!(1000.0);
        let host_document = serde_json::to_string(&host_document).unwrap();

        let html = Iso8583DenoAscii::display(&host_document).unwrap();
        assert!(html.contains("<td>1000</td>"));
        assert_eq!(Iso8583DenoAscii::encode(&host_document).unwrap(), request());

        assert!(Iso8583DenoAscii::display(r#"{"amount":1000.5}"#).is_err());
        assert!(Iso8583DenoAscii::encode(r#"{"message_type":"0200","amount":-1.0}"#).is_err());
    }
}
