//! `SQLite` 持久化边界：保存设置、Workspace 和加密后的证书材料。
//!
//! 本模块用单连接互斥锁串行化事务，并用 revision 做 CAS（比较后更新）：调用方必须带上
//! 自己读到的版本，版本不一致就返回冲突，避免两个窗口或后台任务静默覆盖彼此的数据。
//! 数据库只保存密文证书材料；明文私钥不应越过上层的短生命周期内存边界。

use std::path::Path;

use chrono::{DateTime, Utc};
use parking_lot::Mutex;
use rusqlite::{Connection, OptionalExtension, Transaction, TransactionBehavior, params};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use uuid::Uuid;

use crate::InfrastructureError;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct StoredSettings {
    pub revision: u64,
    pub value: Value,
    pub updated_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct CertificateMaterialRecord {
    pub kind: String,
    pub protected_blob: Vec<u8>,
    pub metadata: Value,
    pub updated_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct CertificateMaterialSnapshot {
    pub revision: u64,
    pub records: Vec<CertificateMaterialRecord>,
}

/// Workspace 的持久化快照。
///
/// `value` 保存完整但不含秘密明文的领域文档；`revision` 单独列出，便于 `SQLite` 在
/// 不解析 JSON 的情况下执行乐观锁。真正的私钥和口令只会以安全引用出现在文档中。
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct WorkspaceRecord {
    pub id: Uuid,
    pub revision: u64,
    pub value: Value,
    pub updated_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct WorkspaceCollectionSnapshot {
    pub selected_id: Option<Uuid>,
    pub records: Vec<WorkspaceRecord>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ProtectedSecretRecord {
    pub provider: String,
    pub key: String,
    pub protected_blob: Vec<u8>,
    pub updated_at: DateTime<Utc>,
}

#[derive(Debug)]
pub struct SqliteStore {
    // rusqlite 的 Connection 不是并发事务池；单锁明确规定本进程内只有一个事务所有者。
    connection: Mutex<Connection>,
    // 所有由该 Store 派生的异步执行器共享同一个准入门，避免阻塞线程池堆积等待连接锁。
    blocking_gate: std::sync::Arc<tokio::sync::Semaphore>,
}

#[cfg(test)]
impl SqliteStore {
    pub(crate) fn execute_test_batch(&self, sql: &str) -> Result<(), InfrastructureError> {
        self.connection
            .lock()
            .execute_batch(sql)
            .map_err(|source| InfrastructureError::Database { source })
    }
}
mod android_runtime_owner;
mod certificates;
mod core;
pub use android_runtime_owner::AndroidRuntimeOwnerRecord;
mod environment_configuration;
mod environment_configuration_baseline;
mod executor;
pub use environment_configuration::EnvironmentCommitFaultPoint;
pub(crate) use environment_configuration::EnvironmentConfigurationCommitAdapter;
pub use executor::{IntoSqlitePersistence, SqliteExecutor, open_sqlite_persistence};
pub(crate) mod external_packages;
mod portable_configuration;
pub(crate) mod protocol_packages;
mod schema;

/// 当前预发布数据库格式版本。低于该版本的数据由 Host 清空后重建。
pub const CURRENT_APPLICATION_SCHEMA_VERSION: i64 = schema::CURRENT_SCHEMA_VERSION;
mod workspaces;

use schema::create_current_schema;

fn stored_certificate_revision(transaction: &Transaction<'_>) -> Result<u64, InfrastructureError> {
    let mut statement = transaction
        .prepare("SELECT metadata_json FROM certificate_material")
        .map_err(|source| InfrastructureError::DatabaseSchema { source })?;
    let metadata = statement
        .query_map([], |row| row.get::<_, String>(0))
        .map_err(|source| InfrastructureError::DatabaseSchema { source })?;
    let mut revision = 0;
    for value in metadata {
        let value = value.map_err(|source| InfrastructureError::DatabaseSchema { source })?;
        let candidate = serde_json::from_str::<Value>(&value)
            .ok()
            .and_then(|value| value.get("revision").and_then(Value::as_u64))
            .unwrap_or(0);
        revision = revision.max(candidate);
    }
    Ok(revision)
}

fn initialize_singleton_state(
    transaction: &Transaction<'_>,
    certificate_revision: u64,
) -> Result<(), InfrastructureError> {
    transaction
        .execute(
            "INSERT OR IGNORE INTO certificate_state(singleton_id, revision) VALUES (1, ?1)",
            [revision_to_i64(certificate_revision)?],
        )
        .map_err(|source| InfrastructureError::DatabaseSchema { source })?;
    transaction
        .execute(
            "INSERT OR IGNORE INTO workspace_state(singleton_id, selected_id)
             VALUES (1, NULL)",
            [],
        )
        .map_err(|source| InfrastructureError::DatabaseSchema { source })?;
    Ok(())
}

fn put_certificate_material(
    connection: &Connection,
    record: &CertificateMaterialRecord,
) -> Result<(), InfrastructureError> {
    connection
        .execute(
            "INSERT INTO certificate_material(kind, protected_blob, metadata_json, updated_at)
             VALUES (?1, ?2, ?3, ?4)
             ON CONFLICT(kind) DO UPDATE SET
                protected_blob = excluded.protected_blob,
                metadata_json = excluded.metadata_json,
                updated_at = excluded.updated_at",
            params![
                record.kind,
                record.protected_blob,
                record.metadata.to_string(),
                record.updated_at.to_rfc3339()
            ],
        )
        .map_err(|source| InfrastructureError::Database { source })?;
    Ok(())
}

fn load_certificate_material(
    connection: &Connection,
    kind: &str,
) -> Result<Option<CertificateMaterialRecord>, InfrastructureError> {
    connection
        .query_row(
            "SELECT protected_blob, metadata_json, updated_at
             FROM certificate_material WHERE kind = ?1",
            [kind],
            |row| {
                Ok((
                    row.get::<_, Vec<u8>>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, String>(2)?,
                ))
            },
        )
        .optional()
        .map_err(|source| InfrastructureError::Database { source })?
        .map(|(protected_blob, metadata, updated_at)| {
            Ok(CertificateMaterialRecord {
                kind: kind.to_owned(),
                protected_blob,
                metadata: serde_json::from_str(&metadata).map_err(|error| {
                    InfrastructureError::CertificateInvalid {
                        message: format!("证书元数据无效：{error}"),
                    }
                })?,
                updated_at: DateTime::parse_from_rfc3339(&updated_at)
                    .map_err(|error| InfrastructureError::CertificateInvalid {
                        message: format!("证书元数据时间无效：{error}"),
                    })?
                    .with_timezone(&Utc),
            })
        })
        .transpose()
}

fn current_revision(
    transaction: &Transaction<'_>,
    table: &str,
    predicate: &str,
) -> Result<Option<u64>, InfrastructureError> {
    let sql = format!("SELECT revision FROM {table} WHERE {predicate}");
    transaction
        .query_row(&sql, [], |row| row.get::<_, i64>(0))
        .optional()
        .map_err(|source| InfrastructureError::Database { source })?
        .map(revision_from_i64)
        .transpose()
}

fn parse_settings_row(
    (revision, json, updated_at): (i64, String, String),
) -> Result<StoredSettings, InfrastructureError> {
    Ok(StoredSettings {
        revision: u64::try_from(revision)
            .map_err(|_| settings_record_corrupt("revision 不能为负数"))?,
        value: serde_json::from_str(&json)
            .map_err(|error| settings_record_corrupt(format!("JSON 无效：{error}")))?,
        updated_at: DateTime::parse_from_rfc3339(&updated_at)
            .map_err(|error| settings_record_corrupt(format!("时间无效：{error}")))?
            .with_timezone(&Utc),
    })
}

fn revision_to_i64(revision: u64) -> Result<i64, InfrastructureError> {
    i64::try_from(revision).map_err(|_| InfrastructureError::RevisionConflict)
}

fn revision_from_i64(revision: i64) -> Result<u64, InfrastructureError> {
    u64::try_from(revision).map_err(|_| InfrastructureError::RevisionConflict)
}

fn settings_record_corrupt(message: impl Into<String>) -> InfrastructureError {
    InfrastructureError::PersistenceCorrupt {
        entity: "settings",
        message: message.into(),
    }
}

fn load_workspace_records(
    transaction: &Transaction<'_>,
) -> Result<Vec<WorkspaceRecord>, InfrastructureError> {
    let mut statement = transaction
        .prepare("SELECT id, revision, json, updated_at FROM workspaces ORDER BY updated_at, id")
        .map_err(|source| InfrastructureError::Database { source })?;
    let rows = statement
        .query_map([], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, i64>(1)?,
                row.get::<_, String>(2)?,
                row.get::<_, String>(3)?,
            ))
        })
        .map_err(|source| InfrastructureError::Database { source })?;
    rows.map(|row| {
        let (id, revision, json, updated_at) =
            row.map_err(|source| InfrastructureError::Database { source })?;
        let id = Uuid::parse_str(&id).map_err(|error| InfrastructureError::PersistenceCorrupt {
            entity: "workspace",
            message: format!("id 无效：{error}"),
        })?;
        parse_workspace_record(id, revision, &json, &updated_at)
    })
    .collect()
}

fn parse_workspace_record(
    id: Uuid,
    revision: i64,
    json: &str,
    updated_at: &str,
) -> Result<WorkspaceRecord, InfrastructureError> {
    Ok(WorkspaceRecord {
        id,
        revision: u64::try_from(revision).map_err(|_| InfrastructureError::PersistenceCorrupt {
            entity: "workspace",
            message: "revision 不能为负数".into(),
        })?,
        value: serde_json::from_str(json).map_err(|error| {
            InfrastructureError::PersistenceCorrupt {
                entity: "workspace",
                message: format!("JSON 无效：{error}"),
            }
        })?,
        updated_at: DateTime::parse_from_rfc3339(updated_at)
            .map_err(|error| InfrastructureError::PersistenceCorrupt {
                entity: "workspace",
                message: format!("updated_at 无效：{error}"),
            })?
            .with_timezone(&Utc),
    })
}

#[cfg(test)]
#[path = "sqlite/tests/mod.rs"]
mod sqlite_tests;
