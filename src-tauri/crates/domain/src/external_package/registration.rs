use crate::{
    DocumentSchema, DomainError, ErrorCode, ProtocolPackageId, ProtocolPackageRef,
    ProtocolPackageVersion,
};
use serde::{Deserialize, Serialize};
use specta::Type;
use std::fmt;

/// 第一版外部软件包注册合同的唯一受支持 API 主版本。
pub const EXTERNAL_PACKAGE_API_V1: u32 = 1;
/// 方法后缀允许的最大 ASCII 字节数。
pub const MAX_EXTERNAL_METHOD_SUFFIX_LEN: usize = 64;
/// 外部软件包展示名称允许的最大 Unicode 字符数。
pub const MAX_EXTERNAL_PACKAGE_NAME_LEN: usize = 128;

fn invalid_registration(field: &str, message: impl Into<String>) -> DomainError {
    DomainError::new(ErrorCode::ProtocolPackageInvalid, "外部软件包注册合同无效")
        .with_field_error(field, message)
}

/// 外部软件包声明的方法后缀。
///
/// 后缀必须是长度为 1 到 64 的 ASCII 标识符 `[A-Za-z_][A-Za-z0-9_]*`。它不允许包含 `.`，
/// 因而第三方不能越出 Proxy 为当前位置分配的 JSON-RPC 命名空间。
#[derive(Clone, Debug, Deserialize, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize, Type)]
#[serde(try_from = "String", into = "String")]
pub struct ExternalPackageMethodSuffix(String);

impl ExternalPackageMethodSuffix {
    /// 校验并创建方法后缀。
    pub fn new(value: impl Into<String>) -> Result<Self, DomainError> {
        let value = value.into();
        let mut bytes = value.bytes();
        let first_valid = bytes
            .next()
            .is_some_and(|byte| byte.is_ascii_alphabetic() || byte == b'_');
        if value.len() > MAX_EXTERNAL_METHOD_SUFFIX_LEN
            || !first_valid
            || !bytes.all(|byte| byte.is_ascii_alphanumeric() || byte == b'_')
        {
            return Err(invalid_registration(
                "method",
                "方法后缀必须匹配 [A-Za-z_][A-Za-z0-9_]*，且长度为 1 到 64 个 ASCII 字节",
            ));
        }
        Ok(Self(value))
    }

    /// 返回已经校验的方法后缀。
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }

    /// 按指定领域位置生成实际 JSON-RPC 方法名。
    ///
    /// 命名空间由 Proxy 决定，第三方只控制最后一个后缀段。
    #[must_use]
    pub fn qualified(
        &self,
        namespace: ExternalPackageMethodNamespace,
        direction: ExternalPackageDirection,
    ) -> String {
        format!("{}.{}.{}", namespace.as_str(), direction.as_str(), self.0)
    }
}

impl TryFrom<String> for ExternalPackageMethodSuffix {
    type Error = DomainError;

    fn try_from(value: String) -> Result<Self, Self::Error> {
        Self::new(value)
    }
}

impl From<ExternalPackageMethodSuffix> for String {
    fn from(value: ExternalPackageMethodSuffix) -> Self {
        value.0
    }
}

impl fmt::Display for ExternalPackageMethodSuffix {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.as_str())
    }
}

/// 外部处理链的数据方向。
#[derive(Clone, Copy, Debug, Deserialize, Eq, Hash, PartialEq, Serialize, Type)]
#[serde(rename_all = "snake_case")]
pub enum ExternalPackageDirection {
    /// 应用到上游服务。
    Upstream,
    /// 上游服务到应用。
    Downstream,
}

impl ExternalPackageDirection {
    /// 返回 JSON-RPC 命名空间中的稳定方向段。
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Upstream => "upstream",
            Self::Downstream => "downstream",
        }
    }
}

/// 外部软件包方法所属的顶层 JSON-RPC 命名空间。
#[derive(Clone, Copy, Debug, Deserialize, Eq, Hash, PartialEq, Serialize, Type)]
#[serde(rename_all = "snake_case")]
pub enum ExternalPackageMethodNamespace {
    /// 报文边界、解码和编码处理入口。
    Hooks,
    /// Document 协议视图入口。
    Document,
}

impl ExternalPackageMethodNamespace {
    /// 返回 JSON-RPC 方法名使用的稳定顶层段。
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Hooks => "hooks",
            Self::Document => "document",
        }
    }
}

/// 外部软件包的精确身份与用户可读元数据。
#[derive(Clone, Debug, Eq, PartialEq, Type)]
pub struct ExternalPackageMetadata {
    identity: ProtocolPackageRef,
    name: String,
    description: String,
}

#[derive(Clone, Debug, Deserialize, Serialize, Type)]
#[serde(deny_unknown_fields)]
struct ExternalPackageMetadataWire {
    id: ProtocolPackageId,
    name: String,
    version: ProtocolPackageVersion,
    description: String,
}

impl ExternalPackageMetadata {
    /// 创建元数据，并保证展示名称包含可见字符且长度有界。
    pub fn new(
        identity: ProtocolPackageRef,
        name: impl Into<String>,
        description: impl Into<String>,
    ) -> Result<Self, DomainError> {
        let name = name.into();
        if name.trim().is_empty() || name.chars().count() > MAX_EXTERNAL_PACKAGE_NAME_LEN {
            return Err(invalid_registration(
                "package.name",
                "必须包含可见字符，且最多 128 个 Unicode 字符",
            ));
        }
        Ok(Self {
            identity,
            name,
            description: description.into(),
        })
    }

    /// 返回入口绑定和目录索引使用的精确 `(id, version)`。
    #[must_use]
    pub const fn identity(&self) -> &ProtocolPackageRef {
        &self.identity
    }

    /// 返回协议包列表使用的作者名称。
    #[must_use]
    pub fn name(&self) -> &str {
        &self.name
    }

    /// 返回作者提供的功能说明；空说明合法。
    #[must_use]
    pub fn description(&self) -> &str {
        &self.description
    }
}

impl TryFrom<ExternalPackageMetadataWire> for ExternalPackageMetadata {
    type Error = DomainError;

    fn try_from(value: ExternalPackageMetadataWire) -> Result<Self, Self::Error> {
        Self::new(
            ProtocolPackageRef {
                id: value.id,
                version: value.version,
            },
            value.name,
            value.description,
        )
    }
}

impl From<ExternalPackageMetadata> for ExternalPackageMetadataWire {
    fn from(value: ExternalPackageMetadata) -> Self {
        Self {
            id: value.identity.id,
            name: value.name,
            version: value.identity.version,
            description: value.description,
        }
    }
}

/// 单方向内联 Schema 与展示方法声明。
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize, Type)]
#[serde(deny_unknown_fields)]
pub struct ExternalPackageDocumentDirection {
    schema: DocumentSchema,
    display: ExternalPackageMethodSuffix,
}

impl ExternalPackageDocumentDirection {
    /// 返回该方向规则、解码和编码共用的 Schema。
    #[must_use]
    pub const fn schema(&self) -> &DocumentSchema {
        &self.schema
    }

    /// 返回该方向 `document.<direction>` 命名空间内的展示方法后缀。
    #[must_use]
    pub const fn display(&self) -> &ExternalPackageMethodSuffix {
        &self.display
    }
}

/// 上下行相互独立的内联 Document 声明。
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize, Type)]
#[serde(deny_unknown_fields)]
pub struct ExternalPackageDocuments {
    upstream: ExternalPackageDocumentDirection,
    downstream: ExternalPackageDocumentDirection,
}

impl ExternalPackageDocuments {
    /// 返回上行 Document 声明。
    #[must_use]
    pub const fn upstream(&self) -> &ExternalPackageDocumentDirection {
        &self.upstream
    }

    /// 返回下行 Document 声明。
    #[must_use]
    pub const fn downstream(&self) -> &ExternalPackageDocumentDirection {
        &self.downstream
    }
}

/// 单方向完整的 `frame → decode → encode` 方法后缀集合。
#[derive(Clone, Debug, Eq, PartialEq, Type)]
pub struct ExternalPackageDirectionHooks {
    frame: ExternalPackageMethodSuffix,
    decode: ExternalPackageMethodSuffix,
    encode: ExternalPackageMethodSuffix,
}

#[derive(Clone, Debug, Deserialize, Serialize, Type)]
#[serde(deny_unknown_fields)]
struct ExternalPackageDirectionHooksWire {
    frame: ExternalPackageMethodSuffix,
    decode: ExternalPackageMethodSuffix,
    encode: ExternalPackageMethodSuffix,
}

impl ExternalPackageDirectionHooks {
    /// 创建方法集合，并拒绝同一 `hooks.<direction>` 命名空间中的后缀冲突。
    pub fn new(
        frame: ExternalPackageMethodSuffix,
        decode: ExternalPackageMethodSuffix,
        encode: ExternalPackageMethodSuffix,
    ) -> Result<Self, DomainError> {
        if frame == decode || frame == encode || decode == encode {
            return Err(invalid_registration(
                "hooks",
                "同一方向的 frame、decode、encode 方法后缀不得重复",
            ));
        }
        Ok(Self {
            frame,
            decode,
            encode,
        })
    }

    /// 返回报文边界识别方法后缀。
    #[must_use]
    pub const fn frame(&self) -> &ExternalPackageMethodSuffix {
        &self.frame
    }

    /// 返回报文解码方法后缀。
    #[must_use]
    pub const fn decode(&self) -> &ExternalPackageMethodSuffix {
        &self.decode
    }

    /// 返回 Document 编码方法后缀。
    #[must_use]
    pub const fn encode(&self) -> &ExternalPackageMethodSuffix {
        &self.encode
    }
}

impl TryFrom<ExternalPackageDirectionHooksWire> for ExternalPackageDirectionHooks {
    type Error = DomainError;

    fn try_from(value: ExternalPackageDirectionHooksWire) -> Result<Self, Self::Error> {
        Self::new(value.frame, value.decode, value.encode)
    }
}

impl From<ExternalPackageDirectionHooks> for ExternalPackageDirectionHooksWire {
    fn from(value: ExternalPackageDirectionHooks) -> Self {
        Self {
            frame: value.frame,
            decode: value.decode,
            encode: value.encode,
        }
    }
}

/// 上下行互相隔离的 Hook 方法声明。
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize, Type)]
#[serde(deny_unknown_fields)]
pub struct ExternalPackageHooks {
    upstream: ExternalPackageDirectionHooks,
    downstream: ExternalPackageDirectionHooks,
}

impl ExternalPackageHooks {
    /// 返回上行 Hook 声明。
    #[must_use]
    pub const fn upstream(&self) -> &ExternalPackageDirectionHooks {
        &self.upstream
    }

    /// 返回下行 Hook 声明。
    #[must_use]
    pub const fn downstream(&self) -> &ExternalPackageDirectionHooks {
        &self.downstream
    }
}

/// 通过严格 JSON 和领域校验后的外部软件包注册结果。
#[derive(Clone, Debug, Eq, PartialEq, Type)]
pub struct ExternalPackageRegistration {
    api: u32,
    package: ExternalPackageMetadata,
    document: ExternalPackageDocuments,
    hooks: ExternalPackageHooks,
}

#[derive(Clone, Debug, Deserialize, Serialize, Type)]
#[serde(deny_unknown_fields)]
struct ExternalPackageRegistrationWire {
    api: u32,
    package: ExternalPackageMetadataWire,
    document: ExternalPackageDocuments,
    hooks: ExternalPackageHooks,
}

impl ExternalPackageRegistration {
    /// 创建注册对象；第一版只接受 [`EXTERNAL_PACKAGE_API_V1`]。
    pub fn new(
        api: u32,
        package: ExternalPackageMetadata,
        document: ExternalPackageDocuments,
        hooks: ExternalPackageHooks,
    ) -> Result<Self, DomainError> {
        if api != EXTERNAL_PACKAGE_API_V1 {
            return Err(invalid_registration(
                "api",
                format!("仅支持 API {EXTERNAL_PACKAGE_API_V1}"),
            ));
        }
        Ok(Self {
            api,
            package,
            document,
            hooks,
        })
    }

    /// 返回已确认受支持的 API 主版本。
    #[must_use]
    pub const fn api(&self) -> u32 {
        self.api
    }

    /// 返回精确身份和展示元数据。
    #[must_use]
    pub const fn package(&self) -> &ExternalPackageMetadata {
        &self.package
    }

    /// 返回上下行内联 Schema 与展示方法。
    #[must_use]
    pub const fn document(&self) -> &ExternalPackageDocuments {
        &self.document
    }

    /// 返回上下行 Hook 方法。
    #[must_use]
    pub const fn hooks(&self) -> &ExternalPackageHooks {
        &self.hooks
    }
}

impl TryFrom<ExternalPackageRegistrationWire> for ExternalPackageRegistration {
    type Error = DomainError;

    fn try_from(value: ExternalPackageRegistrationWire) -> Result<Self, Self::Error> {
        Self::new(
            value.api,
            value.package.try_into()?,
            value.document,
            value.hooks,
        )
    }
}

impl From<ExternalPackageRegistration> for ExternalPackageRegistrationWire {
    fn from(value: ExternalPackageRegistration) -> Self {
        Self {
            api: value.api,
            package: value.package.into(),
            document: value.document,
            hooks: value.hooks,
        }
    }
}

impl<'de> Deserialize<'de> for ExternalPackageMetadata {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        ExternalPackageMetadataWire::deserialize(deserializer)?
            .try_into()
            .map_err(serde::de::Error::custom)
    }
}

impl Serialize for ExternalPackageMetadata {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        ExternalPackageMetadataWire::from(self.clone()).serialize(serializer)
    }
}

impl<'de> Deserialize<'de> for ExternalPackageDirectionHooks {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        ExternalPackageDirectionHooksWire::deserialize(deserializer)?
            .try_into()
            .map_err(serde::de::Error::custom)
    }
}

impl Serialize for ExternalPackageDirectionHooks {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        ExternalPackageDirectionHooksWire::from(self.clone()).serialize(serializer)
    }
}

impl<'de> Deserialize<'de> for ExternalPackageRegistration {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        ExternalPackageRegistrationWire::deserialize(deserializer)?
            .try_into()
            .map_err(serde::de::Error::custom)
    }
}

impl Serialize for ExternalPackageRegistration {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        ExternalPackageRegistrationWire::from(self.clone()).serialize(serializer)
    }
}
