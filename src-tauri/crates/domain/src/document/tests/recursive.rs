use std::collections::BTreeMap;

use serde_json::json;

use super::*;
use crate::ErrorCode;

fn number(value: f64) -> DocumentValue {
    DocumentValue::Number(DocumentNumber::new(value).expect("finite JS number"))
}

#[test]
fn json_value_tree_round_trips_standard_json_and_uses_last_duplicate_key() {
    let document = Document::parse_json(
        r#"{"name":"merchant","amount":12.5,"active":true,"missing":null,"nested":{"items":[1,"two",false]}}"#,
    )
    .expect("recursive JSON document");
    assert_eq!(
        document.to_json().expect("encode recursive JSON"),
        r#"{"active":true,"amount":12.5,"missing":null,"name":"merchant","nested":{"items":[1.0,"two",false]}}"#
    );

    let duplicate = Document::parse_json(r#"{"value":1,"value":2}"#)
        .expect("duplicate object keys use the last value");
    assert_eq!(
        duplicate
            .resolve(&JsonPointer::parse("/value").unwrap())
            .unwrap(),
        &number(2.0)
    );
}

#[test]
fn json_null_is_real_null_and_the_string_null_variant_name_remains_a_string() {
    let null = Document::parse_json("null").expect("JSON null");
    assert!(null.root().is_null());
    assert_eq!(null.to_json().unwrap(), "null");

    let text = Document::parse_json(r#""Null""#).expect("ordinary string");
    assert_eq!(text.root(), &DocumentValue::String("Null".to_owned()));
    assert_eq!(text.to_json().unwrap(), r#""Null""#);
}

#[test]
fn document_number_enforces_the_javascript_number_contract() {
    assert!(DocumentNumber::new(0.25).is_ok());
    assert_eq!(
        DocumentNumber::new(9_007_199_254_740_992.0)
            .unwrap_err()
            .code,
        ErrorCode::DocumentUnsafeInteger
    );
    assert_eq!(
        DocumentNumber::new(-9_007_199_254_740_992.0)
            .unwrap_err()
            .code,
        ErrorCode::DocumentUnsafeInteger
    );
    assert!(DocumentNumber::new(f64::NAN).is_err());
    assert!(DocumentNumber::new(f64::INFINITY).is_err());
    assert_eq!(
        Document::parse_json("9007199254740992").unwrap_err().code,
        ErrorCode::DocumentUnsafeInteger
    );
    assert_eq!(
        Document::parse_json("9.007199254740992e15")
            .unwrap_err()
            .code,
        ErrorCode::DocumentUnsafeInteger
    );
    assert_eq!(Document::parse_json("1e-400").unwrap().root(), &number(0.0));
}

#[test]
fn rfc6901_supports_root_escaping_empty_unicode_and_array_indices() {
    let document =
        Document::parse_json(r#"{"":{"~key/slash":"empty"},"客户":[{"名字":"Alice"}]}"#).unwrap();
    let root = JsonPointer::parse("").unwrap();
    assert_eq!(root.as_str(), "");
    assert_eq!(root.ui_path(), "/");
    assert_eq!(document.resolve(&root).unwrap(), document.root());
    assert_eq!(
        document
            .resolve(&JsonPointer::parse("//~0key~1slash").unwrap())
            .unwrap(),
        &DocumentValue::String("empty".into())
    );
    assert_eq!(
        document
            .resolve(&JsonPointer::parse("/客户/0/名字").unwrap())
            .unwrap(),
        &DocumentValue::String("Alice".into())
    );
    assert!(JsonPointer::parse("missing-leading-slash").is_err());
    assert!(JsonPointer::parse("/~2").is_err());
}

#[test]
fn set_clear_insert_and_append_have_strict_existing_parent_semantics() {
    let mut document = Document::parse_json(r#"{"object":{"old":1},"array":[1,3]}"#).unwrap();
    document
        .set(
            &JsonPointer::parse("/object/new").unwrap(),
            DocumentValue::Boolean(true),
        )
        .unwrap();
    document
        .set(&JsonPointer::parse("/array/0").unwrap(), number(0.0))
        .unwrap();
    document
        .insert(&JsonPointer::parse("/array").unwrap(), 1, number(2.0))
        .unwrap();
    document
        .append(
            &JsonPointer::parse("/array").unwrap(),
            DocumentValue::null(),
        )
        .unwrap();
    document
        .clear_path(&JsonPointer::parse("/object/old").unwrap())
        .unwrap();
    document
        .clear_path(&JsonPointer::parse("/array/0").unwrap())
        .unwrap();
    assert_eq!(
        serde_json::to_value(document).unwrap(),
        json!({"object": {"new": true}, "array": [2.0, 3.0, null]})
    );

    let mut document = Document::new(DocumentValue::Object(BTreeMap::new()));
    document
        .set(&JsonPointer::root(), DocumentValue::Array(vec![]))
        .unwrap();
    assert!(document.clear_path(&JsonPointer::root()).is_err());
    assert!(
        document
            .set(
                &JsonPointer::parse("/missing/child").unwrap(),
                DocumentValue::null()
            )
            .is_err()
    );
    assert!(
        document
            .insert(&JsonPointer::root(), 1, DocumentValue::null())
            .is_err()
    );
}

#[test]
fn recursive_schema_is_incomplete_metadata_and_validates_its_own_definition() {
    let schema: DocumentSchemaNode = serde_json::from_value(json!({
        "type": "object",
        "title": "Payment",
        "properties": {
            "amount": {"type": "number"},
            "items": {
                "type": "array",
                "items": {"type": "string", "title": "Item"}
            }
        }
    }))
    .expect("recursive metadata schema");
    assert_eq!(schema.title(), Some("Payment"));
    schema
        .validate_definition()
        .expect("schema definition is valid");
    assert!(
        schema
            .resolve(&JsonPointer::parse("/amount").unwrap())
            .is_ok()
    );
    assert!(
        schema
            .resolve(&JsonPointer::parse("/undeclared").unwrap())
            .is_err()
    );

    assert!(
        schema
            .resolve(&JsonPointer::parse("/items/not-an-index").unwrap())
            .is_err()
    );

    let invalid_title: DocumentSchemaNode = serde_json::from_value(json!({
        "type": "array",
        "items": {"type": "string", "title": "  "}
    }))
    .unwrap();
    assert_eq!(
        invalid_title.validate_definition().unwrap_err().code,
        ErrorCode::DocumentSchemaInvalid
    );

    assert!(
        serde_json::from_value::<DocumentSchemaNode>(json!({
            "type": "array"
        }))
        .is_err()
    );
    assert!(
        serde_json::from_value::<DocumentSchemaNode>(json!({
            "type": "null"
        }))
        .is_err()
    );
}
