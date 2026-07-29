use std::path::Path;

use chrono::{DateTime, Utc};
use parking_lot::Mutex;
use rusqlite::{Connection, OptionalExtension, Transaction, params};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use uuid::Uuid;

use crate::InfrastructureError;

const LATEST_SCHEMA_VERSION: i64 = 1;

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

#[derive(Debug)]
pub struct SqliteStore {
    connection: Mutex<Connection>,
}

impl SqliteStore {
    pub fn open(path: &Path) -> Result<Self, InfrastructureError> {
        let connection =
            Connection::open(path).map_err(|source| InfrastructureError::Database { source })?;
        Self::from_connection(connection)
    }

    pub fn in_memory() -> Result<Self, InfrastructureError> {
        let connection = Connection::open_in_memory()
            .map_err(|source| InfrastructureError::Database { source })?;
        Self::from_connection(connection)
    }

    fn from_connection(connection: Connection) -> Result<Self, InfrastructureError> {
        connection
            .execute_batch("PRAGMA foreign_keys = ON; PRAGMA journal_mode = WAL;")
            .map_err(|source| InfrastructureError::Database { source })?;
        let store = Self {
            connection: Mutex::new(connection),
        };
        store.migrate()?;
        Ok(store)
    }

    fn migrate(&self) -> Result<(), InfrastructureError> {
        let mut connection = self.connection.lock();
        let transaction = connection
            .transaction()
            .map_err(|source| InfrastructureError::DatabaseMigration { source })?;
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
                CREATE TABLE IF NOT EXISTS certificate_material (
                    kind TEXT PRIMARY KEY,
                    protected_blob BLOB NOT NULL,
                    metadata_json TEXT NOT NULL,
                    updated_at TEXT NOT NULL
                );
                ",
            )
            .map_err(|source| InfrastructureError::DatabaseMigration { source })?;
        transaction
            .execute(
                "INSERT OR IGNORE INTO schema_migrations(version, applied_at) VALUES (?1, ?2)",
                params![LATEST_SCHEMA_VERSION, Utc::now().to_rfc3339()],
            )
            .map_err(|source| InfrastructureError::DatabaseMigration { source })?;
        transaction
            .commit()
            .map_err(|source| InfrastructureError::DatabaseMigration { source })
    }

    pub fn load_settings(&self) -> Result<Option<StoredSettings>, InfrastructureError> {
        let connection = self.connection.lock();
        connection
            .query_row(
                "SELECT revision, json, updated_at FROM settings WHERE singleton_id = 1",
                [],
                |row| {
                    let revision: i64 = row.get(0)?;
                    let json: String = row.get(1)?;
                    let updated_at: String = row.get(2)?;
                    Ok((revision, json, updated_at))
                },
            )
            .optional()
            .map_err(|source| InfrastructureError::Database { source })?
            .map(parse_settings_row)
            .transpose()
    }

    pub fn save_settings(
        &self,
        expected_revision: u64,
        value: &Value,
    ) -> Result<StoredSettings, InfrastructureError> {
        let mut connection = self.connection.lock();
        let transaction = connection
            .transaction()
            .map_err(|source| InfrastructureError::Database { source })?;
        let current = current_revision(&transaction, "settings", "singleton_id = 1")?;
        if current.unwrap_or(0) != expected_revision {
            return Err(InfrastructureError::RevisionConflict);
        }
        let next_revision = expected_revision.saturating_add(1);
        let stored_revision = revision_to_i64(next_revision)?;
        let updated_at = Utc::now();
        transaction
            .execute(
                "INSERT INTO settings(singleton_id, revision, json, updated_at)
                 VALUES (1, ?1, ?2, ?3)
                 ON CONFLICT(singleton_id) DO UPDATE SET
                    revision = excluded.revision,
                    json = excluded.json,
                    updated_at = excluded.updated_at",
                params![stored_revision, value.to_string(), updated_at.to_rfc3339()],
            )
            .map_err(|source| InfrastructureError::Database { source })?;
        transaction
            .commit()
            .map_err(|source| InfrastructureError::Database { source })?;
        Ok(StoredSettings {
            revision: next_revision,
            value: value.clone(),
            updated_at,
        })
    }

    pub fn list_rules(&self) -> Result<Vec<RuleRecord>, InfrastructureError> {
        let connection = self.connection.lock();
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
                id: Uuid::parse_str(&id).map_err(|error| {
                    InfrastructureError::CertificateInvalid {
                        message: format!("持久化规则 ID 无效：{error}"),
                    }
                })?,
                revision: revision_from_i64(revision)?,
                enabled,
                value: serde_json::from_str(&json).map_err(|error| {
                    InfrastructureError::CertificateInvalid {
                        message: format!("持久化规则 JSON 无效：{error}"),
                    }
                })?,
                updated_at: DateTime::parse_from_rfc3339(&updated_at)
                    .map_err(|error| InfrastructureError::CertificateInvalid {
                        message: format!("持久化规则时间无效：{error}"),
                    })?
                    .with_timezone(&Utc),
            })
        })
        .collect()
    }

    pub fn replace_rules_atomically(
        &self,
        records: &[RuleRecord],
    ) -> Result<(), InfrastructureError> {
        let mut connection = self.connection.lock();
        let transaction = connection
            .transaction()
            .map_err(|source| InfrastructureError::Database { source })?;
        transaction
            .execute("DELETE FROM rules", [])
            .map_err(|source| InfrastructureError::Database { source })?;
        for record in records {
            let revision = revision_to_i64(record.revision)?;
            transaction
                .execute(
                    "INSERT INTO rules(id, revision, enabled, json, updated_at)
                     VALUES (?1, ?2, ?3, ?4, ?5)",
                    params![
                        record.id.to_string(),
                        revision,
                        record.enabled,
                        record.value.to_string(),
                        record.updated_at.to_rfc3339()
                    ],
                )
                .map_err(|source| InfrastructureError::Database { source })?;
        }
        transaction
            .commit()
            .map_err(|source| InfrastructureError::Database { source })
    }

    pub fn compare_and_swap_rule_runtime(
        &self,
        expected_signature: &[(Uuid, u64)],
        updates: &[RuleRuntimeUpdate],
    ) -> Result<(), InfrastructureError> {
        let mut connection = self.connection.lock();
        let transaction = connection
            .transaction()
            .map_err(|source| InfrastructureError::Database { source })?;
        let current_signature = rule_signature(&transaction)?;
        let mut expected_signature = expected_signature.to_vec();
        expected_signature.sort_unstable();
        if current_signature != expected_signature {
            return Err(InfrastructureError::RevisionConflict);
        }

        for update in updates {
            let mut value = transaction
                .query_row(
                    "SELECT json FROM rules WHERE id = ?1 AND revision = ?2",
                    params![
                        update.id.to_string(),
                        revision_to_i64(update.expected_revision)?
                    ],
                    |row| row.get::<_, String>(0),
                )
                .optional()
                .map_err(|source| InfrastructureError::Database { source })?
                .ok_or(InfrastructureError::RevisionConflict)
                .and_then(|json| {
                    serde_json::from_str::<Value>(&json).map_err(|error| {
                        InfrastructureError::CertificateInvalid {
                            message: format!("持久化规则 JSON 无效：{error}"),
                        }
                    })
                })?;
            let object =
                value
                    .as_object_mut()
                    .ok_or_else(|| InfrastructureError::CertificateInvalid {
                        message: "持久化规则 JSON 必须是对象".into(),
                    })?;
            object.insert("hit_count".into(), Value::from(update.hit_count));
            object.insert(
                "last_hit_at".into(),
                serde_json::to_value(update.last_hit_at).map_err(|error| {
                    InfrastructureError::CertificateInvalid {
                        message: format!("规则命中时间序列化失败：{error}"),
                    }
                })?,
            );
            if update.revision != update.expected_revision {
                object.insert("revision".into(), Value::from(update.revision));
                object.insert("enabled".into(), Value::from(update.enabled));
            }
            let affected = transaction
                .execute(
                    "UPDATE rules
                     SET revision = ?1, enabled = ?2, json = ?3, updated_at = ?4
                     WHERE id = ?5 AND revision = ?6",
                    params![
                        revision_to_i64(update.revision)?,
                        update.enabled,
                        value.to_string(),
                        Utc::now().to_rfc3339(),
                        update.id.to_string(),
                        revision_to_i64(update.expected_revision)?
                    ],
                )
                .map_err(|source| InfrastructureError::Database { source })?;
            if affected != 1 {
                return Err(InfrastructureError::RevisionConflict);
            }
        }
        transaction
            .commit()
            .map_err(|source| InfrastructureError::Database { source })
    }

    pub fn put_certificate_material(
        &self,
        record: &CertificateMaterialRecord,
    ) -> Result<(), InfrastructureError> {
        let connection = self.connection.lock();
        put_certificate_material(&connection, record)?;
        Ok(())
    }

    pub fn put_certificate_materials_atomically(
        &self,
        records: &[CertificateMaterialRecord],
    ) -> Result<(), InfrastructureError> {
        let mut connection = self.connection.lock();
        let transaction = connection
            .transaction()
            .map_err(|source| InfrastructureError::Database { source })?;
        for record in records {
            put_certificate_material(&transaction, record)?;
        }
        transaction
            .commit()
            .map_err(|source| InfrastructureError::Database { source })
    }

    pub fn load_certificate_material(
        &self,
        kind: &str,
    ) -> Result<Option<CertificateMaterialRecord>, InfrastructureError> {
        let connection = self.connection.lock();
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

    #[cfg(test)]
    fn table_names(&self) -> Result<Vec<String>, InfrastructureError> {
        let connection = self.connection.lock();
        let mut statement = connection
            .prepare("SELECT name FROM sqlite_master WHERE type = 'table' ORDER BY name")
            .map_err(|source| InfrastructureError::Database { source })?;
        statement
            .query_map([], |row| row.get(0))
            .map_err(|source| InfrastructureError::Database { source })?
            .map(|row| row.map_err(|source| InfrastructureError::Database { source }))
            .collect()
    }
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
                Uuid::parse_str(&id).map_err(|error| InfrastructureError::CertificateInvalid {
                    message: format!("持久化规则 ID 无效：{error}"),
                })?,
                revision_from_i64(revision)?,
            ))
        })
        .collect()
}

fn parse_settings_row(
    (revision, json, updated_at): (i64, String, String),
) -> Result<StoredSettings, InfrastructureError> {
    Ok(StoredSettings {
        revision: revision_from_i64(revision)?,
        value: serde_json::from_str(&json).map_err(|error| {
            InfrastructureError::CertificateInvalid {
                message: format!("持久化设置 JSON 无效：{error}"),
            }
        })?,
        updated_at: DateTime::parse_from_rfc3339(&updated_at)
            .map_err(|error| InfrastructureError::CertificateInvalid {
                message: format!("持久化设置时间无效：{error}"),
            })?
            .with_timezone(&Utc),
    })
}

fn revision_to_i64(revision: u64) -> Result<i64, InfrastructureError> {
    i64::try_from(revision).map_err(|_| InfrastructureError::RevisionConflict)
}

fn revision_from_i64(revision: i64) -> Result<u64, InfrastructureError> {
    u64::try_from(revision).map_err(|_| InfrastructureError::RevisionConflict)
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    /// SECURITY-001, SECURITY-002, SECURITY-003: migrations create only
    /// durable configuration tables and no captured payload/session table.
    #[test]
    fn schema_has_no_payload_storage() {
        let store = SqliteStore::in_memory().expect("store");
        let tables = store.table_names().expect("tables");
        assert_eq!(
            tables,
            vec![
                "certificate_material",
                "rules",
                "schema_migrations",
                "settings"
            ]
        );
        assert!(!tables.iter().any(|name| {
            name.contains("payload") || name.contains("session") || name.contains("breakpoint")
        }));
    }

    /// ENGINE-008, SECURITY-004: stale settings writes fail atomically.
    #[test]
    fn settings_use_optimistic_revision() {
        let store = SqliteStore::in_memory().expect("store");
        let first = store
            .save_settings(0, &json!({"port": 16627}))
            .expect("save");
        assert_eq!(first.revision, 1);
        assert!(matches!(
            store.save_settings(0, &json!({"port": 16127})),
            Err(InfrastructureError::RevisionConflict)
        ));
        assert_eq!(
            store.load_settings().expect("load").expect("value").value,
            json!({"port": 16627})
        );
    }

    #[test]
    fn certificate_batch_write_rolls_back_on_failure() {
        let store = SqliteStore::in_memory().expect("store");
        store
            .connection
            .lock()
            .execute_batch(
                "CREATE TRIGGER reject_leaf
                 BEFORE INSERT ON certificate_material
                 WHEN NEW.kind = 'proxy_leaf'
                 BEGIN
                    SELECT RAISE(ABORT, 'reject leaf');
                 END;",
            )
            .expect("trigger");
        let now = Utc::now();
        let records = [
            CertificateMaterialRecord {
                kind: "local_root_ca".into(),
                protected_blob: vec![1],
                metadata: json!({}),
                updated_at: now,
            },
            CertificateMaterialRecord {
                kind: "proxy_leaf".into(),
                protected_blob: vec![2],
                metadata: json!({}),
                updated_at: now,
            },
        ];

        assert!(
            store
                .put_certificate_materials_atomically(&records)
                .is_err()
        );
        assert!(
            store
                .load_certificate_material("local_root_ca")
                .expect("load")
                .is_none()
        );
    }

    /// RULE-001, SECURITY-004: rule import replaces the collection in one
    /// transaction.
    #[test]
    fn rule_import_is_atomic() {
        let store = SqliteStore::in_memory().expect("store");
        let record = RuleRecord {
            id: Uuid::new_v4(),
            revision: 1,
            enabled: true,
            value: json!({"name": "first"}),
            updated_at: Utc::now(),
        };
        store
            .replace_rules_atomically(std::slice::from_ref(&record))
            .expect("replace");
        assert_eq!(store.list_rules().expect("list"), vec![record]);
    }
}
