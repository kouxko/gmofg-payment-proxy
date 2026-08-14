use std::fmt;

use serde::Serialize;
use thiserror::Error;

/// 协议声明解析失败时对应的逻辑文件。
///
/// 这里故意不保存本机路径。导入器即使从临时目录解析协议包，也只能向 UI 暴露协议包内部的
/// 逻辑文件名，避免泄漏用户目录或应用缓存位置。
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ProtocolPackageFile {
    /// ZIP 根目录的 `manifest.toml`。
    Manifest,
    /// Manifest 引用的 Document Schema TOML。
    DocumentSchema,
}

impl ProtocolPackageFile {
    /// 返回面向作者诊断的稳定逻辑文件名。
    #[must_use]
    pub const fn file_name(self) -> &'static str {
        match self {
            Self::Manifest => "manifest.toml",
            Self::DocumentSchema => "document.toml",
        }
    }
}

impl fmt::Display for ProtocolPackageFile {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.file_name())
    }
}

/// Manifest/Schema 解析器对外提供的稳定错误分类。
///
/// TOML 库的原始错误文本不是产品契约，也可能包含输入片段。因此跨层调用只依赖这些代码、逻辑文件和
/// 字段路径；底层错误不会成为 [`std::error::Error::source`]。
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum ProtocolPackageParseErrorCode {
    /// TOML 语法、字段类型、未知键或必需键无效。
    TomlInvalid,
    /// 单个声明文件超过解析器的独立字节上限。
    InputTooLarge,
    /// Manifest 请求了宿主不支持的 API 版本。
    UnsupportedHostApi,
    /// Manifest 的身份、显示文本、路径或函数声明不合法。
    ManifestInvalid,
    /// Document Schema 未通过领域模型校验。
    DocumentSchemaInvalid,
    /// Manifest 引用的 Schema 或脚本没有出现在协议包文件集合中。
    ReferencedFileMissing,
    /// 协议包缺少导入流程要求的固定文件。
    RequiredFileMissing,
}

impl ProtocolPackageParseErrorCode {
    /// 返回可持久化、无需解析中文消息的稳定机器码。
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::TomlInvalid => "TOML_INVALID",
            Self::InputTooLarge => "INPUT_TOO_LARGE",
            Self::UnsupportedHostApi => "UNSUPPORTED_HOST_API",
            Self::ManifestInvalid => "MANIFEST_INVALID",
            Self::DocumentSchemaInvalid => "DOCUMENT_SCHEMA_INVALID",
            Self::ReferencedFileMissing => "REFERENCED_FILE_MISSING",
            Self::RequiredFileMissing => "REQUIRED_FILE_MISSING",
        }
    }
}

impl fmt::Display for ProtocolPackageParseErrorCode {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.as_str())
    }
}

/// 严格 Manifest/Schema 解析的脱敏错误。
///
/// `field` 是长度受限的 Serde 路径，例如 `hooks.upstream.receive.frame` 或
/// `fields[0].type`。解析器不会把无效值、脚本源码、TOML 行文本或绝对路径放入本类型。
#[derive(Clone, Debug, Eq, Error, PartialEq, Serialize)]
#[error("{file} 的 {field} 无效（{code}）")]
pub struct ProtocolPackageParseError {
    code: ProtocolPackageParseErrorCode,
    file: ProtocolPackageFile,
    field: String,
}

impl ProtocolPackageParseError {
    /// 返回稳定错误分类。
    #[must_use]
    pub const fn code(&self) -> ProtocolPackageParseErrorCode {
        self.code
    }

    /// 返回不包含本机目录的逻辑文件。
    #[must_use]
    pub const fn file(&self) -> ProtocolPackageFile {
        self.file
    }

    /// 返回受长度与字符集约束的字段路径。
    #[must_use]
    pub fn field(&self) -> &str {
        &self.field
    }

    /// 构造固定文件缺失错误，供后续安全 ZIP 读取器复用。
    #[must_use]
    pub fn required_file_missing(file: ProtocolPackageFile) -> Self {
        Self::new(
            ProtocolPackageParseErrorCode::RequiredFileMissing,
            file,
            "$",
        )
    }

    pub(crate) fn new(
        code: ProtocolPackageParseErrorCode,
        file: ProtocolPackageFile,
        field: &str,
    ) -> Self {
        Self {
            code,
            file,
            field: sanitize_field_path(field),
        }
    }
}

// Serde 路径只需要表达 struct 字段和数组索引。未知键可能完全由攻击者控制，所以非 ASCII、控制字符、
// 路径分隔符或超长值统一折叠为根节点，既保留定位能力又不回显输入。
fn sanitize_field_path(field: &str) -> String {
    const MAX_FIELD_PATH_BYTES: usize = 160;

    if field.is_empty()
        || field.len() > MAX_FIELD_PATH_BYTES
        || !field
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || b"_.$[]".contains(&byte))
    {
        "$".to_owned()
    } else {
        field.to_owned()
    }
}
