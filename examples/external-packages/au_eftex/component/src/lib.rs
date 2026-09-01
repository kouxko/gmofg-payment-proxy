mod crypto;
mod iso8583;

use std::{env, fs};

use serde_json::{Map, Value};

wit_bindgen::generate!({
    path: "../../../../src-tauri/crates/package-runtime/wit",
    world: "socket-package",
});

const _: &str =
    include_str!("../../../../../src-tauri/crates/package-runtime/wit/protocol-package.wit");

const HEADER_BYTES: usize = 39;
const MAX_FRAME_BYTES: usize = 65_535;
const IV_MASK: [u8; 8] = [0x01, 0x23, 0x45, 0x67, 0x89, 0xab, 0xcd, 0xef];

#[derive(Clone, Copy, PartialEq)]
enum Direction {
    Upstream,
    Downstream,
}

#[derive(Clone, Copy)]
enum Prefix {
    None,
    Body,
    Total,
}

struct ClearFrame {
    header: [u8; HEADER_BYTES],
    message: Vec<u8>,
    prefix: Prefix,
}

struct AuEftex;

impl AuEftex {
    fn frame(direction: Direction, buffer: &[u8]) -> Result<FrameResult, String> {
        if buffer.len() < HEADER_BYTES {
            return Ok(FrameResult::NeedMore(Some(HEADER_BYTES as u64)));
        }
        if buffer.first() != Some(&b'T') && buffer.len() < HEADER_BYTES + 2 {
            return Ok(FrameResult::NeedMore(Some((HEADER_BYTES + 2) as u64)));
        }
        let offset = header_offset(buffer)?;
        if offset == 2 {
            let total = prefixed_frame_size([buffer[0], buffer[1]])?;
            return if buffer.len() < total {
                Ok(FrameResult::NeedMore(Some(total as u64)))
            } else {
                Ok(FrameResult::Complete(total as u64))
            };
        }
        let header: [u8; HEADER_BYTES] = buffer[..HEADER_BYTES].try_into().unwrap();
        let ksn = parse_header(&header)?;
        let encrypted = &buffer[HEADER_BYTES..];
        if encrypted.len() < 12 {
            return Ok(FrameResult::NeedMore(None));
        }
        let clear = crypto::ofb(data_key(direction, ksn)?, data_iv(&header)?, encrypted);
        let clear_bytes = match iso8583::message_length(&clear) {
            Ok(Some(value)) => value,
            Ok(None) => return Ok(FrameResult::NeedMore(None)),
            Err(error) => {
                return Err(direction_error(
                    direction,
                    ksn,
                    data_iv(&header)?,
                    encrypted,
                    error,
                )?);
            }
        };
        let encrypted_bytes = clear_bytes + (8 - clear_bytes % 8);
        let total = HEADER_BYTES + encrypted_bytes;
        if total > MAX_FRAME_BYTES {
            return Err("AU EFTEX frame exceeds 65,535 bytes".into());
        }
        if buffer.len() < total {
            Ok(FrameResult::NeedMore(Some(total as u64)))
        } else {
            Ok(FrameResult::Complete(total as u64))
        }
    }

    fn decode(direction: Direction, input: &[u8]) -> Result<String, String> {
        let clear = decrypt(direction, input)?;
        serde_json::to_string(&iso8583::decode(&clear.message)?).map_err(|error| error.to_string())
    }

    fn encode(
        direction: Direction,
        original_input: &[u8],
        document_json: &str,
    ) -> Result<Vec<u8>, String> {
        let clear = decrypt(direction, original_input)?;
        let document = serde_json::from_str::<Map<String, Value>>(document_json)
            .map_err(|error| error.to_string())?;
        let message = iso8583::encode(&document)?;
        if message != clear.message {
            let original = iso8583::decode(&clear.message)?;
            if iso8583::contains_mac(&original) || iso8583::contains_mac(&document) {
                return Err(
                    "ISO 8583 fields changed but replacement MAC validation is unavailable".into(),
                );
            }
        }
        encrypt(direction, clear.header, &message, clear.prefix)
    }

    fn display(direction: Direction, document_json: &str) -> Result<String, String> {
        let document = serde_json::from_str::<Map<String, Value>>(document_json)
            .map_err(|error| error.to_string())?;
        iso8583::encode(&document)?;
        let label = if direction == Direction::Upstream {
            "Upstream"
        } else {
            "Downstream"
        };
        let mut rows = format!("<tr><th>Direction</th><td>{label}</td></tr>");
        for field in iso8583::FIELDS {
            let Some(value) = document.get(field.name) else {
                continue;
            };
            let displayed = display_value(field.name, value)?;
            rows.push_str(&format!(
                "<tr><th>DE{} {}</th><td>{}</td></tr>",
                if field.number == 0 {
                    "MTI".into()
                } else {
                    field.number.to_string()
                },
                escape(field.name),
                escape(&displayed)
            ));
        }
        Ok(format!(
            "<section class=\"protocol-document\"><h3>AU EFTEX ISO 8583</h3><table><tbody>{rows}</tbody></table></section>"
        ))
    }
}

impl Guest for AuEftex {
    fn upstream_frame(buffer: Vec<u8>) -> Result<FrameResult, PackageError> {
        Self::frame(Direction::Upstream, &buffer)
            .map_err(package_error("PROTOCOL_PACKAGE_INVALID"))
    }
    fn downstream_frame(buffer: Vec<u8>) -> Result<FrameResult, PackageError> {
        Self::frame(Direction::Downstream, &buffer)
            .map_err(package_error("PROTOCOL_PACKAGE_INVALID"))
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
        Self::encode(Direction::Upstream, &original_input, &document_json)
            .map_err(package_error("BODY_ENCODE_FAILED"))
    }
    fn downstream_encode(
        original_input: Vec<u8>,
        document_json: String,
    ) -> Result<Vec<u8>, PackageError> {
        Self::encode(Direction::Downstream, &original_input, &document_json)
            .map_err(package_error("BODY_ENCODE_FAILED"))
    }
    fn upstream_display(document_json: String) -> Result<String, PackageError> {
        Self::display(Direction::Upstream, &document_json)
            .map_err(package_error("INTERNAL_ERROR"))
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

fn load_bdk() -> Result<[u8; 16], String> {
    let file = env::var("AU_EFTEX_BDK_FILE").ok();
    let inline = env::var("AU_EFTEX_BDK_HEX").ok();
    let value = match (file, inline) {
        (Some(path), None) => {
            fs::read_to_string(path).map_err(|_| "unable to read AU_EFTEX_BDK_FILE".to_owned())?
        }
        (None, Some(value)) => value,
        _ => return Err("configure exactly one of AU_EFTEX_BDK_FILE or AU_EFTEX_BDK_HEX".into()),
    };
    decode_hex_16(value.trim(), "AU_EFTEX_BDK")
}

fn decode_hex_16(value: &str, name: &str) -> Result<[u8; 16], String> {
    if value.len() != 32 || !value.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        return Err(format!("{name} must contain exactly 16 hexadecimal bytes"));
    }
    let mut output = [0; 16];
    for (index, byte) in output.iter_mut().enumerate() {
        *byte = u8::from_str_radix(&value[index * 2..index * 2 + 2], 16).unwrap();
    }
    Ok(output)
}

fn data_key(direction: Direction, ksn: [u8; 10]) -> Result<[u8; 16], String> {
    let bdk = load_bdk()?;
    let transaction = crypto::derive_transaction_key(crypto::derive_ipek(bdk, ksn), ksn)?;
    Ok(crypto::derive_data_key(
        transaction,
        direction == Direction::Upstream,
    ))
}

fn header_offset(input: &[u8]) -> Result<usize, String> {
    if input.first() == Some(&b'T') {
        return Ok(0);
    }
    if input.len() < HEADER_BYTES + 2 {
        return Ok(2);
    }
    parse_header(input[2..2 + HEADER_BYTES].try_into().unwrap())?;
    Ok(2)
}

fn parse_header(header: &[u8; HEADER_BYTES]) -> Result<[u8; 10], String> {
    if header[0] != b'T' {
        return Err("AU EFTEX header must start with T".into());
    }
    for (offset, tag, length) in [
        (1, [0xdf, 0x00], 1),
        (5, [0xdf, 0x01], 8),
        (16, [0xdf, 0x02], 6),
        (25, [0xdf, 0x03], 10),
    ] {
        if header[offset..offset + 2] != tag || header[offset + 2] != length {
            return Err("AU EFTEX header TLV layout is invalid".into());
        }
    }
    if header[38] != b'B' {
        return Err("AU EFTEX encoding indicator must be B".into());
    }
    Ok(header[28..38].try_into().unwrap())
}

fn data_iv(header: &[u8; HEADER_BYTES]) -> Result<[u8; 8], String> {
    parse_header(header)?;
    let stan = std::str::from_utf8(&header[19..25])
        .map_err(|_| "AU EFTEX STAN must contain ASCII digits".to_owned())?;
    if !stan.bytes().all(|byte| byte.is_ascii_digit()) {
        return Err("AU EFTEX STAN must contain ASCII digits".into());
    }
    let digits = format!("{:016}", stan.parse::<u64>().unwrap());
    let mut iv = [0; 8];
    for index in 0..8 {
        iv[index] =
            u8::from_str_radix(&digits[index * 2..index * 2 + 2], 16).unwrap() ^ IV_MASK[index];
    }
    Ok(iv)
}

fn prefixed_frame_size(prefix: [u8; 2]) -> Result<usize, String> {
    let declared = usize::from(u16::from_be_bytes(prefix));
    let candidates = [declared + 2, declared];
    let valid: Vec<_> = candidates
        .into_iter()
        .filter(|total| {
            *total >= 2 + HEADER_BYTES + 16
                && *total <= MAX_FRAME_BYTES
                && (*total - 2 - HEADER_BYTES).is_multiple_of(8)
        })
        .collect();
    if valid.len() != 1 {
        return Err("AU EFTEX length prefix does not describe a block-aligned frame".into());
    }
    Ok(valid[0])
}

fn decrypt(direction: Direction, input: &[u8]) -> Result<ClearFrame, String> {
    if input.len() < HEADER_BYTES + 8 || input.len() > MAX_FRAME_BYTES {
        return Err("invalid AU EFTEX frame length".into());
    }
    let offset = header_offset(input)?;
    let header: [u8; HEADER_BYTES] = input[offset..offset + HEADER_BYTES]
        .try_into()
        .map_err(|_| "invalid AU EFTEX frame length")?;
    let ksn = parse_header(&header)?;
    let encrypted = &input[offset + HEADER_BYTES..];
    if !encrypted.len().is_multiple_of(8) {
        return Err("AU EFTEX encrypted message must be block aligned".into());
    }
    let prefix = if offset == 0 {
        Prefix::None
    } else {
        let declared = usize::from(u16::from_be_bytes([input[0], input[1]]));
        if declared == input.len() - 2 {
            Prefix::Body
        } else if declared == input.len() {
            Prefix::Total
        } else {
            return Err("AU EFTEX length prefix does not match the complete frame".into());
        }
    };
    let padded = crypto::ofb(data_key(direction, ksn)?, data_iv(&header)?, encrypted);
    if !plausible_mti(&padded) {
        return Err(direction_error(
            direction,
            ksn,
            data_iv(&header)?,
            encrypted,
            "AU EFTEX decrypted MTI is invalid".into(),
        )?);
    }
    let message = unpad(&padded)?;
    if iso8583::message_length(&message)? != Some(message.len()) {
        return Err("AU EFTEX message length does not match its bitmap fields".into());
    }
    Ok(ClearFrame {
        header,
        message,
        prefix,
    })
}

fn encrypt(
    direction: Direction,
    header: [u8; HEADER_BYTES],
    message: &[u8],
    prefix: Prefix,
) -> Result<Vec<u8>, String> {
    let ksn = parse_header(&header)?;
    if iso8583::message_length(message)? != Some(message.len()) {
        return Err("AU EFTEX message length does not match its bitmap fields".into());
    }
    let mut body = header.to_vec();
    body.extend(crypto::ofb(
        data_key(direction, ksn)?,
        data_iv(&header)?,
        &pad(message),
    ));
    let mut output = match prefix {
        Prefix::None => body,
        Prefix::Body | Prefix::Total => {
            let declared = if matches!(prefix, Prefix::Body) {
                body.len()
            } else {
                body.len() + 2
            };
            let mut result = u16::try_from(declared)
                .map_err(|_| "AU EFTEX length prefix exceeds 65,535 bytes")?
                .to_be_bytes()
                .to_vec();
            result.extend(body);
            result
        }
    };
    if output.len() > MAX_FRAME_BYTES {
        return Err("AU EFTEX frame exceeds 65,535 bytes".into());
    }
    Ok(std::mem::take(&mut output))
}

fn pad(message: &[u8]) -> Vec<u8> {
    let count = 8 - message.len() % 8;
    let mut out = message.to_vec();
    out.extend(std::iter::repeat_n(0xff, count - 1));
    out.push((count - 1) as u8);
    out
}
fn unpad(value: &[u8]) -> Result<Vec<u8>, String> {
    if value.is_empty() || !value.len().is_multiple_of(8) {
        return Err("invalid EFTEX padding length".into());
    }
    let fill = *value.last().unwrap() as usize;
    if fill > 7
        || value.len() < fill + 1
        || value[value.len() - fill - 1..value.len() - 1]
            .iter()
            .any(|byte| *byte != 0xff)
    {
        return Err("invalid EFTEX padding fill bytes".into());
    }
    Ok(value[..value.len() - fill - 1].to_vec())
}
fn plausible_mti(value: &[u8]) -> bool {
    value.len() >= 4 && value[..4].iter().all(u8::is_ascii_digit)
}

fn direction_error(
    direction: Direction,
    ksn: [u8; 10],
    iv: [u8; 8],
    encrypted: &[u8],
    cause: String,
) -> Result<String, String> {
    let opposite = if direction == Direction::Upstream {
        Direction::Downstream
    } else {
        Direction::Upstream
    };
    let clear = crypto::ofb(
        data_key(opposite, ksn)?,
        iv,
        &encrypted[..encrypted.len().min(4)],
    );
    if plausible_mti(&clear) {
        Ok("AU EFTEX data key direction does not match the hook direction".into())
    } else if plausible_mti(encrypted) {
        Ok("AU EFTEX payload is unexpectedly not encrypted".into())
    } else {
        Ok(cause)
    }
}

fn display_value(name: &str, value: &Value) -> Result<String, String> {
    let object = value
        .as_object()
        .ok_or_else(|| format!("{name} must be a tagged value"))?;
    if object.get("type").and_then(Value::as_str) == Some("blob") {
        let encoded = object
            .get("value_base64")
            .and_then(Value::as_str)
            .ok_or_else(|| format!("{name} contains invalid Base64"))?;
        let bytes = base64::Engine::decode(&base64::engine::general_purpose::STANDARD, encoded)
            .map_err(|_| format!("{name} contains invalid Base64"))?;
        return Ok(format!("[redacted blob: {} bytes]", bytes.len()));
    }
    let text = object
        .get("value")
        .and_then(Value::as_str)
        .ok_or_else(|| format!("{name} must be a tagged value"))?;
    let sensitive = matches!(
        name,
        "primary_account_number"
            | "track_2_data"
            | "pin_data"
            | "security_control_information"
            | "message_authentication_code"
            | "message_authentication_code_extended"
            | "additional_private"
            | "reserved_private"
            | "receipt_data"
            | "display_data"
    );
    Ok(if sensitive {
        format!("[redacted: {} chars]", text.len())
    } else {
        text.into()
    })
}
fn escape(value: &str) -> String {
    value
        .replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&#39;")
}

export!(AuEftex);

#[cfg(test)]
mod tests {
    use super::*;
    const BDK: &str = "0123456789ABCDEFFEDCBA9876543210";
    fn header() -> [u8; 39] {
        let bytes=b"T\xdf\x00\x012\xdf\x01\x0812345678\xdf\x02\x06000001\xdf\x03\x0a\xff\xff\x98\x76\x54\x32\x10\xe0\x00\x08B";
        *bytes
    }
    fn message() -> Vec<u8> {
        let mut bitmap = [0u8; 8];
        bitmap[0] |= 0x20;
        bitmap[7] |= 1;
        [
            b"1200".as_slice(),
            &bitmap,
            b"000000",
            &[1, 2, 3, 4, 5, 6, 7, 8],
        ]
        .concat()
    }
    #[test]
    fn golden_upstream_round_trip() {
        unsafe { env::set_var("AU_EFTEX_BDK_HEX", BDK) };
        let frame = encrypt(Direction::Upstream, header(), &message(), Prefix::None).unwrap();
        assert_eq!(
            hex(&frame),
            "54df000132df01083132333435363738df0206303030303031df030affff9876543210e00008427b758dda6a29d38b8020b31687b21d636dbc15e6f3a17cdee8a868124d4c8f84"
        );
        let json = AuEftex::decode(Direction::Upstream, &frame).unwrap();
        assert_eq!(
            AuEftex::encode(Direction::Upstream, &frame, &json).unwrap(),
            frame
        );
    }
    #[test]
    fn modified_mac_message_fails_closed() {
        unsafe { env::set_var("AU_EFTEX_BDK_HEX", BDK) };
        let frame = encrypt(Direction::Upstream, header(), &message(), Prefix::None).unwrap();
        let mut document = iso8583::decode(&message()).unwrap();
        document.insert(
            "processing_code".into(),
            serde_json::json!({"type":"string","value":"990000"}),
        );
        assert!(
            AuEftex::encode(
                Direction::Upstream,
                &frame,
                &serde_json::to_string(&document).unwrap()
            )
            .unwrap_err()
            .contains("MAC")
        );
    }

    #[test]
    fn downstream_and_length_prefixed_golden_round_trip() {
        unsafe { env::set_var("AU_EFTEX_BDK_HEX", BDK) };
        let mut bitmap = [0u8; 8];
        bitmap[0] |= 0x20;
        let message = [b"1210".as_slice(), &bitmap, b"000000"].concat();
        let body = encrypt(Direction::Downstream, header(), &message, Prefix::None).unwrap();
        assert_eq!(
            hex(&body),
            "54df000132df01083132333435363738df0206303030303031df030affff9876543210e000084247737e0317a4310697a84e728f754c84798309ef10edd18e"
        );

        let frame = encrypt(Direction::Downstream, header(), &message, Prefix::Body).unwrap();
        assert!(matches!(
            AuEftex::frame(Direction::Downstream, &frame[..frame.len() - 1]).unwrap(),
            FrameResult::NeedMore(Some(expected)) if expected == frame.len() as u64
        ));
        assert!(matches!(
            AuEftex::frame(Direction::Downstream, &frame).unwrap(),
            FrameResult::Complete(consumed) if consumed == frame.len() as u64
        ));
        let json = AuEftex::decode(Direction::Downstream, &frame).unwrap();
        assert_eq!(
            AuEftex::encode(Direction::Downstream, &frame, &json).unwrap(),
            frame
        );
    }
    fn hex(value: &[u8]) -> String {
        value.iter().map(|byte| format!("{byte:02x}")).collect()
    }
}
