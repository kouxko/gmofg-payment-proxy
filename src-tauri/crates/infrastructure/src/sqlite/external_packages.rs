//! 外部协议包注册元数据的 strict-current `SQLite` 仓储。
//!
//! 本模块只持久化可公开的注册合同、规范化指纹、用户启用位和脱敏连接历史。活动
//! WebSocket client、RPC 载荷与第三方内部状态严格留在内存中，避免应用重启后伪造在线状态。

use chrono::{DateTime, Utc};
use intercept_proxy_domain::{ExternalPackageRegistration, ProtocolPackageRef};
use rusqlite::{OptionalExtension, params};
use std::net::SocketAddr;

use super::{InfrastructureError, SqliteStore};

/// 一条可在应用重启后恢复的外部协议包记录。
#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct StoredExternalPackage {
    /// 经过领域严格校验的完整注册合同。
    pub registration: ExternalPackageRegistration,
    /// Proxy 对规范化注册合同计算的 SHA-256 指纹。
    pub fingerprint: [u8; 32],
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

impl SqliteStore {
    /// 删除外部包表以注入可重现的持久化故障。
    ///
    /// 仅供适配器错误投影测试使用，避免为测试扩大 `SQLite` 连接的生产可见性。
    #[cfg(test)]
    pub(crate) fn remove_external_package_table_for_test(&self) {
        self.connection
            .lock()
            .execute("DROP TABLE external_protocol_packages", [])
            .expect("external package table should exist");
    }

    /// 原子比较或插入一个外部协议包注册结果。
    ///
    /// 相同 `(id, version)` 只有指纹和规范化注册 JSON 同时相等才视为重连。双重比较防止
    /// 调用方传入错误指纹，也避免极低概率摘要碰撞静默改变 Schema 或方法映射。
    pub(crate) fn accept_external_package_registration(
        &self,
        registration: &ExternalPackageRegistration,
        fingerprint: [u8; 32],
        connected_at: DateTime<Utc>,
    ) -> Result<StoredExternalPackageRegistrationOutcome, InfrastructureError> {
        let identity = registration.package().identity();
        let registration_json = canonical_external_registration_json(registration)?;
        if sha256(registration_json.as_bytes()) != fingerprint {
            return Err(corrupt_external_package(
                "调用方提供的注册指纹与规范化内容不一致",
            ));
        }
        let mut connection = self.connection.lock();
        let transaction = connection.transaction().map_err(database_error)?;
        let internal_identity_exists = transaction
            .query_row(
                "SELECT EXISTS(
                    SELECT 1 FROM protocol_packages
                    WHERE package_id = ?1 AND version = ?2
                 )",
                params![identity.id.as_str(), identity.version.as_str()],
                |row| row.get::<_, bool>(0),
            )
            .map_err(database_error)?;
        if internal_identity_exists {
            // 两种执行来源不能共享同一精确身份，否则统一目录、Listener 绑定和删除语义会
            // 依赖查询顺序。冲突在任何外部表写入之前结束事务，保持 fail-closed。
            return Ok(StoredExternalPackageRegistrationOutcome::IdentityConflict);
        }
        let existing = transaction
            .query_row(
                "SELECT registration_json, registration_fingerprint, enabled
                 FROM external_protocol_packages
                 WHERE package_id = ?1 AND version = ?2",
                params![identity.id.as_str(), identity.version.as_str()],
                |row| {
                    Ok((
                        row.get::<_, String>(0)?,
                        row.get::<_, Vec<u8>>(1)?,
                        row.get::<_, i64>(2)?,
                    ))
                },
            )
            .optional()
            .map_err(database_error)?;

        let outcome = match existing {
            Some((stored_json, stored_fingerprint, enabled))
                if stored_json == registration_json && stored_fingerprint == fingerprint =>
            {
                let enabled = parse_enabled(enabled)?;
                transaction
                    .execute(
                        "UPDATE external_protocol_packages SET last_connected_at = ?3
                         WHERE package_id = ?1 AND version = ?2",
                        params![
                            identity.id.as_str(),
                            identity.version.as_str(),
                            connected_at.to_rfc3339(),
                        ],
                    )
                    .map_err(database_error)?;
                StoredExternalPackageRegistrationOutcome::Reconnected { enabled }
            }
            Some(_) => StoredExternalPackageRegistrationOutcome::IdentityConflict,
            None => {
                transaction
                    .execute(
                        "INSERT INTO external_protocol_packages(
                            package_id, version, registration_json, registration_fingerprint,
                            enabled, first_connected_at, last_connected_at
                         ) VALUES (?1, ?2, ?3, ?4, 0, ?5, ?5)",
                        params![
                            identity.id.as_str(),
                            identity.version.as_str(),
                            registration_json,
                            fingerprint.as_slice(),
                            connected_at.to_rfc3339(),
                        ],
                    )
                    .map_err(database_error)?;
                StoredExternalPackageRegistrationOutcome::Inserted
            }
        };
        transaction.commit().map_err(database_error)?;
        Ok(outcome)
    }

    /// 列出全部外部协议包精确版本，按身份稳定排序。
    pub(crate) fn list_external_packages(
        &self,
    ) -> Result<Vec<StoredExternalPackage>, InfrastructureError> {
        let connection = self.connection.lock();
        let mut statement = connection
            .prepare(
                "SELECT package_id, version, registration_json, registration_fingerprint,
                        enabled, first_connected_at, last_connected_at, last_remote_address,
                        recent_error_code, recent_error_message, recent_error_occurred_at
                 FROM external_protocol_packages ORDER BY package_id, version",
            )
            .map_err(database_error)?;
        let rows = statement
            .query_map([], |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, String>(2)?,
                    row.get::<_, Vec<u8>>(3)?,
                    row.get::<_, i64>(4)?,
                    row.get::<_, String>(5)?,
                    row.get::<_, String>(6)?,
                    row.get::<_, Option<String>>(7)?,
                    row.get::<_, Option<String>>(8)?,
                    row.get::<_, Option<String>>(9)?,
                    row.get::<_, Option<String>>(10)?,
                ))
            })
            .map_err(database_error)?;
        rows.map(|row| {
            row.map_err(database_error)
                .and_then(parse_external_package_row)
        })
        .collect()
    }

    /// 查询一个外部协议包精确版本。
    pub(crate) fn get_external_package(
        &self,
        package: &ProtocolPackageRef,
    ) -> Result<Option<StoredExternalPackage>, InfrastructureError> {
        let connection = self.connection.lock();
        connection
            .query_row(
                "SELECT package_id, version, registration_json, registration_fingerprint,
                        enabled, first_connected_at, last_connected_at, last_remote_address,
                        recent_error_code, recent_error_message, recent_error_occurred_at
                 FROM external_protocol_packages
                 WHERE package_id = ?1 AND version = ?2",
                params![package.id.as_str(), package.version.as_str()],
                |row| {
                    Ok((
                        row.get::<_, String>(0)?,
                        row.get::<_, String>(1)?,
                        row.get::<_, String>(2)?,
                        row.get::<_, Vec<u8>>(3)?,
                        row.get::<_, i64>(4)?,
                        row.get::<_, String>(5)?,
                        row.get::<_, String>(6)?,
                        row.get::<_, Option<String>>(7)?,
                        row.get::<_, Option<String>>(8)?,
                        row.get::<_, Option<String>>(9)?,
                        row.get::<_, Option<String>>(10)?,
                    ))
                },
            )
            .optional()
            .map_err(database_error)?
            .map(parse_external_package_row)
            .transpose()
    }

    /// 更新精确版本的用户启用位；返回记录是否存在。
    pub(crate) fn set_external_package_enabled(
        &self,
        package: &ProtocolPackageRef,
        enabled: bool,
    ) -> Result<bool, InfrastructureError> {
        self.connection
            .lock()
            .execute(
                "UPDATE external_protocol_packages SET enabled = ?3
                 WHERE package_id = ?1 AND version = ?2",
                params![package.id.as_str(), package.version.as_str(), enabled],
            )
            .map(|updated| updated == 1)
            .map_err(database_error)
    }

    /// 原子记录最近连接地址，并清除上一条已失效的连接错误。
    ///
    /// 地址来自 `TcpListener::accept` 的结构化值，不接收任意文本。清除与地址更新在同一条
    /// SQL 语句内完成，避免详情页观察到“新连接地址 + 旧连接错误”的中间状态。
    pub(crate) fn record_external_package_remote_address(
        &self,
        package: &ProtocolPackageRef,
        remote_address: SocketAddr,
    ) -> Result<bool, InfrastructureError> {
        self.connection
            .lock()
            .execute(
                "UPDATE external_protocol_packages
                 SET last_remote_address = ?3,
                     recent_error_code = NULL,
                     recent_error_message = NULL,
                     recent_error_occurred_at = NULL
                 WHERE package_id = ?1 AND version = ?2",
                params![
                    package.id.as_str(),
                    package.version.as_str(),
                    remote_address.to_string(),
                ],
            )
            .map(|updated| updated == 1)
            .map_err(database_error)
    }

    /// 原子记录最近连接错误的稳定、脱敏摘要。
    pub(crate) fn record_external_package_recent_error(
        &self,
        package: &ProtocolPackageRef,
        code: &str,
        message: &str,
        occurred_at: DateTime<Utc>,
    ) -> Result<bool, InfrastructureError> {
        validate_stable_error(code, message)?;
        self.connection
            .lock()
            .execute(
                "UPDATE external_protocol_packages
                 SET recent_error_code = ?3,
                     recent_error_message = ?4,
                     recent_error_occurred_at = ?5
                 WHERE package_id = ?1 AND version = ?2",
                params![
                    package.id.as_str(),
                    package.version.as_str(),
                    code,
                    message,
                    occurred_at.to_rfc3339(),
                ],
            )
            .map(|updated| updated == 1)
            .map_err(database_error)
    }

    /// 删除精确版本的持久化记录；不存在时幂等返回 `false`。
    pub(crate) fn delete_external_package(
        &self,
        package: &ProtocolPackageRef,
    ) -> Result<bool, InfrastructureError> {
        self.connection
            .lock()
            .execute(
                "DELETE FROM external_protocol_packages
                 WHERE package_id = ?1 AND version = ?2",
                params![package.id.as_str(), package.version.as_str()],
            )
            .map(|deleted| deleted == 1)
            .map_err(database_error)
    }
}

/// `external_protocol_packages` 查询的原始行形状。
///
/// 为持久化边界命名该结构，避免多个查询与严格解析器之间出现隐式列顺序分歧。
type ExternalPackageRow = (
    String,
    String,
    String,
    Vec<u8>,
    i64,
    String,
    String,
    Option<String>,
    Option<String>,
    Option<String>,
    Option<String>,
);

fn parse_external_package_row(
    (
        package_id,
        version,
        registration_json,
        fingerprint,
        enabled,
        first,
        last,
        remote_address,
        recent_error_code,
        recent_error_message,
        recent_error_occurred_at,
    ): ExternalPackageRow,
) -> Result<StoredExternalPackage, InfrastructureError> {
    let registration = serde_json::from_str::<ExternalPackageRegistration>(&registration_json)
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
        enabled,
        first_connected_at: parse_timestamp("first_connected_at", &first)?,
        last_connected_at: parse_timestamp("last_connected_at", &last)?,
        remote_address,
        recent_error,
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

fn validate_stable_error(code: &str, message: &str) -> Result<(), InfrastructureError> {
    let stable_message = match code {
        "EXTERNAL_PACKAGE_BUSY" => Some("外部软件包繁忙。"),
        "EXTERNAL_PACKAGE_TIMEOUT" => Some("外部软件包调用超时。"),
        "EXTERNAL_PACKAGE_DISCONNECTED" => Some("外部软件包连接已断开。"),
        "EXTERNAL_PACKAGE_REMOTE_ERROR" => Some("外部软件包返回 JSON-RPC 错误。"),
        "EXTERNAL_PACKAGE_MESSAGE_TOO_LARGE" => Some("外部软件包消息超过限制。"),
        "EXTERNAL_PACKAGE_INVALID_PAYLOAD" => Some("外部软件包 payload 无效。"),
        "EXTERNAL_PACKAGE_PROTOCOL_FATAL" => Some("外部软件包协议失效。"),
        "EXTERNAL_PACKAGE_TRANSPORT_ERROR" => Some("外部软件包传输失败。"),
        _ => None,
    };
    if stable_message != Some(message) {
        return Err(corrupt_external_package(
            "recent_error 必须使用稳定的本地错误码与脱敏消息",
        ));
    }
    Ok(())
}

/// 对严格注册对象的稳定 JSON 表达计算 SHA-256 指纹。
pub(crate) fn canonical_external_registration_fingerprint(
    registration: &ExternalPackageRegistration,
) -> Result<[u8; 32], InfrastructureError> {
    let json = canonical_external_registration_json(registration)?;
    Ok(sha256(json.as_bytes()))
}

fn canonical_external_registration_json(
    registration: &ExternalPackageRegistration,
) -> Result<String, InfrastructureError> {
    serde_json::to_string(registration).map_err(registration_serialization_error)
}

pub(super) fn registration_serialization_error(error: serde_json::Error) -> InfrastructureError {
    let message = format!("注册合同无法规范化序列化：{error}");
    // `Result::map_err` 按值交付错误；明确消费所有权，避免该边界被误读为可借用回调。
    drop(error);
    corrupt_external_package(message)
}

fn sha256(bytes: &[u8]) -> [u8; 32] {
    let digest = ring::digest::digest(&ring::digest::SHA256, bytes);
    let mut fingerprint = [0_u8; 32];
    fingerprint.copy_from_slice(digest.as_ref());
    fingerprint
}

fn parse_enabled(enabled: i64) -> Result<bool, InfrastructureError> {
    Ok(match enabled {
        0 => false,
        1 => true,
        _ => return Err(corrupt_external_package("enabled 不是严格布尔值")),
    })
}

fn parse_timestamp(field: &str, value: &str) -> Result<DateTime<Utc>, InfrastructureError> {
    DateTime::parse_from_rfc3339(value)
        .map(|timestamp| timestamp.with_timezone(&Utc))
        .map_err(|error| corrupt_external_package(format!("{field} 无效：{error}")))
}

fn corrupt_external_package(message: impl Into<String>) -> InfrastructureError {
    InfrastructureError::PersistenceCorrupt {
        entity: "external_protocol_package",
        message: message.into(),
    }
}

fn database_error(source: rusqlite::Error) -> InfrastructureError {
    InfrastructureError::Database { source }
}
