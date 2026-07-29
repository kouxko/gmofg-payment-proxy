//! Strict Shift-JIS codec (`MESSAGE-002`, `MESSAGE-003`, `TEST-CODEC`).

use encoding_rs::SHIFT_JIS;

use crate::{ErrorCode, ProxyError, Result};

pub fn decode_strict(bytes: &[u8]) -> Result<String> {
    let (decoded, had_errors) = SHIFT_JIS.decode_without_bom_handling(bytes);
    if had_errors {
        return Err(ProxyError::new(
            ErrorCode::ShiftJisDecodeFailed,
            "body contains an invalid Shift-JIS byte sequence",
        ));
    }
    Ok(decoded.into_owned())
}

pub fn encode_strict(text: &str) -> Result<Vec<u8>> {
    let (encoded, _encoding, had_errors) = SHIFT_JIS.encode(text);
    if had_errors {
        return Err(ProxyError::new(
            ErrorCode::ShiftJisEncodeFailed,
            "text cannot be represented losslessly in Shift-JIS",
        ));
    }
    Ok(encoded.into_owned())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn strict_round_trip_and_failures() {
        let encoded = encode_strict("決済OK").expect("encodable Japanese");
        assert_eq!(decode_strict(&encoded).unwrap(), "決済OK");
        assert!(decode_strict(&[0x82]).is_err());
        assert!(encode_strict("emoji: 🧪").is_err());
    }

    #[test]
    fn ascii_and_empty_are_valid() {
        assert_eq!(decode_strict(b"abc").unwrap(), "abc");
        assert_eq!(encode_strict("").unwrap(), Vec::<u8>::new());
    }
}
