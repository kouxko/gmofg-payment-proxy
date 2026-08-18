use std::fmt::Write;

use intercept_proxy_domain::{
    Document, DocumentField, DocumentFieldType, DocumentValue, ErrorCode,
};

use super::fixtures::{TEMPLATE_SCHEMA, one_field_schema, schema_with_fields};
use crate::{
    MAX_DOCUMENT_SCHEMA_TOML_BYTES, ProtocolPackageFile, ProtocolPackageParseErrorCode,
    parse_document_schema,
};

#[test]
fn official_iso8583_schema_preserves_declared_field_order() {
    let schema = parse_document_schema(TEMPLATE_SCHEMA).unwrap();
    assert_eq!(schema.id().as_str(), "iso8583-financial-message");
    assert_eq!(schema.version(), 1);
    assert_eq!(schema.title(), "ISO 8583:1987 Financial Message");
    let names: Vec<_> = schema
        .fields()
        .iter()
        .map(|field| field.name().as_str())
        .collect();
    assert_eq!(names.len(), 127);
    assert_eq!(
        &names[..4],
        [
            "message_type",
            "primary_account_number",
            "processing_code",
            "amount",
        ]
    );
    assert_eq!(names[63], "message_authentication_code");
    assert_eq!(names[64], "settlement_code");
    assert_eq!(names[126], "message_authentication_code_2");
    assert_eq!(schema.fields()[3].field_type(), DocumentFieldType::Int);
    assert_eq!(schema.fields()[3].label(), "DE4 Transaction Amount");
}

#[test]
fn schema_supports_all_v1_types_and_document_behavior() {
    let input = schema_with_fields(
        r#"
[[fields]]
name = "text_value"
label = "Text"
type = "string"

[[fields]]
name = "int_value"
label = "Integer"
type = "int"

[[fields]]
name = "bool_value"
label = "Boolean"
type = "bool"

[[fields]]
name = "blob_value"
label = "Blob"
type = "blob"
"#,
    );
    let schema = parse_document_schema(&input).unwrap();
    assert_eq!(
        schema
            .fields()
            .iter()
            .map(DocumentField::field_type)
            .collect::<Vec<_>>(),
        [
            DocumentFieldType::String,
            DocumentFieldType::Int,
            DocumentFieldType::Bool,
            DocumentFieldType::Blob,
        ]
    );

    let mut document = Document::new(schema);
    assert!(!document.has("text_value").unwrap());
    let unassigned = document.get("text_value").unwrap_err();
    assert_eq!(unassigned.code, ErrorCode::DocumentFieldUnassigned);
    document
        .set("text_value", DocumentValue::String("value".to_owned()))
        .unwrap();
    document.set("int_value", DocumentValue::Int(42)).unwrap();
    document
        .set("bool_value", DocumentValue::Bool(true))
        .unwrap();
    document
        .set("blob_value", DocumentValue::Blob(vec![0, 1, 255]))
        .unwrap();
    let names: Vec<_> = document
        .fields()
        .map(|state| state.field.name().as_str())
        .collect();
    assert_eq!(
        names,
        ["text_value", "int_value", "bool_value", "blob_value"]
    );
    let mismatch = document
        .set("int_value", DocumentValue::String("42".to_owned()))
        .unwrap_err();
    assert_eq!(mismatch.code, ErrorCode::DocumentFieldTypeMismatch);
    let unknown = document.has("undeclared").unwrap_err();
    assert_eq!(unknown.code, ErrorCode::DocumentFieldUndeclared);
}

#[test]
fn strict_schema_toml_rejects_unknown_duplicate_required_and_wrong_shapes() {
    let base = one_field_schema();
    let cases = [
        String::new(),
        base.replace("version = 1", "version = \"one\""),
        base.replace(
            "id = \"example-message\"",
            "id = \"example-message\"\nid = \"duplicate\"",
        ),
        format!("{base}\nrequired = true\n"),
        format!("{base}\nunknown = \"value\"\n"),
        base.replace("[[fields]]", "[fields]"),
        schema_with_fields("fields = [\"amount\"]"),
        format!("{base}\n[metadata]\nauthor = \"unknown\"\n"),
        base.replace("type = \"int\"", "type = [\"int\"]"),
    ];
    for input in cases {
        let error = parse_document_schema(&input).unwrap_err();
        assert_eq!(error.code(), ProtocolPackageParseErrorCode::TomlInvalid);
        assert_eq!(error.file(), ProtocolPackageFile::DocumentSchema);
    }
}

#[test]
fn strict_schema_type_errors_keep_the_nested_serde_field_path() {
    let input = one_field_schema().replace("type = \"int\"", "type = [\"int\"]");
    let error = parse_document_schema(&input).unwrap_err();
    assert_eq!(error.code(), ProtocolPackageParseErrorCode::TomlInvalid);
    assert_eq!(error.field(), "fields[0].type");
}

#[test]
fn schema_semantic_matrix_rejects_invalid_identity_fields_and_types() {
    let base = one_field_schema();
    let long_title = "题".repeat(129);
    let long_label = "标".repeat(129);
    let duplicate_fields = r#"
[[fields]]
name = "amount"
label = "Amount"
type = "int"

[[fields]]
name = "amount"
label = "Amount Again"
type = "int"
"#;
    let cases = [
        (
            base.replace("id = \"example-message\"", "id = \"Invalid_ID\""),
            "id",
        ),
        (base.replace("version = 1", "version = 0"), "version"),
        (
            base.replace("title = \"Example Message\"", "title = \"   \""),
            "title",
        ),
        (
            base.replace(
                "title = \"Example Message\"",
                &format!("title = \"{long_title}\""),
            ),
            "title",
        ),
        (schema_with_fields("fields = []"), "fields"),
        (schema_with_fields(duplicate_fields), "fields"),
        (
            base.replace("name = \"amount\"", "name = \"Amount\""),
            "fields[0].name",
        ),
        (
            base.replace("name = \"amount\"", "name = \"while\""),
            "fields[0].name",
        ),
        (
            base.replace("type = \"int\"", "type = \"decimal\""),
            "fields[0].type",
        ),
        (
            base.replace("label = \"Amount\"", "label = \"   \""),
            "fields[0].label",
        ),
        (
            base.replace("label = \"Amount\"", &format!("label = \"{long_label}\"")),
            "fields[0].label",
        ),
    ];
    for (input, field) in cases {
        let error = parse_document_schema(&input).unwrap_err();
        assert_eq!(
            error.code(),
            ProtocolPackageParseErrorCode::DocumentSchemaInvalid
        );
        assert_eq!(error.field(), field);
    }
}

#[test]
fn schema_field_count_and_display_text_boundaries_are_exact() {
    let maximum_fields = (0..256).fold(String::new(), |mut output, index| {
        write!(
            output,
            "[[fields]]\nname = \"field_{index}\"\nlabel = \"Field {index}\"\ntype = \"string\"\n"
        )
        .unwrap();
        output
    });
    let schema = parse_document_schema(&schema_with_fields(&maximum_fields)).unwrap();
    assert_eq!(schema.fields().len(), 256);

    let one_too_many = format!(
        "{maximum_fields}[[fields]]\nname = \"field_256\"\nlabel = \"Field 256\"\ntype = \"string\"\n"
    );
    let error = parse_document_schema(&schema_with_fields(&one_too_many)).unwrap_err();
    assert_eq!(error.field(), "fields");

    let title_128 = "题".repeat(128);
    let valid = one_field_schema().replace(
        "title = \"Example Message\"",
        &format!("title = \"{title_128}\""),
    );
    assert!(parse_document_schema(&valid).is_ok());

    let label_128 = "标".repeat(128);
    let valid =
        one_field_schema().replace("label = \"Amount\"", &format!("label = \"{label_128}\""));
    assert!(parse_document_schema(&valid).is_ok());
}

#[test]
fn oversized_and_sensitive_schema_errors_do_not_echo_input() {
    let oversized = format!(
        "{}#{}",
        one_field_schema(),
        "x".repeat(MAX_DOCUMENT_SCHEMA_TOML_BYTES)
    );
    let error = parse_document_schema(&oversized).unwrap_err();
    assert_eq!(error.code(), ProtocolPackageParseErrorCode::InputTooLarge);
    assert_eq!(error.field(), "$");

    let secret = "4111111111111111";
    let input = one_field_schema().replace("name = \"amount\"", &format!("name = \"{secret}\""));
    let error = parse_document_schema(&input).unwrap_err();
    assert!(!error.to_string().contains(secret));
}
