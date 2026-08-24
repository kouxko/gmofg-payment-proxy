use super::super::*;
use super::four_field_schema;
use crate::ErrorCode;

#[test]
fn document_supports_all_value_types_and_preserves_schema_order() {
    let mut document = Document::new(four_field_schema());
    for name in ["merchant", "amount", "approved", "raw"] {
        assert!(!document.has(name).unwrap());
    }

    document
        .set("merchant", DocumentValue::String("M-001".into()))
        .unwrap();
    document.set("amount", DocumentValue::Int(1234)).unwrap();
    document.set("approved", DocumentValue::Bool(true)).unwrap();
    document
        .set("raw", DocumentValue::Blob(vec![0, 127, 255]))
        .unwrap();

    assert_eq!(
        document.get("merchant").unwrap(),
        &DocumentValue::String("M-001".into())
    );
    assert_eq!(document.get("amount").unwrap(), &DocumentValue::Int(1234));
    assert_eq!(
        document.get("approved").unwrap(),
        &DocumentValue::Bool(true)
    );
    assert_eq!(
        document.get("raw").unwrap(),
        &DocumentValue::Blob(vec![0, 127, 255])
    );
    assert!(document.has("amount").unwrap());
    assert_eq!(document.schema().id().as_str(), "payment-message");
    assert_eq!(
        document
            .fields()
            .map(|state| state.field.name().as_str())
            .collect::<Vec<_>>(),
        vec!["merchant", "amount", "approved", "raw"]
    );
}

#[test]
fn unknown_and_declared_but_unassigned_fields_have_distinct_errors() {
    let document = Document::new(four_field_schema());

    let unknown = document.get("currency").unwrap_err();
    assert_eq!(unknown.code, ErrorCode::DocumentFieldUndeclared);
    assert!(unknown.field_errors.contains_key("document.currency"));
    assert_eq!(
        document.has("currency").unwrap_err().code,
        ErrorCode::DocumentFieldUndeclared
    );

    let unassigned = document.get("amount").unwrap_err();
    assert_eq!(unassigned.code, ErrorCode::DocumentFieldUnassigned);
    assert!(unassigned.field_errors.contains_key("document.amount"));
}

#[test]
fn set_rejects_unknown_fields_and_every_type_mismatch_without_mutation() {
    let cases = [
        ("merchant", DocumentValue::Int(1)),
        ("amount", DocumentValue::Bool(true)),
        ("approved", DocumentValue::Blob(vec![1])),
        ("raw", DocumentValue::String("raw".into())),
    ];
    for (name, value) in cases {
        let mut document = Document::new(four_field_schema());
        let error = document.set(name, value).unwrap_err();
        assert_eq!(error.code, ErrorCode::DocumentFieldTypeMismatch);
        assert!(!document.has(name).unwrap());
    }

    let mut document = Document::new(four_field_schema());
    let error = document
        .set("currency", DocumentValue::String("CNY".into()))
        .unwrap_err();
    assert_eq!(error.code, ErrorCode::DocumentFieldUndeclared);
}

#[test]
fn values_use_tagged_unambiguous_serialization() {
    let cases = [
        (
            DocumentValue::String("7".into()),
            r#"{"type":"string","value":"7"}"#,
        ),
        (DocumentValue::Int(7), r#"{"type":"int","value":7}"#),
        (DocumentValue::Bool(true), r#"{"type":"bool","value":true}"#),
        (
            DocumentValue::Blob(vec![7]),
            r#"{"type":"blob","value":[7]}"#,
        ),
    ];
    for (value, expected) in cases {
        assert_eq!(serde_json::to_string(&value).unwrap(), expected);
        assert_eq!(
            serde_json::from_str::<DocumentValue>(expected).unwrap(),
            value
        );
    }
}

#[test]
fn document_clone_eq_and_serde_round_trip_preserve_unassigned_slots() {
    let mut document = Document::new(four_field_schema());
    document.set("amount", DocumentValue::Int(99)).unwrap();
    assert_eq!(document.clone(), document);

    let json = serde_json::to_string(&document).unwrap();
    let restored: Document = serde_json::from_str(&json).unwrap();
    assert_eq!(restored, document);
    assert!(!restored.has("merchant").unwrap());
    assert_eq!(restored.get("amount").unwrap(), &DocumentValue::Int(99));
}

#[test]
fn document_deserialization_revalidates_slot_count_types_and_unknown_keys() {
    let document = Document::new(four_field_schema());
    let valid = serde_json::to_value(document).unwrap();

    let mut too_few = valid.clone();
    too_few["values"].as_array_mut().unwrap().pop();
    let error = serde_json::from_value::<Document>(too_few).unwrap_err();
    assert!(error.to_string().contains("DOCUMENT_SCHEMA_INVALID"));

    let mut wrong_type = valid.clone();
    wrong_type["values"][0] = serde_json::json!({"type": "int", "value": 7});
    let error = serde_json::from_value::<Document>(wrong_type).unwrap_err();
    assert!(error.to_string().contains("DOCUMENT_FIELD_TYPE_MISMATCH"));

    let mut unknown_key = valid;
    unknown_key["extra"] = serde_json::json!(true);
    assert!(serde_json::from_value::<Document>(unknown_key).is_err());
}

#[test]
fn document_error_codes_have_stable_wire_values() {
    let cases = [
        (
            ErrorCode::ProtocolPackageInvalid,
            "PROTOCOL_PACKAGE_INVALID",
        ),
        (ErrorCode::DocumentSchemaInvalid, "DOCUMENT_SCHEMA_INVALID"),
        (
            ErrorCode::DocumentFieldUndeclared,
            "DOCUMENT_FIELD_UNDECLARED",
        ),
        (
            ErrorCode::DocumentFieldUnassigned,
            "DOCUMENT_FIELD_UNASSIGNED",
        ),
        (
            ErrorCode::DocumentFieldTypeMismatch,
            "DOCUMENT_FIELD_TYPE_MISMATCH",
        ),
    ];
    for (code, expected) in cases {
        assert_eq!(code.as_str(), expected);
        assert_eq!(
            serde_json::to_string(&code).unwrap(),
            format!("\"{expected}\"")
        );
    }
}
