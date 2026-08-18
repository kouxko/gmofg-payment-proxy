use crate::{DomainError, ErrorCode};
use serde::{Deserialize, Serialize};
use specta::Type;
use std::{collections::HashSet, fmt, str::FromStr};

/// Schema ID 的最大 ASCII 字节数。
pub const MAX_DOCUMENT_SCHEMA_ID_LEN: usize = 64;
/// 可成为规则变量的字段名最大 ASCII 字节数。
pub const MAX_DOCUMENT_FIELD_NAME_LEN: usize = 64;
/// Schema 标题和字段标签允许的最大 Unicode 字符数。
pub const MAX_DOCUMENT_DISPLAY_TEXT_LEN: usize = 128;
/// 单个 Schema 允许声明的字段上限。
pub const MAX_DOCUMENT_FIELDS: usize = 256;

pub(super) const RHAI_RESERVED_WORDS: &[&str] = &[
    "Fn",
    "as",
    "async",
    "await",
    "break",
    "call",
    "case",
    "catch",
    "const",
    "continue",
    "curry",
    "debug",
    "default",
    "do",
    "else",
    "eval",
    "export",
    "false",
    "fn",
    "for",
    "global",
    "go",
    "goto",
    "if",
    "import",
    "in",
    "is",
    "is_def_fn",
    "is_def_var",
    "is_shared",
    "let",
    "loop",
    "match",
    "module",
    "new",
    "nil",
    "null",
    "package",
    "print",
    "private",
    "protected",
    "public",
    "return",
    "shared",
    "spawn",
    "static",
    "super",
    "switch",
    "sync",
    "this",
    "thread",
    "throw",
    "true",
    "try",
    "type_of",
    "until",
    "use",
    "var",
    "void",
    "while",
    "with",
    "yield",
];

/// 判断名称是否属于 Host API v1 明确拒绝的 Rhai active/reserved 关键字。
///
/// Document 字段名和 Manifest 顶层函数名必须使用同一份关键字表，否则协议包可能在 Schema
/// 校验时通过，却在脚本编译或变量注册时失败。该函数只判断关键字；调用方仍需自行校验标识符形状。
#[must_use]
pub fn is_rhai_reserved_word(value: &str) -> bool {
    RHAI_RESERVED_WORDS.contains(&value)
}

fn invalid_schema(field: &str, message: impl Into<String>) -> DomainError {
    DomainError::new(ErrorCode::DocumentSchemaInvalid, "Document Schema 无效")
        .with_field_error(field, message)
}

fn valid_schema_id(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= MAX_DOCUMENT_SCHEMA_ID_LEN
        && value.as_bytes().first().is_some_and(u8::is_ascii_lowercase)
        && value
            .bytes()
            .skip(1)
            .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'-')
}

fn valid_field_name(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= MAX_DOCUMENT_FIELD_NAME_LEN
        && value.as_bytes().first().is_some_and(u8::is_ascii_lowercase)
        && value
            .bytes()
            .skip(1)
            .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'_')
        && !is_rhai_reserved_word(value)
}

fn valid_display_text(value: &str) -> bool {
    !value.trim().is_empty() && value.chars().count() <= MAX_DOCUMENT_DISPLAY_TEXT_LEN
}

#[derive(Clone, Debug, Deserialize, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize, Type)]
#[serde(try_from = "String", into = "String")]
/// 协议包内 Document Schema 的稳定 ID。
///
/// Wire 形式是匹配 `[a-z][a-z0-9-]*` 的字符串。它标识字段契约，不等同于协议包 ID。
pub struct DocumentSchemaId(String);

impl DocumentSchemaId {
    /// 校验并创建 Schema ID。
    pub fn new(value: impl Into<String>) -> Result<Self, DomainError> {
        let value = value.into();
        if valid_schema_id(&value) {
            Ok(Self(value))
        } else {
            Err(invalid_schema(
                "schema.id",
                "必须匹配 [a-z][a-z0-9-]*，且长度为 1 到 64 个 ASCII 字节",
            ))
        }
    }

    #[must_use]
    /// 返回已校验的 Schema ID 文本。
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl TryFrom<String> for DocumentSchemaId {
    type Error = DomainError;

    fn try_from(value: String) -> Result<Self, Self::Error> {
        Self::new(value)
    }
}

impl From<DocumentSchemaId> for String {
    fn from(value: DocumentSchemaId) -> Self {
        value.0
    }
}

impl FromStr for DocumentSchemaId {
    type Err = DomainError;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        Self::new(value)
    }
}

impl fmt::Display for DocumentSchemaId {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.as_str())
    }
}

#[derive(Clone, Debug, Deserialize, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize, Type)]
#[serde(try_from = "String", into = "String")]
/// Schema 字段名，同时也是脚本和 协议报文规则使用的稳定变量名。
/// 名称必须匹配 `[a-z][a-z0-9_]*`，并拒绝全部 Rhai active/reserved 关键字，
/// 防止同一个字段在 Schema 中合法、注册到脚本时却无法使用。
pub struct DocumentFieldName(String);

impl DocumentFieldName {
    /// 校验并创建字段变量名。
    pub fn new(value: impl Into<String>) -> Result<Self, DomainError> {
        let value = value.into();
        if valid_field_name(&value) {
            Ok(Self(value))
        } else {
            Err(invalid_schema(
                "schema.fields.name",
                "必须匹配 [a-z][a-z0-9_]*、长度为 1 到 64 个 ASCII 字节，且不能是 Rhai 保留字",
            ))
        }
    }

    #[must_use]
    /// 返回已校验的字段变量名。
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl TryFrom<String> for DocumentFieldName {
    type Error = DomainError;

    fn try_from(value: String) -> Result<Self, Self::Error> {
        Self::new(value)
    }
}

impl From<DocumentFieldName> for String {
    fn from(value: DocumentFieldName) -> Self {
        value.0
    }
}

impl FromStr for DocumentFieldName {
    type Err = DomainError;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        Self::new(value)
    }
}

impl fmt::Display for DocumentFieldName {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.as_str())
    }
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, Hash, PartialEq, Serialize, Type)]
#[serde(rename_all = "snake_case")]
/// Host API v1 支持的协议无关字段类型。
///
/// serde 形式固定为 `string`、`int`、`bool`、`blob`，不根据 JSON 数字或文本进行隐式转换。
pub enum DocumentFieldType {
    /// UTF-8 文本或编码后标识符。
    String,
    /// 有符号 64 位整数，金额通常用最小货币单位表达。
    Int,
    /// 布尔标志。
    Bool,
    /// 必须逐字节保真的未解释二进制值。
    Blob,
}

impl DocumentFieldType {
    #[must_use]
    /// 返回 Manifest/Schema 使用的稳定小写类型名。
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::String => "string",
            Self::Int => "int",
            Self::Bool => "bool",
            Self::Blob => "blob",
        }
    }
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize, Type)]
#[serde(try_from = "DocumentFieldWire", into = "DocumentFieldWire")]
/// 一个可由协议脚本赋值、UI 展示并被 协议报文规则引用的字段声明。
///
/// 此类型只声明允许的名称和类型，不表达字段是否必须出现；协议条件完整性属于脚本。
pub struct DocumentField {
    name: DocumentFieldName,
    #[serde(rename = "type")]
    field_type: DocumentFieldType,
    label: String,
}

#[derive(Clone, Debug, Deserialize, Serialize, Type)]
#[serde(deny_unknown_fields)]
// 严格 wire DTO：先拒绝未知键，再通过 TryFrom 回到 DocumentField::new，避免反序列化绕过校验。
struct DocumentFieldWire {
    name: DocumentFieldName,
    #[serde(rename = "type")]
    field_type: DocumentFieldType,
    label: String,
}

impl DocumentField {
    /// 创建字段声明，并校验 UI 标签非空且不超过字符上限。
    pub fn new(
        name: DocumentFieldName,
        field_type: DocumentFieldType,
        label: impl Into<String>,
    ) -> Result<Self, DomainError> {
        let label = label.into();
        if !valid_display_text(&label) {
            return Err(invalid_schema(
                "schema.fields.label",
                "必须包含可见字符，且最多 128 个 Unicode 字符",
            ));
        }
        Ok(Self {
            name,
            field_type,
            label,
        })
    }

    #[must_use]
    /// 返回脚本和规则使用的稳定变量名。
    pub const fn name(&self) -> &DocumentFieldName {
        &self.name
    }

    #[must_use]
    /// 返回写入该字段时必须匹配的值类型。
    pub const fn field_type(&self) -> DocumentFieldType {
        self.field_type
    }

    #[must_use]
    /// 返回只用于 UI 的可读标签。
    pub fn label(&self) -> &str {
        &self.label
    }
}

impl TryFrom<DocumentFieldWire> for DocumentField {
    type Error = DomainError;

    fn try_from(value: DocumentFieldWire) -> Result<Self, Self::Error> {
        Self::new(value.name, value.field_type, value.label)
    }
}

impl From<DocumentField> for DocumentFieldWire {
    fn from(value: DocumentField) -> Self {
        Self {
            name: value.name,
            field_type: value.field_type,
            label: value.label,
        }
    }
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize, Type)]
#[serde(try_from = "DocumentSchemaWire", into = "DocumentSchemaWire")]
/// 协议包提前声明的有序 Document 字段契约。
///
/// 字段顺序是 wire/UI 稳定契约，也是 [`crate::Document`] 值槽的索引来源。Schema 至少包含一个
/// 字段，且同一 Schema 内字段名不能重复。
pub struct DocumentSchema {
    id: DocumentSchemaId,
    version: u32,
    title: String,
    fields: Vec<DocumentField>,
}

#[derive(Clone, Debug, Deserialize, Serialize, Type)]
#[serde(deny_unknown_fields)]
// 聚合 wire DTO 同样不直接构造私有字段；TryFrom 会重跑版本、数量和唯一性校验。
struct DocumentSchemaWire {
    id: DocumentSchemaId,
    version: u32,
    title: String,
    fields: Vec<DocumentField>,
}

impl DocumentSchema {
    /// 校验版本、标题、字段数量和字段名唯一性后创建 Schema。
    ///
    /// `version` 是 Schema 自身的正整数版本，不是协议包 `SemVer`。
    pub fn new(
        id: DocumentSchemaId,
        version: u32,
        title: impl Into<String>,
        fields: Vec<DocumentField>,
    ) -> Result<Self, DomainError> {
        let title = title.into();
        if version == 0 {
            return Err(invalid_schema("schema.version", "必须大于 0"));
        }
        if !valid_display_text(&title) {
            return Err(invalid_schema(
                "schema.title",
                "必须包含可见字符，且最多 128 个 Unicode 字符",
            ));
        }
        if fields.is_empty() || fields.len() > MAX_DOCUMENT_FIELDS {
            return Err(invalid_schema(
                "schema.fields",
                "字段数量必须为 1 到 256 个",
            ));
        }
        let mut names = HashSet::with_capacity(fields.len());
        if let Some(duplicate) = fields
            .iter()
            .map(|field| field.name().as_str())
            .find(|name| !names.insert(*name))
        {
            return Err(invalid_schema(
                "schema.fields",
                format!("字段名 {duplicate} 重复"),
            ));
        }
        Ok(Self {
            id,
            version,
            title,
            fields,
        })
    }

    #[must_use]
    /// 返回 Schema 的稳定 ID。
    pub const fn id(&self) -> &DocumentSchemaId {
        &self.id
    }

    #[must_use]
    /// 返回大于零的 Schema 版本。
    pub const fn version(&self) -> u32 {
        self.version
    }

    #[must_use]
    /// 返回只用于 UI 的 Schema 标题。
    pub fn title(&self) -> &str {
        &self.title
    }

    #[must_use]
    /// 按作者声明顺序返回全部字段。
    pub fn fields(&self) -> &[DocumentField] {
        &self.fields
    }

    #[must_use]
    /// 查找字段对应的稳定值槽索引；未知字段返回 `None`。
    pub fn field_index(&self, name: &str) -> Option<usize> {
        self.fields
            .iter()
            .position(|field| field.name().as_str() == name)
    }
}

impl TryFrom<DocumentSchemaWire> for DocumentSchema {
    type Error = DomainError;

    fn try_from(value: DocumentSchemaWire) -> Result<Self, Self::Error> {
        Self::new(value.id, value.version, value.title, value.fields)
    }
}

impl From<DocumentSchema> for DocumentSchemaWire {
    fn from(value: DocumentSchema) -> Self {
        Self {
            id: value.id,
            version: value.version,
            title: value.title,
            fields: value.fields,
        }
    }
}
