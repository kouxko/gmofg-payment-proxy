//! Recursive protocol-rule value parsing at the application boundary.

use intercept_proxy_domain::{DocumentValue, MAX_DOCUMENT_RULE_STRING_BYTES};

use crate::{
    AppError, ProtocolPackageSchemaFieldTypeViewModel as FieldType, parse_protocol_rule_value,
};

#[test]
fn parses_each_recursive_schema_value_kind_without_frontend_coercion() {
    assert_eq!(
        parse_protocol_rule_value(FieldType::String, " 金额¥ ").unwrap(),
        DocumentValue::String(" 金额¥ ".into())
    );
    assert_eq!(
        parse_protocol_rule_value(FieldType::Number, "1.5").unwrap(),
        intercept_proxy_domain::Document::parse_json("1.5")
            .unwrap()
            .root()
            .clone()
    );
    assert_eq!(
        parse_protocol_rule_value(FieldType::Boolean, "true").unwrap(),
        DocumentValue::Boolean(true)
    );
    assert!(matches!(
        parse_protocol_rule_value(FieldType::Object, r#"{"nested":[null,1]}"#).unwrap(),
        DocumentValue::Object(_)
    ));
    assert!(matches!(
        parse_protocol_rule_value(FieldType::Array, r#"[1,"two",false]"#).unwrap(),
        DocumentValue::Array(_)
    ));
}

#[test]
fn number_uses_standard_json_and_the_javascript_safe_integer_contract() {
    for raw in ["0", "-0", "1.5", "1e3", "1e-400", " 1.5 "] {
        assert!(matches!(
            parse_protocol_rule_value(FieldType::Number, raw).unwrap(),
            DocumentValue::Number(_)
        ));
    }
    for raw in ["", "+1", "1_000", "NaN", "Infinity", "9007199254740992"] {
        assert!(
            parse_protocol_rule_value(FieldType::Number, raw).is_err(),
            "{raw}"
        );
    }
}

#[test]
fn string_limit_counts_utf8_bytes_and_preserves_the_exact_value() {
    let exact = "x".repeat(MAX_DOCUMENT_RULE_STRING_BYTES);
    assert_eq!(
        parse_protocol_rule_value(FieldType::String, &exact).unwrap(),
        DocumentValue::String(exact)
    );
    let oversized = "s".repeat(MAX_DOCUMENT_RULE_STRING_BYTES + 1);
    assert_error(
        &parse_protocol_rule_value(FieldType::String, &oversized).unwrap_err(),
        "PROTOCOL_RULE_VALUE_TOO_LARGE",
    );
}

#[test]
fn boolean_and_container_kinds_use_standard_json_and_remain_type_strict() {
    assert_eq!(
        parse_protocol_rule_value(FieldType::Boolean, " false ").unwrap(),
        DocumentValue::Boolean(false)
    );
    assert_error(
        &parse_protocol_rule_value(FieldType::Boolean, "TRUE").unwrap_err(),
        "JSON_INVALID",
    );
    for raw in ["1", r#""true""#] {
        assert_error(
            &parse_protocol_rule_value(FieldType::Boolean, raw).unwrap_err(),
            "PROTOCOL_RULE_VALUE_INVALID",
        );
    }
    assert_error(
        &parse_protocol_rule_value(FieldType::Object, "[]").unwrap_err(),
        "PROTOCOL_RULE_VALUE_INVALID",
    );
    assert_error(
        &parse_protocol_rule_value(FieldType::Array, "{}").unwrap_err(),
        "PROTOCOL_RULE_VALUE_INVALID",
    );
}

fn assert_error(error: &AppError, code: &str) {
    assert_eq!(error.view_model.code, code);
    assert!(!error.view_model.retryable);
}
