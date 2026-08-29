use intercept_proxy_domain::DocumentSchemaNode;

use crate::{
    MAX_DOCUMENT_SCHEMA_TOML_BYTES, ProtocolPackageFile, ProtocolPackageParseErrorCode,
    parse_document_schema,
};

const RECURSIVE_SCHEMA: &str = r#"
type = "object"
title = "Payment"

[properties.customer]
type = "object"

[properties.customer.properties.name]
type = "string"
title = "Name"

[properties.lines]
type = "array"

[properties.lines.items]
type = "object"

[properties.lines.items.properties.amount]
type = "number"
"#;

#[test]
fn parses_recursive_object_array_and_optional_titles() {
    let schema = parse_document_schema(RECURSIVE_SCHEMA).unwrap();
    let DocumentSchemaNode::Object { title, properties } = schema else {
        panic!("root must be object")
    };
    assert_eq!(title.as_deref(), Some("Payment"));
    assert!(matches!(
        properties["customer"],
        DocumentSchemaNode::Object { .. }
    ));
    assert!(matches!(
        properties["lines"],
        DocumentSchemaNode::Array { .. }
    ));
}

#[test]
fn accepts_exact_schema_type_set() {
    for kind in ["string", "number", "boolean", "object", "array"] {
        let input = match kind {
            "object" => "type = \"object\"\nproperties = {}\n".to_owned(),
            "array" => "type = \"array\"\n[items]\ntype = \"string\"\n".to_owned(),
            _ => format!("type = \"{kind}\"\n"),
        };
        parse_document_schema(&input).unwrap();
    }
    for kind in ["null", "integer", "blob"] {
        let error = parse_document_schema(&format!("type = \"{kind}\"\n")).unwrap_err();
        assert_eq!(error.code(), ProtocolPackageParseErrorCode::TomlInvalid);
    }
}

#[test]
fn array_items_is_required_and_unknown_fields_are_rejected() {
    for input in [
        "type = \"array\"\n",
        "type = \"string\"\nunknown = true\n",
        "type = \"object\"\nproperties = []\n",
    ] {
        let error = parse_document_schema(input).unwrap_err();
        assert_eq!(error.code(), ProtocolPackageParseErrorCode::TomlInvalid);
        assert_eq!(error.file(), ProtocolPackageFile::DocumentSchema);
    }
}

#[test]
fn duplicate_keys_are_rejected_by_strict_toml() {
    let error = parse_document_schema("type = \"string\"\ntype = \"number\"\n").unwrap_err();
    assert_eq!(error.code(), ProtocolPackageParseErrorCode::TomlInvalid);
}

#[test]
fn schema_source_size_is_bounded_before_parse() {
    let input = "x".repeat(MAX_DOCUMENT_SCHEMA_TOML_BYTES + 1);
    let error = parse_document_schema(&input).unwrap_err();
    assert_eq!(
        error.code(),
        ProtocolPackageParseErrorCode::DocumentSchemaInvalid
    );
    assert_eq!(error.file(), ProtocolPackageFile::DocumentSchema);
}

#[test]
fn invalid_nested_title_is_rejected_without_a_document_value() {
    let error = parse_document_schema(
        "type = \"object\"\n[properties.value]\ntype = \"string\"\ntitle = \"  \"\n",
    )
    .unwrap_err();
    assert_eq!(
        error.code(),
        ProtocolPackageParseErrorCode::DocumentSchemaInvalid
    );
}
