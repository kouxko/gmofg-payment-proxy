use std::fmt;

use intercept_proxy_domain::is_rhai_reserved_word;

use crate::{ProtocolPackageFile, ProtocolPackageParseError, ProtocolPackageParseErrorCode};

/// Manifest 内相对 UTF-8 文件路径的最大字节数。
pub const MAX_PACKAGE_FILE_PATH_BYTES: usize = 256;
/// Manifest 顶层 Rhai 函数名的最大 ASCII 字节数。
pub const MAX_PROTOCOL_FUNCTION_NAME_BYTES: usize = 64;

/// 已通过跨平台相对路径校验的协议包内部文件名。
///
/// 本类型保存 `/` 分隔的 UTF-8 文本，不访问文件系统，也不执行规范化。它拒绝绝对路径、Windows
/// 反斜线/盘符、空段、`.`、`..`、控制字符和冒号；T07 仍需对真实 ZIP 条目执行更严格的重复项、
/// 大小写冲突和符号链接检查。
#[derive(Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct PackageFilePath(String);

impl PackageFilePath {
    /// 校验并创建协议包内相对路径。
    pub fn new(value: impl Into<String>) -> Result<Self, ProtocolPackageParseError> {
        Self::new_for_field(value.into(), "$file")
    }

    pub(crate) fn new_for_field(
        value: String,
        field: &str,
    ) -> Result<Self, ProtocolPackageParseError> {
        let segments_valid = value
            .split('/')
            .all(|segment| !segment.is_empty() && segment != "." && segment != "..");
        let characters_valid = value
            .chars()
            .all(|character| !character.is_control() && character != '\\' && character != ':');
        if value.is_empty()
            || value.len() > MAX_PACKAGE_FILE_PATH_BYTES
            || value.starts_with('/')
            || !segments_valid
            || !characters_valid
        {
            return Err(ProtocolPackageParseError::new(
                ProtocolPackageParseErrorCode::ManifestInvalid,
                ProtocolPackageFile::Manifest,
                field,
            ));
        }
        Ok(Self(value))
    }

    /// 返回 `/` 分隔的已校验包内路径。
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for PackageFilePath {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.as_str())
    }
}

/// Manifest 明确导出的顶层 Rhai 函数名。
///
/// Host API v1 将函数名限制为 ASCII 标识符 `[A-Za-z_][A-Za-z0-9_]*`，并与 Document 字段名
/// 共用 Rhai 关键字拒绝表。T08 还会使用真实 AST 校验函数存在、位于顶层且参数数量正确。
#[derive(Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct ProtocolFunctionName(String);

impl ProtocolFunctionName {
    /// 校验并创建顶层入口函数名。
    pub fn new(value: impl Into<String>) -> Result<Self, ProtocolPackageParseError> {
        Self::new_for_field(value.into(), "$function")
    }

    pub(crate) fn new_for_field(
        value: String,
        field: &str,
    ) -> Result<Self, ProtocolPackageParseError> {
        let mut bytes = value.bytes();
        let first_valid = bytes
            .next()
            .is_some_and(|byte| byte.is_ascii_alphabetic() || byte == b'_');
        if value.len() > MAX_PROTOCOL_FUNCTION_NAME_BYTES
            || !first_valid
            || !bytes.all(|byte| byte.is_ascii_alphanumeric() || byte == b'_')
            || is_rhai_reserved_word(&value)
        {
            return Err(ProtocolPackageParseError::new(
                ProtocolPackageParseErrorCode::ManifestInvalid,
                ProtocolPackageFile::Manifest,
                field,
            ));
        }
        Ok(Self(value))
    }

    /// 返回已校验的 Rhai 顶层函数名。
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for ProtocolFunctionName {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.as_str())
    }
}
