//! Socket 规则类型化值解析的应用层边界测试。

use intercept_proxy_domain::{
    DocumentValue, MAX_SOCKET_DOCUMENT_RULE_BLOB_BYTES, MAX_SOCKET_DOCUMENT_RULE_STRING_BYTES,
};

use crate::{
    AppError, ProtocolPackageSchemaFieldTypeViewModel as FieldType, parse_socket_rule_value,
};

#[test]
fn parses_all_four_schema_field_types_without_frontend_coercion() {
    assert_eq!(
        parse_socket_rule_value(FieldType::String, " 金额¥ ").unwrap(),
        DocumentValue::String(" 金额¥ ".into())
    );
    assert_eq!(
        parse_socket_rule_value(FieldType::Int, " -42 ").unwrap(),
        DocumentValue::Int(-42)
    );
    assert_eq!(
        parse_socket_rule_value(FieldType::Bool, "true").unwrap(),
        DocumentValue::Bool(true)
    );
    assert_eq!(
        parse_socket_rule_value(FieldType::Blob, "01:a0-FF\n10").unwrap(),
        DocumentValue::Blob(vec![0x01, 0xA0, 0xFF, 0x10])
    );
}

#[test]
fn string_limit_counts_utf8_bytes_and_preserves_the_exact_value() {
    let exact_ascii = "x".repeat(MAX_SOCKET_DOCUMENT_RULE_STRING_BYTES);
    assert_eq!(
        parse_socket_rule_value(FieldType::String, &exact_ascii).unwrap(),
        DocumentValue::String(exact_ascii)
    );
    let exact_multibyte = format!(
        "{}x",
        "界".repeat((MAX_SOCKET_DOCUMENT_RULE_STRING_BYTES - 1) / 3)
    );
    assert_eq!(exact_multibyte.len(), MAX_SOCKET_DOCUMENT_RULE_STRING_BYTES);
    assert!(parse_socket_rule_value(FieldType::String, &exact_multibyte).is_ok());

    let oversized = "s".repeat(MAX_SOCKET_DOCUMENT_RULE_STRING_BYTES + 1);
    assert_error(
        &parse_socket_rule_value(FieldType::String, &oversized).unwrap_err(),
        "SOCKET_RULE_VALUE_TOO_LARGE",
    );
}

#[test]
fn int_accepts_only_decimal_i64_inside_the_javascript_safe_range() {
    const INT_TEXT_LIMIT: usize = 128;
    const MAX_SAFE: i64 = 9_007_199_254_740_991;
    for (raw, expected) in [
        ("0", 0),
        ("-0", 0),
        ("0007", 7),
        ("9007199254740991", MAX_SAFE),
        ("-9007199254740991", -MAX_SAFE),
    ] {
        assert_eq!(
            parse_socket_rule_value(FieldType::Int, raw).unwrap(),
            DocumentValue::Int(expected)
        );
    }
    for raw in [
        "",
        "-",
        "+1",
        "1.0",
        "1e3",
        "1_000",
        "9007199254740992",
        "-9007199254740992",
        "9223372036854775808",
    ] {
        assert_error(
            &parse_socket_rule_value(FieldType::Int, raw).unwrap_err(),
            "SOCKET_RULE_VALUE_INVALID",
        );
    }

    let exact_with_whitespace = format!("{}1", " ".repeat(INT_TEXT_LIMIT - 1));
    assert_eq!(
        parse_socket_rule_value(FieldType::Int, &exact_with_whitespace).unwrap(),
        DocumentValue::Int(1)
    );
    let exact_digits = "1".repeat(INT_TEXT_LIMIT);
    assert_error(
        &parse_socket_rule_value(FieldType::Int, &exact_digits).unwrap_err(),
        "SOCKET_RULE_VALUE_INVALID",
    );
    for oversized in [
        format!("{}1", " ".repeat(INT_TEXT_LIMIT)),
        "1".repeat(INT_TEXT_LIMIT + 1),
    ] {
        assert_error(
            &parse_socket_rule_value(FieldType::Int, &oversized).unwrap_err(),
            "SOCKET_RULE_VALUE_TOO_LARGE",
        );
    }
}

#[test]
fn bool_requires_exact_lowercase_literals() {
    assert_eq!(
        parse_socket_rule_value(FieldType::Bool, "false").unwrap(),
        DocumentValue::Bool(false)
    );
    for raw in ["", "TRUE", "False", " true ", "1", "yes"] {
        assert_error(
            &parse_socket_rule_value(FieldType::Bool, raw).unwrap_err(),
            "SOCKET_RULE_VALUE_INVALID",
        );
    }
}

#[test]
fn blob_streams_supported_separators_and_enforces_decoded_and_text_limits() {
    assert_eq!(
        parse_socket_rule_value(FieldType::Blob, " \n\t:- ").unwrap(),
        DocumentValue::Blob(Vec::new())
    );
    assert_eq!(
        parse_socket_rule_value(FieldType::Blob, "A\u{2003}A:0f-10").unwrap(),
        DocumentValue::Blob(vec![0xAA, 0x0F, 0x10])
    );

    let exact = "AA".repeat(MAX_SOCKET_DOCUMENT_RULE_BLOB_BYTES);
    let DocumentValue::Blob(parsed) = parse_socket_rule_value(FieldType::Blob, &exact).unwrap()
    else {
        panic!("Blob field type must return DocumentValue::Blob")
    };
    assert_eq!(parsed.len(), MAX_SOCKET_DOCUMENT_RULE_BLOB_BYTES);

    for malformed in ["0", "ABC", "GG", "00,11", "00_11", "０１"] {
        assert_error(
            &parse_socket_rule_value(FieldType::Blob, malformed).unwrap_err(),
            "SOCKET_RULE_VALUE_INVALID",
        );
    }
    let decoded_oversized = "AA".repeat(MAX_SOCKET_DOCUMENT_RULE_BLOB_BYTES + 1);
    assert_error(
        &parse_socket_rule_value(FieldType::Blob, &decoded_oversized).unwrap_err(),
        "SOCKET_RULE_VALUE_TOO_LARGE",
    );
    let representation_oversized = " ".repeat(MAX_SOCKET_DOCUMENT_RULE_BLOB_BYTES * 4 + 1);
    assert_error(
        &parse_socket_rule_value(FieldType::Blob, &representation_oversized).unwrap_err(),
        "SOCKET_RULE_VALUE_TOO_LARGE",
    );
}

#[test]
fn errors_expose_only_stable_codes_and_generic_field_details() {
    let secret = "merchant-secret-not-hex";
    let error = parse_socket_rule_value(FieldType::Blob, secret).unwrap_err();
    assert_error(&error, "SOCKET_RULE_VALUE_INVALID");
    let safe = format!("{error:?}");
    assert!(!safe.contains(secret));
    assert_eq!(error.view_model.message, "Socket 规则值格式无效。");
}

fn assert_error(error: &AppError, code: &str) {
    assert_eq!(error.view_model.code, code);
    assert_eq!(error.view_model.field_errors.len(), 1);
    assert!(error.view_model.field_errors.contains_key("raw"));
    assert!(!error.view_model.retryable);
    assert!(error.view_model.entity_id.is_none());
}
