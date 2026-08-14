use crate::{DomainError, ErrorCode};
use serde::{Deserialize, Serialize};
use specta::Type;
use std::{fmt, str::FromStr};

/// 协议包 ID 的最大 ASCII 字节数。
pub const MAX_PROTOCOL_PACKAGE_ID_LEN: usize = 64;
/// 协议包 `SemVer` 文本的最大 ASCII 字节数。
pub const MAX_PROTOCOL_PACKAGE_VERSION_LEN: usize = 128;

fn invalid_identity(field: &str, message: impl Into<String>) -> DomainError {
    DomainError::new(ErrorCode::ProtocolPackageInvalid, "协议包身份无效")
        .with_field_error(field, message)
}

fn valid_kebab_id(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= MAX_PROTOCOL_PACKAGE_ID_LEN
        && value.as_bytes().first().is_some_and(u8::is_ascii_lowercase)
        && value
            .bytes()
            .skip(1)
            .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'-')
}

#[derive(Clone, Debug, Deserialize, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize, Type)]
#[serde(try_from = "String", into = "String")]
/// 应用级协议包的稳定 ID。
/// Wire 形式是字符串，必须匹配 `[a-z][a-z0-9-]*`；小写限制保证跨平台文件系统、
/// ZIP 条目和数据库查询使用同一规范形式。
pub struct ProtocolPackageId(String);

impl ProtocolPackageId {
    /// 校验并创建协议包 ID。
    ///
    /// 非 ASCII、首字符不是小写字母或超过 [`MAX_PROTOCOL_PACKAGE_ID_LEN`] 时返回
    /// [`ErrorCode::ProtocolPackageInvalid`]。
    pub fn new(value: impl Into<String>) -> Result<Self, DomainError> {
        let value = value.into();
        if valid_kebab_id(&value) {
            Ok(Self(value))
        } else {
            Err(invalid_identity(
                "package.id",
                "必须匹配 [a-z][a-z0-9-]*，且长度为 1 到 64 个 ASCII 字节",
            ))
        }
    }

    #[must_use]
    /// 返回已经规范化校验的 ID 文本，不进行分配。
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl TryFrom<String> for ProtocolPackageId {
    type Error = DomainError;

    fn try_from(value: String) -> Result<Self, Self::Error> {
        Self::new(value)
    }
}

impl From<ProtocolPackageId> for String {
    fn from(value: ProtocolPackageId) -> Self {
        value.0
    }
}

impl FromStr for ProtocolPackageId {
    type Err = DomainError;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        Self::new(value)
    }
}

impl fmt::Display for ProtocolPackageId {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.as_str())
    }
}

#[derive(Clone, Debug, Deserialize, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize, Type)]
#[serde(try_from = "String", into = "String")]
/// 协议包不可变版本号。
/// 保存作者提供的完整 `SemVer` 文本，包括 prerelease 和 build metadata；Listener 后续必须绑定
/// 此精确值，不做范围匹配或自动升级。
pub struct ProtocolPackageVersion(String);

impl ProtocolPackageVersion {
    /// 使用 `SemVer` 解析器校验并创建精确版本。
    pub fn new(value: impl Into<String>) -> Result<Self, DomainError> {
        let value = value.into();
        if value.is_empty() || value.len() > MAX_PROTOCOL_PACKAGE_VERSION_LEN {
            return Err(invalid_identity(
                "package.version",
                "SemVer 长度必须为 1 到 128 个 ASCII 字节",
            ));
        }
        semver::Version::parse(&value).map_err(|_| {
            invalid_identity(
                "package.version",
                "必须是完整 SemVer，例如 1.2.3 或 1.2.3-beta.1+build.7",
            )
        })?;
        Ok(Self(value))
    }

    #[must_use]
    /// 返回原始且已通过 `SemVer` 校验的版本文本。
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl TryFrom<String> for ProtocolPackageVersion {
    type Error = DomainError;

    fn try_from(value: String) -> Result<Self, Self::Error> {
        Self::new(value)
    }
}

impl From<ProtocolPackageVersion> for String {
    fn from(value: ProtocolPackageVersion) -> Self {
        value.0
    }
}

impl FromStr for ProtocolPackageVersion {
    type Err = DomainError;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        Self::new(value)
    }
}

impl fmt::Display for ProtocolPackageVersion {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.as_str())
    }
}

#[derive(Clone, Debug, Deserialize, Eq, Hash, PartialEq, Serialize, Type)]
#[serde(deny_unknown_fields)]
/// Listener/Workspace 引用协议包时使用的精确身份。
/// ID 与版本缺一不可；该类型不表达版本范围，也不负责查询包是否已安装或启用。
pub struct ProtocolPackageRef {
    /// 应用级协议包 ID。
    pub id: ProtocolPackageId,
    /// 必须精确匹配的协议包版本。
    pub version: ProtocolPackageVersion,
}
