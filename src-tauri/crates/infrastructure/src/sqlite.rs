//! `SQLite` 持久化边界：保存设置、规则运行态和加密后的证书材料。
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

const LATEST_SCHEMA_VERSION: i64 = 6;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct StoredSettings {
    pub revision: u64,
    pub value: Value,
    pub updated_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct RuleRecord {
    pub id: Uuid,
    pub revision: u64,
    pub enabled: bool,
    pub value: Value,
    pub updated_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct RuleCollectionSnapshot {
    pub revision: u64,
    pub records: Vec<RuleRecord>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct RuleRuntimeUpdate {
    pub id: Uuid,
    pub expected_revision: u64,
    pub revision: u64,
    pub enabled: bool,
    pub hit_count: u64,
    pub last_hit_at: Option<DateTime<Utc>>,
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
    // 不要在持锁期间调用外部 async 服务，否则会把一次慢 I/O 放大成所有持久化阻塞。
    connection: Mutex<Connection>,
}

mod core;
pub(crate) mod protocol_packages;
mod rules_and_certificates;
mod workspaces;

fn create_schema(transaction: &Transaction<'_>) -> Result<(), InfrastructureError> {
    transaction
        .execute_batch(
            "
            CREATE TABLE IF NOT EXISTS schema_migrations (
                version INTEGER PRIMARY KEY,
                applied_at TEXT NOT NULL
            );
            CREATE TABLE IF NOT EXISTS settings (
                singleton_id INTEGER PRIMARY KEY CHECK (singleton_id = 1),
                revision INTEGER NOT NULL,
                json TEXT NOT NULL,
                updated_at TEXT NOT NULL
            );
            CREATE TABLE IF NOT EXISTS rules (
                id TEXT PRIMARY KEY,
                revision INTEGER NOT NULL,
                enabled INTEGER NOT NULL,
                json TEXT NOT NULL,
                updated_at TEXT NOT NULL
            );
            CREATE TABLE IF NOT EXISTS rule_state (
                singleton_id INTEGER PRIMARY KEY CHECK (singleton_id = 1),
                revision INTEGER NOT NULL
            );
            CREATE TABLE IF NOT EXISTS certificate_material (
                kind TEXT PRIMARY KEY,
                protected_blob BLOB NOT NULL,
                metadata_json TEXT NOT NULL,
                updated_at TEXT NOT NULL
            );
            CREATE TABLE IF NOT EXISTS certificate_state (
                singleton_id INTEGER PRIMARY KEY CHECK (singleton_id = 1),
                revision INTEGER NOT NULL
            );
            CREATE TABLE IF NOT EXISTS workspaces (
                id TEXT PRIMARY KEY,
                revision INTEGER NOT NULL,
                json TEXT NOT NULL,
                updated_at TEXT NOT NULL
            );
            CREATE TABLE IF NOT EXISTS workspace_state (
                singleton_id INTEGER PRIMARY KEY CHECK (singleton_id = 1),
                selected_id TEXT NULL,
                FOREIGN KEY(selected_id) REFERENCES workspaces(id) ON DELETE SET NULL
            );
            CREATE TABLE IF NOT EXISTS protected_secrets (
                provider TEXT NOT NULL,
                secret_key TEXT NOT NULL,
                protected_blob BLOB NOT NULL,
                updated_at TEXT NOT NULL,
                PRIMARY KEY(provider, secret_key)
            );
            CREATE TABLE IF NOT EXISTS protocol_packages (
                package_id TEXT NOT NULL,
                version TEXT NOT NULL,
                name TEXT NOT NULL,
                host_api INTEGER NOT NULL,
                enabled INTEGER NOT NULL CHECK(enabled IN (0, 1)),
                validation_state TEXT NOT NULL,
                validation_error_code TEXT NULL,
                installed_at TEXT NOT NULL,
                generation TEXT NOT NULL,
                PRIMARY KEY(package_id, version),
                CHECK(validation_state IN ('valid', 'invalid')),
                CHECK(
                    (validation_state = 'valid' AND validation_error_code IS NULL)
                    OR
                    (validation_state = 'invalid' AND validation_error_code IS NOT NULL)
                )
            );
            CREATE TABLE IF NOT EXISTS protocol_package_files (
                package_id TEXT NOT NULL,
                version TEXT NOT NULL,
                path TEXT NOT NULL,
                contents BLOB NOT NULL,
                PRIMARY KEY(package_id, version, path),
                FOREIGN KEY(package_id, version)
                    REFERENCES protocol_packages(package_id, version)
                    ON DELETE CASCADE
            );
            ",
        )
        .map_err(|source| InfrastructureError::DatabaseMigration { source })
}

fn stored_certificate_revision(transaction: &Transaction<'_>) -> Result<u64, InfrastructureError> {
    let mut statement = transaction
        .prepare("SELECT metadata_json FROM certificate_material")
        .map_err(|source| InfrastructureError::DatabaseMigration { source })?;
    let metadata = statement
        .query_map([], |row| row.get::<_, String>(0))
        .map_err(|source| InfrastructureError::DatabaseMigration { source })?;
    let mut revision = 0;
    for value in metadata {
        let value = value.map_err(|source| InfrastructureError::DatabaseMigration { source })?;
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
            "INSERT OR IGNORE INTO rule_state(singleton_id, revision) VALUES (1, 0)",
            [],
        )
        .map_err(|source| InfrastructureError::DatabaseMigration { source })?;
    transaction
        .execute(
            "INSERT OR IGNORE INTO certificate_state(singleton_id, revision) VALUES (1, ?1)",
            [revision_to_i64(certificate_revision)?],
        )
        .map_err(|source| InfrastructureError::DatabaseMigration { source })?;
    transaction
        .execute(
            "INSERT OR IGNORE INTO workspace_state(singleton_id, selected_id)
             VALUES (1, NULL)",
            [],
        )
        .map_err(|source| InfrastructureError::DatabaseMigration { source })?;
    Ok(())
}

fn record_schema_migration(transaction: &Transaction<'_>) -> Result<(), InfrastructureError> {
    transaction
        .execute(
            "INSERT OR IGNORE INTO schema_migrations(version, applied_at) VALUES (?1, ?2)",
            params![LATEST_SCHEMA_VERSION, Utc::now().to_rfc3339()],
        )
        .map(|_| ())
        .map_err(|source| InfrastructureError::DatabaseMigration { source })
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

fn rule_signature(transaction: &Transaction<'_>) -> Result<Vec<(Uuid, u64)>, InfrastructureError> {
    let mut statement = transaction
        .prepare("SELECT id, revision FROM rules ORDER BY id ASC")
        .map_err(|source| InfrastructureError::Database { source })?;
    statement
        .query_map([], |row| {
            Ok((row.get::<_, String>(0)?, row.get::<_, i64>(1)?))
        })
        .map_err(|source| InfrastructureError::Database { source })?
        .map(|row| {
            let (id, revision) = row.map_err(|source| InfrastructureError::Database { source })?;
            Ok((
                Uuid::parse_str(&id)
                    .map_err(|error| rule_record_corrupt(format!("ID 无效：{error}")))?,
                rule_revision_from_i64(revision)?,
            ))
        })
        .collect()
}

fn load_rule_records(connection: &Connection) -> Result<Vec<RuleRecord>, InfrastructureError> {
    let mut statement = connection
        .prepare(
            "SELECT id, revision, enabled, json, updated_at
             FROM rules ORDER BY updated_at DESC, id ASC",
        )
        .map_err(|source| InfrastructureError::Database { source })?;
    let rows = statement
        .query_map([], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, i64>(1)?,
                row.get::<_, bool>(2)?,
                row.get::<_, String>(3)?,
                row.get::<_, String>(4)?,
            ))
        })
        .map_err(|source| InfrastructureError::Database { source })?;
    rows.map(|row| {
        let (id, revision, enabled, json, updated_at) =
            row.map_err(|source| InfrastructureError::Database { source })?;
        Ok(RuleRecord {
            id: Uuid::parse_str(&id)
                .map_err(|error| rule_record_corrupt(format!("ID 无效：{error}")))?,
            revision: rule_revision_from_i64(revision)?,
            enabled,
            value: serde_json::from_str(&json)
                .map_err(|error| rule_record_corrupt(format!("JSON 无效：{error}")))?,
            updated_at: DateTime::parse_from_rfc3339(&updated_at)
                .map_err(|error| rule_record_corrupt(format!("时间无效：{error}")))?
                .with_timezone(&Utc),
        })
    })
    .collect()
}

fn insert_rule(connection: &Connection, record: &RuleRecord) -> Result<usize, InfrastructureError> {
    connection
        .execute(
            "INSERT OR IGNORE INTO rules(id, revision, enabled, json, updated_at)
             VALUES (?1, ?2, ?3, ?4, ?5)",
            params![
                record.id.to_string(),
                revision_to_i64(record.revision)?,
                record.enabled,
                record.value.to_string(),
                record.updated_at.to_rfc3339()
            ],
        )
        .map_err(|source| InfrastructureError::Database { source })
}

fn advance_rule_collection_revision(
    transaction: &Transaction<'_>,
) -> Result<(), InfrastructureError> {
    let affected = transaction
        .execute(
            "UPDATE rule_state SET revision = revision + 1
             WHERE singleton_id = 1 AND revision < 9223372036854775807",
            [],
        )
        .map_err(|source| InfrastructureError::Database { source })?;
    if affected != 1 {
        return Err(InfrastructureError::RevisionConflict);
    }
    Ok(())
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

fn rule_revision_from_i64(revision: i64) -> Result<u64, InfrastructureError> {
    u64::try_from(revision).map_err(|_| rule_record_corrupt("revision 不能为负数"))
}

fn rule_record_corrupt(message: impl Into<String>) -> InfrastructureError {
    InfrastructureError::PersistenceCorrupt {
        entity: "rule",
        message: message.into(),
    }
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
        Ok(WorkspaceRecord {
            id: Uuid::parse_str(&id).map_err(|error| InfrastructureError::PersistenceCorrupt {
                entity: "workspace",
                message: format!("id 无效：{error}"),
            })?,
            revision: u64::try_from(revision).map_err(|_| {
                InfrastructureError::PersistenceCorrupt {
                    entity: "workspace",
                    message: "revision 不能为负数".into(),
                }
            })?,
            value: serde_json::from_str(&json).map_err(|error| {
                InfrastructureError::PersistenceCorrupt {
                    entity: "workspace",
                    message: format!("JSON 无效：{error}"),
                }
            })?,
            updated_at: DateTime::parse_from_rfc3339(&updated_at)
                .map_err(|error| InfrastructureError::PersistenceCorrupt {
                    entity: "workspace",
                    message: format!("updated_at 无效：{error}"),
                })?
                .with_timezone(&Utc),
        })
    })
    .collect()
}

#[cfg(test)]
#[path = "sqlite/tests/mod.rs"]
mod sqlite_tests;
