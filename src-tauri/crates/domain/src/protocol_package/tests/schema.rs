use super::super::*;
use super::{field, four_field_schema};
use crate::ErrorCode;
use crate::protocol_package::schema::RHAI_RESERVED_WORDS;

#[test]
fn schema_and_field_ids_enforce_ascii_boundaries() {
    let schema_maximum = format!("a{}", "-".repeat(MAX_DOCUMENT_SCHEMA_ID_LEN - 1));
    assert!(DocumentSchemaId::new(schema_maximum).is_ok());
    let field_maximum = format!("a{}", "_".repeat(MAX_DOCUMENT_FIELD_NAME_LEN - 1));
    assert!(DocumentFieldName::new(field_maximum).is_ok());

    for invalid in ["", "1schema", "Schema", "schema_name", "schema.1", "文档"] {
        let error = DocumentSchemaId::new(invalid).unwrap_err();
        assert_eq!(error.code, ErrorCode::DocumentSchemaInvalid);
    }
    for invalid in ["", "1field", "Field", "field-name", "field.name", "金额"] {
        let error = DocumentFieldName::new(invalid).unwrap_err();
        assert_eq!(error.code, ErrorCode::DocumentSchemaInvalid);
    }
    assert!(DocumentSchemaId::new(format!("a{}", "a".repeat(MAX_DOCUMENT_SCHEMA_ID_LEN))).is_err());
    assert!(
        DocumentFieldName::new(format!("a{}", "a".repeat(MAX_DOCUMENT_FIELD_NAME_LEN))).is_err()
    );
}

#[test]
fn field_names_reject_all_v1_rhai_reserved_words() {
    for reserved in RHAI_RESERVED_WORDS {
        assert!(is_rhai_reserved_word(reserved));
        let error = DocumentFieldName::new(*reserved).unwrap_err();
        assert_eq!(error.code, ErrorCode::DocumentSchemaInvalid, "{reserved}");
    }
    assert!(!is_rhai_reserved_word("decode_payment"));
}

#[test]
fn schema_and_field_names_support_from_str_display_and_validated_serde() {
    let schema_id: DocumentSchemaId = "payment-message".parse().unwrap();
    assert_eq!(schema_id.to_string(), "payment-message");
    assert_eq!(
        serde_json::to_string(&schema_id).unwrap(),
        "\"payment-message\""
    );
    assert!(serde_json::from_str::<DocumentSchemaId>("\"INVALID\"").is_err());

    let field_name: DocumentFieldName = "merchant_id".parse().unwrap();
    assert_eq!(field_name.to_string(), "merchant_id");
    assert_eq!(
        serde_json::to_string(&field_name).unwrap(),
        "\"merchant_id\""
    );
    assert!(serde_json::from_str::<DocumentFieldName>("\"while\"").is_err());
}

#[test]
fn display_text_counts_unicode_characters_not_bytes() {
    let maximum = "界".repeat(MAX_DOCUMENT_DISPLAY_TEXT_LEN);
    assert!(
        DocumentField::new(
            DocumentFieldName::new("memo").unwrap(),
            DocumentFieldType::String,
            &maximum,
        )
        .is_ok()
    );
    assert!(
        DocumentField::new(
            DocumentFieldName::new("memo").unwrap(),
            DocumentFieldType::String,
            format!("{maximum}界"),
        )
        .is_err()
    );
    for invalid in ["", " \t\n"] {
        assert!(
            DocumentField::new(
                DocumentFieldName::new("memo").unwrap(),
                DocumentFieldType::String,
                invalid,
            )
            .is_err()
        );
    }
}

#[test]
fn schema_rejects_zero_version_empty_fields_duplicates_and_field_overflow() {
    let id = || DocumentSchemaId::new("message").unwrap();
    let title = "Message";
    assert_eq!(
        DocumentSchema::new(
            id(),
            0,
            title,
            vec![field("amount", DocumentFieldType::Int)]
        )
        .unwrap_err()
        .code,
        ErrorCode::DocumentSchemaInvalid
    );
    assert!(DocumentSchema::new(id(), 1, title, vec![]).is_err());
    assert!(
        DocumentSchema::new(
            id(),
            1,
            title,
            vec![
                field("amount", DocumentFieldType::Int),
                field("amount", DocumentFieldType::String),
            ],
        )
        .is_err()
    );
    let overflow = (0..=MAX_DOCUMENT_FIELDS)
        .map(|index| field(&format!("field_{index}"), DocumentFieldType::String))
        .collect();
    assert!(DocumentSchema::new(id(), 1, title, overflow).is_err());
}

#[test]
fn schema_title_and_exact_maximum_field_count_are_validated() {
    let fields = (0..MAX_DOCUMENT_FIELDS)
        .map(|index| field(&format!("field_{index}"), DocumentFieldType::String))
        .collect();
    let title = "界".repeat(MAX_DOCUMENT_DISPLAY_TEXT_LEN);
    assert!(
        DocumentSchema::new(DocumentSchemaId::new("message").unwrap(), 1, title, fields,).is_ok()
    );
    assert!(
        DocumentSchema::new(
            DocumentSchemaId::new("message").unwrap(),
            1,
            " ",
            vec![field("amount", DocumentFieldType::Int)],
        )
        .is_err()
    );
}

#[test]
fn schema_accessors_and_field_order_survive_serde_round_trip() {
    let schema = four_field_schema();
    assert_eq!(schema.id().as_str(), "payment-message");
    assert_eq!(schema.version(), 1);
    assert_eq!(schema.title(), "Payment Message");
    assert_eq!(schema.field_index("approved"), Some(2));
    assert_eq!(schema.field_index("unknown"), None);
    assert_eq!(schema.fields()[0].name().as_str(), "merchant");
    assert_eq!(schema.fields()[0].label(), "MERCHANT");
    assert_eq!(schema.fields()[0].field_type(), DocumentFieldType::String);

    let json = serde_json::to_string(&schema).unwrap();
    let merchant = json.find("merchant").unwrap();
    let amount = json.find("amount").unwrap();
    let approved = json.find("approved").unwrap();
    let raw = json.find("raw").unwrap();
    assert!(merchant < amount && amount < approved && approved < raw);
    assert_eq!(
        serde_json::from_str::<DocumentSchema>(&json).unwrap(),
        schema
    );
    assert_eq!(schema.clone(), schema);
}

#[test]
fn schema_deserialization_rejects_unknown_type_unknown_keys_and_invalid_nested_values() {
    let valid = serde_json::to_value(four_field_schema()).unwrap();
    assert!(valid["fields"][0].get("required").is_none());

    let mut unknown_type = valid.clone();
    unknown_type["fields"][0]["type"] = serde_json::json!("decimal");
    assert!(serde_json::from_value::<DocumentSchema>(unknown_type).is_err());

    let mut unknown_key = valid.clone();
    unknown_key["extra"] = serde_json::json!(true);
    assert!(serde_json::from_value::<DocumentSchema>(unknown_key).is_err());

    let mut deprecated_required = valid.clone();
    deprecated_required["fields"][0]["required"] = serde_json::json!(true);
    assert!(serde_json::from_value::<DocumentSchema>(deprecated_required).is_err());

    let mut invalid_name = valid;
    invalid_name["fields"][0]["name"] = serde_json::json!("if");
    assert!(serde_json::from_value::<DocumentSchema>(invalid_name).is_err());
}

#[test]
fn field_types_have_unambiguous_stable_wire_values() {
    let cases = [
        (DocumentFieldType::String, "string"),
        (DocumentFieldType::Int, "int"),
        (DocumentFieldType::Bool, "bool"),
        (DocumentFieldType::Blob, "blob"),
    ];
    for (field_type, expected) in cases {
        assert_eq!(field_type.as_str(), expected);
        assert_eq!(
            serde_json::to_string(&field_type).unwrap(),
            format!("\"{expected}\"")
        );
    }
}
