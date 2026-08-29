use intercept_proxy_domain::DocumentSchemaNode;

use crate::{ProtocolPackageFile, ProtocolPackageParseError, ProtocolPackageParseErrorCode};

/// Maximum recursive schema source size.
pub const MAX_DOCUMENT_SCHEMA_TOML_BYTES: usize = 256 * 1024;

/// Parses strict recursive schema metadata from JSON.
pub fn parse_document_schema(input: &str) -> Result<DocumentSchemaNode, ProtocolPackageParseError> {
    if input.len() > MAX_DOCUMENT_SCHEMA_TOML_BYTES {
        return Err(invalid_schema("$"));
    }
    let schema: DocumentSchemaNode = crate::toml_parser::parse_toml(
        input,
        ProtocolPackageFile::DocumentSchema,
        MAX_DOCUMENT_SCHEMA_TOML_BYTES,
    )?;
    schema
        .validate_definition()
        .map_err(|_| invalid_schema("$"))?;
    Ok(schema)
}

fn invalid_schema(field: &str) -> ProtocolPackageParseError {
    ProtocolPackageParseError::new(
        ProtocolPackageParseErrorCode::DocumentSchemaInvalid,
        ProtocolPackageFile::DocumentSchema,
        field,
    )
}
