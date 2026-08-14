use serde::de::DeserializeOwned;

use crate::{ProtocolPackageFile, ProtocolPackageParseError, ProtocolPackageParseErrorCode};

pub(crate) fn parse_toml<T: DeserializeOwned>(
    input: &str,
    file: ProtocolPackageFile,
    maximum_bytes: usize,
) -> Result<T, ProtocolPackageParseError> {
    if input.len() > maximum_bytes {
        return Err(ProtocolPackageParseError::new(
            ProtocolPackageParseErrorCode::InputTooLarge,
            file,
            "$",
        ));
    }

    let deserializer = toml::Deserializer::parse(input).map_err(|_| {
        ProtocolPackageParseError::new(ProtocolPackageParseErrorCode::TomlInvalid, file, "$")
    })?;
    serde_path_to_error::deserialize(deserializer).map_err(|error| {
        // 不传播 TOML 的 Display/source：它可能包含无效值或输入行。只保留受控的 Serde 字段路径。
        ProtocolPackageParseError::new(
            ProtocolPackageParseErrorCode::TomlInvalid,
            file,
            &error.path().to_string(),
        )
    })
}
