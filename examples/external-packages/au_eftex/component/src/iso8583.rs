use base64::{Engine as _, engine::general_purpose::STANDARD};
use serde_json::{Map, Value, json};

#[derive(Clone, Copy, PartialEq)]
enum Kind {
    Fixed,
    Amount,
    Blob,
    Ll,
    Lll,
    LllBlob,
    LlllBlob,
}

#[derive(Clone, Copy)]
pub struct Field {
    pub number: usize,
    pub name: &'static str,
    kind: Kind,
    max: usize,
}

pub const FIELDS: &[Field] = &[
    Field {
        number: 0,
        name: "message_type",
        kind: Kind::Fixed,
        max: 4,
    },
    f(2, "primary_account_number", Kind::Ll, 19),
    f(3, "processing_code", Kind::Fixed, 6),
    f(4, "amount", Kind::Amount, 12),
    f(7, "transmission_time", Kind::Fixed, 10),
    f(11, "stan", Kind::Fixed, 6),
    f(12, "local_transaction_time", Kind::Fixed, 12),
    f(14, "expiration_date", Kind::Fixed, 4),
    f(22, "pos_data_code", Kind::Fixed, 15),
    f(23, "card_sequence_number", Kind::Fixed, 3),
    f(24, "function_code", Kind::Fixed, 3),
    f(25, "message_reason_code", Kind::Fixed, 4),
    f(28, "reconciliation_date", Kind::Fixed, 6),
    f(29, "reconciliation_indicator", Kind::Fixed, 3),
    f(35, "track_2_data", Kind::Ll, 37),
    f(37, "retrieval_reference_number", Kind::Fixed, 12),
    f(38, "approval_code", Kind::Fixed, 6),
    f(39, "action_code", Kind::Fixed, 3),
    f(40, "service_code", Kind::Fixed, 3),
    f(41, "terminal_id", Kind::Fixed, 8),
    f(42, "card_acceptor_id", Kind::Fixed, 15),
    f(46, "amounts_fees", Kind::Lll, 204),
    f(48, "additional_private", Kind::LlllBlob, 9999),
    f(49, "currency", Kind::Fixed, 3),
    f(50, "reconciliation_currency", Kind::Fixed, 3),
    f(52, "pin_data", Kind::Blob, 8),
    f(53, "security_control_information", Kind::Ll, 48),
    f(54, "additional_amounts", Kind::Lll, 120),
    f(55, "icc_data", Kind::LllBlob, 512),
    f(56, "original_data_elements", Kind::Ll, 31),
    f(63, "reserved_private", Kind::Lll, 800),
    f(64, "message_authentication_code", Kind::Blob, 8),
    f(74, "credits_number", Kind::Fixed, 10),
    f(75, "credits_reversal_number", Kind::Fixed, 10),
    f(76, "debits_number", Kind::Fixed, 10),
    f(77, "debits_reversal_number", Kind::Fixed, 10),
    f(81, "authorisations_number", Kind::Fixed, 10),
    f(86, "credits_amount", Kind::Fixed, 16),
    f(87, "credits_reversal_amount", Kind::Fixed, 16),
    f(88, "debits_amount", Kind::Fixed, 16),
    f(89, "debits_reversal_amount", Kind::Fixed, 16),
    f(90, "authorisations_reversal_number", Kind::Fixed, 10),
    f(97, "net_reconciliation_amount", Kind::Fixed, 17),
    f(109, "credits_fee_amounts", Kind::Ll, 84),
    f(110, "debits_fee_amounts", Kind::Ll, 84),
    f(123, "receipt_data", Kind::Lll, 999),
    f(124, "display_data", Kind::Lll, 999),
    f(128, "message_authentication_code_extended", Kind::Blob, 8),
];

const fn f(number: usize, name: &'static str, kind: Kind, max: usize) -> Field {
    Field {
        number,
        name,
        kind,
        max,
    }
}

fn field(number: usize) -> Result<&'static Field, String> {
    FIELDS
        .iter()
        .find(|field| field.number == number)
        .ok_or_else(|| format!("DE{number} is not supported by the AU EFTEX profile"))
}

fn bitmap_has(bitmap: &[u8], number: usize) -> bool {
    let index = number - 1;
    bitmap[index / 8] & (1 << (7 - index % 8)) != 0
}

fn prefix(kind: Kind) -> usize {
    match kind {
        Kind::Ll => 2,
        Kind::Lll | Kind::LllBlob => 3,
        Kind::LlllBlob => 4,
        _ => 0,
    }
}

pub fn message_length(message: &[u8]) -> Result<Option<usize>, String> {
    if message.len() < 4 {
        return Ok(None);
    }
    digits(&message[..4], "MTI")?;
    if message.len() < 12 {
        return Ok(None);
    }
    let bitmap_bytes = if bitmap_has(&message[4..12], 1) {
        16
    } else {
        8
    };
    if message.len() < 4 + bitmap_bytes {
        return Ok(None);
    }
    let bitmap = &message[4..4 + bitmap_bytes];
    if bitmap_bytes == 16 && bitmap_has(bitmap, 65) {
        return Err("tertiary bitmap is not supported by the AU EFTEX profile".into());
    }
    let mut offset = 4 + bitmap_bytes;
    for number in 2..=bitmap_bytes * 8 {
        if number == 65 || !bitmap_has(bitmap, number) {
            continue;
        }
        let spec = field(number)?;
        let prefix = prefix(spec.kind);
        if message.len() < offset + prefix {
            return Ok(None);
        }
        let length = if prefix == 0 {
            spec.max
        } else {
            digits(&message[offset..offset + prefix], spec.name)?;
            std::str::from_utf8(&message[offset..offset + prefix])
                .unwrap()
                .parse::<usize>()
                .unwrap()
        };
        if length > spec.max {
            return Err(format!("{} length exceeds profile maximum", spec.name));
        }
        offset += prefix;
        if message.len() < offset + length {
            return Ok(None);
        }
        if !matches!(spec.kind, Kind::Blob | Kind::LllBlob | Kind::LlllBlob) {
            ascii(&message[offset..offset + length], spec.name)?;
        }
        offset += length;
    }
    Ok(Some(offset))
}

pub fn decode(message: &[u8]) -> Result<Map<String, Value>, String> {
    let length = message_length(message)?.ok_or("ISO 8583 message is incomplete")?;
    if length != message.len() {
        return Err("ISO 8583 message has trailing bytes not described by bitmap".into());
    }
    let mut document = Map::new();
    document.insert(
        "message_type".into(),
        json!({"type":"string","value":ascii(&message[..4], "MTI")?}),
    );
    let bitmap_bytes = if bitmap_has(&message[4..12], 1) {
        16
    } else {
        8
    };
    let bitmap = &message[4..4 + bitmap_bytes];
    let mut offset = 4 + bitmap_bytes;
    for number in 2..=bitmap_bytes * 8 {
        if number == 65 || !bitmap_has(bitmap, number) {
            continue;
        }
        let spec = field(number)?;
        let prefix = prefix(spec.kind);
        let length = if prefix == 0 {
            spec.max
        } else {
            std::str::from_utf8(&message[offset..offset + prefix])
                .unwrap()
                .parse()
                .unwrap()
        };
        offset += prefix;
        let payload = &message[offset..offset + length];
        let value = match spec.kind {
            Kind::Blob | Kind::LllBlob | Kind::LlllBlob => {
                json!({"type":"blob","value_base64":STANDARD.encode(payload)})
            }
            Kind::Amount => {
                json!({"type":"int","value":ascii(payload,spec.name)?.trim_start_matches('0').to_string().chars().collect::<String>()})
            }
            _ => json!({"type":"string","value":ascii(payload,spec.name)?}),
        };
        let value = if spec.kind == Kind::Amount && value["value"] == "" {
            json!({"type":"int","value":"0"})
        } else {
            value
        };
        document.insert(spec.name.into(), value);
        offset += length;
    }
    Ok(document)
}

pub fn encode(document: &Map<String, Value>) -> Result<Vec<u8>, String> {
    for name in document.keys() {
        if !FIELDS.iter().any(|field| field.name == name) {
            return Err(format!("unknown document field: {name}"));
        }
    }
    let mti = tagged_text(
        document
            .get("message_type")
            .ok_or("message_type is required")?,
        "message_type",
        "string",
    )?;
    if mti.len() != 4 {
        return Err("message_type must contain exactly 4 ASCII digits".into());
    }
    digits(mti.as_bytes(), "message_type")?;
    let present: Vec<_> = FIELDS[1..]
        .iter()
        .filter(|field| document.contains_key(field.name))
        .collect();
    let secondary = present.iter().any(|field| field.number > 64);
    let mut bitmap = vec![0u8; if secondary { 16 } else { 8 }];
    if secondary {
        bitmap[0] |= 0x80;
    }
    let mut output = mti.into_bytes();
    for spec in &present {
        let index = spec.number - 1;
        bitmap[index / 8] |= 1 << (7 - index % 8);
    }
    output.extend(bitmap);
    for spec in present {
        let value = &document[spec.name];
        let mut payload = match spec.kind {
            Kind::Blob | Kind::LllBlob | Kind::LlllBlob => tagged_blob(value, spec.name)?,
            Kind::Amount => {
                let integer = tagged_text(value, spec.name, "int")?;
                if !integer.bytes().all(|byte| byte.is_ascii_digit())
                    || (integer.len() > 1 && integer.starts_with('0'))
                {
                    return Err(format!("{} must use a canonical integer string", spec.name));
                }
                format!("{integer:0>width$}", width = spec.max).into_bytes()
            }
            _ => tagged_text(value, spec.name, "string")?.into_bytes(),
        };
        if payload.len() > spec.max {
            return Err(format!("{} length exceeds profile maximum", spec.name));
        }
        let prefix = prefix(spec.kind);
        if prefix == 0 && payload.len() != spec.max {
            return Err(format!(
                "{} must contain exactly {} bytes",
                spec.name, spec.max
            ));
        }
        if prefix != 0 {
            output.extend(format!("{:0width$}", payload.len(), width = prefix).bytes());
        }
        output.append(&mut payload);
    }
    Ok(output)
}

fn tagged_text(value: &Value, name: &str, kind: &str) -> Result<String, String> {
    let object = value
        .as_object()
        .ok_or_else(|| format!("{name} must be a closed {kind} tagged value"))?;
    if object.len() != 2 || object.get("type").and_then(Value::as_str) != Some(kind) {
        return Err(format!("{name} must be a closed {kind} tagged value"));
    }
    let text = object
        .get("value")
        .and_then(Value::as_str)
        .ok_or_else(|| format!("{name} must be a closed {kind} tagged value"))?;
    if !text.is_ascii() {
        return Err(format!("{name} must contain ASCII characters"));
    }
    Ok(text.to_owned())
}

fn tagged_blob(value: &Value, name: &str) -> Result<Vec<u8>, String> {
    let object = value
        .as_object()
        .ok_or_else(|| format!("{name} must be a closed blob tagged value"))?;
    if object.len() != 2 || object.get("type").and_then(Value::as_str) != Some("blob") {
        return Err(format!("{name} must be a closed blob tagged value"));
    }
    STANDARD
        .decode(
            object
                .get("value_base64")
                .and_then(Value::as_str)
                .ok_or_else(|| format!("{name} must be a closed blob tagged value"))?,
        )
        .map_err(|_| format!("{name} must contain valid base64"))
}

fn digits(value: &[u8], name: &str) -> Result<(), String> {
    if value.iter().all(u8::is_ascii_digit) {
        Ok(())
    } else {
        Err(format!("{name} must contain ASCII digits"))
    }
}
fn ascii<'a>(value: &'a [u8], name: &str) -> Result<&'a str, String> {
    std::str::from_utf8(value)
        .map_err(|_| format!("{name} must contain ASCII bytes"))
        .and_then(|value| {
            if value.is_ascii() {
                Ok(value)
            } else {
                Err(format!("{name} must contain ASCII bytes"))
            }
        })
}

pub fn contains_mac(document: &Map<String, Value>) -> bool {
    document.contains_key("message_authentication_code")
        || document.contains_key("message_authentication_code_extended")
}
