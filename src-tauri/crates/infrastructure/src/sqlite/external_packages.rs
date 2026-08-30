//! 外部协议包注册元数据的 strict-current `SQLite` 仓储。
//!
//! 本模块只持久化可公开的注册合同、规范化指纹、用户启用位和脱敏连接历史。活动
//! WebSocket client、RPC 载荷与第三方内部状态严格留在内存中，避免应用重启后伪造在线状态。

use chrono::{DateTime, Utc};
use intercept_proxy_domain::ProtocolPackageRef;
use intercept_proxy_package_contract::PackageManifest;
use rusqlite::{OptionalExtension, params};
use std::net::SocketAddr;

use super::{InfrastructureError, SqliteStore};

#[path = "external_packages/fingerprint.rs"]
mod fingerprint;
pub(crate) use fingerprint::canonical_external_registration_fingerprint;
use fingerprint::{canonical_external_registration_json, sha256};
#[path = "external_packages/rows.rs"]
mod rows;
pub(crate) use rows::{StoredExternalPackage, StoredExternalPackageRegistrationOutcome};
use rows::{
    parse_enabled, parse_external_package_row, read_external_package_row, validate_stable_error,
};

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
        registration: &PackageManifest,
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
            .query_map([], read_external_package_row)
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
                read_external_package_row,
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

fn database_error(source: rusqlite::Error) -> InfrastructureError {
    InfrastructureError::Database { source }
}

fn corrupt_external_package(message: impl Into<String>) -> InfrastructureError {
    InfrastructureError::PersistenceCorrupt {
        entity: "external_protocol_package",
        message: message.into(),
    }
}

#[cfg(test)]
pub(super) fn registration_serialization_error(error: serde_json::Error) -> InfrastructureError {
    fingerprint::registration_serialization_error(error)
}
