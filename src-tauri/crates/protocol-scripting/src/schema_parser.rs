use intercept_proxy_domain::{
    DocumentField, DocumentFieldName, DocumentFieldType, DocumentSchema, DocumentSchemaId,
    DomainError,
};
use serde::Deserialize;

use crate::{
    ProtocolPackageFile, ProtocolPackageParseError, ProtocolPackageParseErrorCode,
    toml_parser::parse_toml,
};

/// `document.toml` 的独立解析上限；字段数量仍由 T03 的 [`DocumentSchema`] 限制。
pub const MAX_DOCUMENT_SCHEMA_TOML_BYTES: usize = 256 * 1024;

/// 严格解析 `document.toml`，并通过 T03 领域构造器重新校验全部不变量。
///
/// 字段保存在 `Vec` 中并按 TOML 的 `[[fields]]` 出现顺序构造，因而规则目录、Document 值槽和
/// 后续 UI 表格共享同一稳定顺序。废弃的 `required` 或任何其他未知键都会被拒绝。
pub fn parse_document_schema(input: &str) -> Result<DocumentSchema, ProtocolPackageParseError> {
    let wire: DocumentSchemaWire = parse_toml(
        input,
        ProtocolPackageFile::DocumentSchema,
        MAX_DOCUMENT_SCHEMA_TOML_BYTES,
    )?;

    let id = DocumentSchemaId::new(wire.id).map_err(|error| schema_error(&error, "id"))?;
    let fields = wire
        .fields
        .into_iter()
        .enumerate()
        .map(|(index, field)| field_from_wire(index, field))
        .collect::<Result<Vec<_>, _>>()?;
    DocumentSchema::new(id, wire.version, wire.title, fields)
        .map_err(|error| schema_error(&error, "fields"))
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct DocumentSchemaWire {
    id: String,
    version: u32,
    title: String,
    fields: Vec<DocumentFieldWire>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct DocumentFieldWire {
    name: String,
    label: String,
    #[serde(rename = "type")]
    field_type: String,
}

fn field_from_wire(
    index: usize,
    wire: DocumentFieldWire,
) -> Result<DocumentField, ProtocolPackageParseError> {
    let name = DocumentFieldName::new(wire.name)
        .map_err(|_| invalid_schema(&format!("fields[{index}].name")))?;
    let field_type = match wire.field_type.as_str() {
        "string" => DocumentFieldType::String,
        "int" => DocumentFieldType::Int,
        "bool" => DocumentFieldType::Bool,
        "blob" => DocumentFieldType::Blob,
        _ => return Err(invalid_schema(&format!("fields[{index}].type"))),
    };
    DocumentField::new(name, field_type, wire.label)
        .map_err(|_| invalid_schema(&format!("fields[{index}].label")))
}

fn schema_error(error: &DomainError, fallback: &str) -> ProtocolPackageParseError {
    let domain_field = error
        .field_errors
        .keys()
        .next()
        .map_or(fallback, String::as_str);
    let field = domain_field.strip_prefix("schema.").unwrap_or(domain_field);
    invalid_schema(field)
}

fn invalid_schema(field: &str) -> ProtocolPackageParseError {
    ProtocolPackageParseError::new(
        ProtocolPackageParseErrorCode::DocumentSchemaInvalid,
        ProtocolPackageFile::DocumentSchema,
        field,
    )
}
