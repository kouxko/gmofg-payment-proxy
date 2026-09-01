//! 外部协议包持久化行模型及严格解析。
//!
//! SQL 和事务由父模块维护；本模块集中验证数据库标量、注册合同、指纹和脱敏错误历史。

use std::net::SocketAddr;

use chrono::{DateTime, Utc};
use intercept_proxy_package_contract::PackageManifest;

use super::{
    InfrastructureError, canonical_external_registration_fingerprint, corrupt_external_package,
};

/// 一条可在应用重启后恢复的外部协议包记录。
#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct StoredExternalPackage {
    /// 经过领域严格校验的完整注册合同。
    pub registration: PackageManifest,
    /// Proxy 对规范化注册合同计算的 SHA-256 指纹。
    pub fingerprint: [u8; 32],
    /// Exact imported Component for a Proxy-managed local runtime; remote packages store `None`.
    pub local_archive: Option<Vec<u8>>,
    /// 用户显式设置的启用位；与当前在线状态相互独立。
    pub enabled: bool,
    /// 该精确版本第一次注册成功的时间。
    pub first_connected_at: DateTime<Utc>,
    /// 该精确版本最近一次注册成功的时间。
    pub last_connected_at: DateTime<Utc>,
    /// 最近一次成功连接的 TCP 对端地址；不代表当前仍在线。
    pub remote_address: Option<SocketAddr>,
    /// 最近一次连接级错误的安全摘要；不保存 payload、远端 data 或密钥。
    pub recent_error: Option<StoredExternalPackageRecentError>,
}

/// 可安全持久化的最近连接错误。
#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct StoredExternalPackageRecentError {
    /// 稳定、可供 UI 分类的本地错误码。
    pub code: String,
    /// 固定的脱敏说明，不包含第三方返回内容。
    pub message: String,
    /// 本地观察到该错误的时间。
    pub occurred_at: DateTime<Utc>,
}

/// 持久化注册操作的无歧义结果。
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum StoredExternalPackageRegistrationOutcome {
    /// 首次出现该精确身份，已按默认停用插入。
    Inserted,
    /// 指纹与元数据完全一致，只更新最近连接时间。
    Reconnected {
        /// 注册前已持久化的用户启用位。
        enabled: bool,
    },
    /// 相同精确身份已有不同规范化注册内容，未修改任何数据。
    IdentityConflict,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum StoredLocalPackageInstallOutcome {
    Installed,
    Reused,
    IdentityConflict,
}

/// `external_protocol_packages` 查询的原始行形状。
pub(super) type ExternalPackageRow = (
    String,
    String,
    String,
    Vec<u8>,
    Option<Vec<u8>>,
    i64,
    String,
    String,
    Option<String>,
    Option<String>,
    Option<String>,
    Option<String>,
);

pub(super) fn read_external_package_row(
    row: &rusqlite::Row<'_>,
) -> rusqlite::Result<ExternalPackageRow> {
    Ok((
        row.get(0)?,
        row.get(1)?,
        row.get(2)?,
        row.get(3)?,
        row.get(4)?,
        row.get(5)?,
        row.get(6)?,
        row.get(7)?,
        row.get(8)?,
        row.get(9)?,
        row.get(10)?,
        row.get(11)?,
    ))
}

pub(super) fn parse_external_package_row(
    (
        package_id,
        version,
        registration_json,
        fingerprint,
        local_archive,
        enabled,
        first,
        last,
        remote_address,
        recent_error_code,
        recent_error_message,
        recent_error_occurred_at,
    ): ExternalPackageRow,
) -> Result<StoredExternalPackage, InfrastructureError> {
    let registration = serde_json::from_str::<PackageManifest>(&registration_json)
        .map_err(|error| corrupt_external_package(format!("registration_json 无效：{error}")))?;
    let identity = registration.package().identity();
    if identity.id.as_str() != package_id || identity.version.as_str() != version {
        return Err(corrupt_external_package(
            "索引身份与 registration_json 身份不一致",
        ));
    }
    let fingerprint: [u8; 32] = fingerprint
        .try_into()
        .map_err(|_| corrupt_external_package("registration_fingerprint 长度不是 32 字节"))?;
    if canonical_external_registration_fingerprint(&registration)? != fingerprint {
        return Err(corrupt_external_package(
            "registration_fingerprint 与规范化注册内容不一致",
        ));
    }
    let enabled = parse_enabled(enabled)?;
    let remote_address = remote_address
        .map(|value| {
            value.parse::<SocketAddr>().map_err(|error| {
                corrupt_external_package(format!("last_remote_address 无效：{error}"))
            })
        })
        .transpose()?;
    let recent_error = parse_recent_error(
        recent_error_code,
        recent_error_message,
        recent_error_occurred_at,
    )?;
    Ok(StoredExternalPackage {
        registration,
        fingerprint,
        local_archive,
        enabled,
        first_connected_at: parse_timestamp("first_connected_at", &first)?,
        last_connected_at: parse_timestamp("last_connected_at", &last)?,
        remote_address,
        recent_error,
    })
}

pub(super) fn validate_stable_error(code: &str, message: &str) -> Result<(), InfrastructureError> {
    let stable_message = match code {
        "EXTERNAL_PACKAGE_BUSY" => Some("外部软件包繁忙。"),
        "EXTERNAL_PACKAGE_TIMEOUT" => Some("外部软件包调用超时。"),
        "EXTERNAL_PACKAGE_DISCONNECTED" => Some("外部软件包连接已断开。"),
        "EXTERNAL_PACKAGE_REMOTE_ERROR" => Some("外部软件包返回 JSON-RPC 错误。"),
        "EXTERNAL_PACKAGE_MESSAGE_TOO_LARGE" => Some("外部软件包消息超过限制。"),
        "EXTERNAL_PACKAGE_INVALID_PAYLOAD" => Some("外部软件包 payload 无效。"),
        "EXTERNAL_PACKAGE_PROTOCOL_FATAL" => Some("外部软件包协议失效。"),
        "EXTERNAL_PACKAGE_TRANSPORT_ERROR" => Some("外部软件包传输失败。"),
        "EXTERNAL_PACKAGE_PROCESS_FAILED" => Some("本地软件包进程启动失败。"),
        _ => None,
    };
    if stable_message != Some(message) {
        return Err(corrupt_external_package(
            "recent_error 必须使用稳定的本地错误码与脱敏消息",
        ));
    }
    Ok(())
}

pub(super) fn parse_enabled(enabled: i64) -> Result<bool, InfrastructureError> {
    Ok(match enabled {
        0 => false,
        1 => true,
        _ => return Err(corrupt_external_package("enabled 不是严格布尔值")),
    })
}

fn parse_recent_error(
    code: Option<String>,
    message: Option<String>,
    occurred_at: Option<String>,
) -> Result<Option<StoredExternalPackageRecentError>, InfrastructureError> {
    match (code, message, occurred_at) {
        (None, None, None) => Ok(None),
        (Some(code), Some(message), Some(occurred_at)) => {
            validate_stable_error(&code, &message)?;
            Ok(Some(StoredExternalPackageRecentError {
                code,
                message,
                occurred_at: parse_timestamp("recent_error_occurred_at", &occurred_at)?,
            }))
        }
        _ => Err(corrupt_external_package(
            "recent_error 字段必须同时为空或同时有值",
        )),
    }
}

fn parse_timestamp(field: &str, value: &str) -> Result<DateTime<Utc>, InfrastructureError> {
    DateTime::parse_from_rfc3339(value)
        .map(|timestamp| timestamp.with_timezone(&Utc))
        .map_err(|error| corrupt_external_package(format!("{field} 无效：{error}")))
}
