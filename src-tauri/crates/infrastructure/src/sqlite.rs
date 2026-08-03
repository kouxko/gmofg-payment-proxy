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

const LATEST_SCHEMA_VERSION: i64 = 5;

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
            .execute_batch(
                "PRAGMA foreign_keys = ON;
                 PRAGMA busy_timeout = 5000;
                 PRAGMA journal_mode = WAL;",
            )
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
        create_schema(&transaction)?;
        let certificate_revision = stored_certificate_revision(&transaction)?;
        initialize_singleton_state(&transaction, certificate_revision)?;
        record_schema_migration(&transaction)?;
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

    pub fn save_protected_secret(
        &self,
        record: &ProtectedSecretRecord,
    ) -> Result<(), InfrastructureError> {
        let connection = self.connection.lock();
        connection
            .execute(
                "INSERT INTO protected_secrets(provider, secret_key, protected_blob, updated_at)
                 VALUES (?1, ?2, ?3, ?4)
                 ON CONFLICT(provider, secret_key) DO UPDATE SET
                   protected_blob = excluded.protected_blob,
                   updated_at = excluded.updated_at",
                params![
                    record.provider,
                    record.key,
                    record.protected_blob,
                    record.updated_at.to_rfc3339(),
                ],
            )
            .map(|_| ())
            .map_err(|source| InfrastructureError::Database { source })
    }

    pub fn load_protected_secret(
        &self,
        provider: &str,
        key: &str,
    ) -> Result<Option<ProtectedSecretRecord>, InfrastructureError> {
        let connection = self.connection.lock();
        connection
            .query_row(
                "SELECT protected_blob, updated_at FROM protected_secrets
                 WHERE provider = ?1 AND secret_key = ?2",
                params![provider, key],
                |row| {
                    let protected_blob = row.get(0)?;
                    let updated_at: String = row.get(1)?;
                    Ok((protected_blob, updated_at))
                },
            )
            .optional()
            .map_err(|source| InfrastructureError::Database { source })?
            .map(|(protected_blob, updated_at)| {
                Ok(ProtectedSecretRecord {
                    provider: provider.to_owned(),
                    key: key.to_owned(),
                    protected_blob,
                    updated_at: DateTime::parse_from_rfc3339(&updated_at)
                        .map(|value| value.with_timezone(&Utc))
                        .map_err(|error| InfrastructureError::PersistenceCorrupt {
                            entity: "protected_secrets",
                            message: format!("updated_at 无效：{error}"),
                        })?,
                })
            })
            .transpose()
    }

    /// 在一个读事务内返回 Workspace 列表和当前选中项，避免 UI 看到不一致快照。
    pub fn load_workspaces(&self) -> Result<WorkspaceCollectionSnapshot, InfrastructureError> {
        let mut connection = self.connection.lock();
        let transaction = connection
            .transaction()
            .map_err(|source| InfrastructureError::Database { source })?;
        let selected_id = transaction
            .query_row(
                "SELECT selected_id FROM workspace_state WHERE singleton_id = 1",
                [],
                |row| row.get::<_, Option<String>>(0),
            )
            .optional()
            .map_err(|source| InfrastructureError::Database { source })?
            .flatten()
            .map(|value| {
                Uuid::parse_str(&value).map_err(|error| InfrastructureError::PersistenceCorrupt {
                    entity: "workspace_state",
                    message: format!("selected_id 无效：{error}"),
                })
            })
            .transpose()?;
        let records = load_workspace_records(&transaction)?;
        transaction
            .commit()
            .map_err(|source| InfrastructureError::Database { source })?;
        Ok(WorkspaceCollectionSnapshot {
            selected_id,
            records,
        })
    }

    /// 插入一个全新的 Workspace。若当前没有选中项，则原子地选中新 Workspace。
    pub fn insert_workspace(&self, record: &WorkspaceRecord) -> Result<(), InfrastructureError> {
        let mut connection = self.connection.lock();
        let transaction = connection
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .map_err(|source| InfrastructureError::Database { source })?;
        let inserted = transaction
            .execute(
                "INSERT OR IGNORE INTO workspaces(id, revision, json, updated_at)
                 VALUES (?1, ?2, ?3, ?4)",
                params![
                    record.id.to_string(),
                    revision_to_i64(record.revision)?,
                    record.value.to_string(),
                    record.updated_at.to_rfc3339(),
                ],
            )
            .map_err(|source| InfrastructureError::Database { source })?;
        if inserted != 1 {
            return Err(InfrastructureError::RevisionConflict);
        }
        transaction
            .execute(
                "UPDATE workspace_state SET selected_id = ?1
                 WHERE singleton_id = 1 AND selected_id IS NULL",
                [record.id.to_string()],
            )
            .map_err(|source| InfrastructureError::Database { source })?;
        transaction
            .commit()
            .map_err(|source| InfrastructureError::Database { source })
    }

    /// 仅当数据库中的 revision 与调用者编辑起点一致时更新。
    pub fn compare_and_swap_workspace(
        &self,
        expected_revision: u64,
        record: &WorkspaceRecord,
    ) -> Result<(), InfrastructureError> {
        let connection = self.connection.lock();
        let changed = connection
            .execute(
                "UPDATE workspaces SET revision = ?1, json = ?2, updated_at = ?3
                 WHERE id = ?4 AND revision = ?5",
                params![
                    revision_to_i64(record.revision)?,
                    record.value.to_string(),
                    record.updated_at.to_rfc3339(),
                    record.id.to_string(),
                    revision_to_i64(expected_revision)?,
                ],
            )
            .map_err(|source| InfrastructureError::Database { source })?;
        if changed == 1 {
            Ok(())
        } else {
            Err(InfrastructureError::RevisionConflict)
        }
    }

    /// 仅更新当前选中的 Workspace，并把“仍然选中”与 revision 比较放在同一写事务中。
    ///
    /// 规则编辑器没有单独的规则集合：它编辑的是当前 Workspace 聚合内的 `rules` 字段。
    /// 因此，仅按 Workspace ID 做 CAS 还不够——用户切换 Workspace 的同时，旧页面不得把
    /// 修改写回已经取消选中的 Workspace。这个方法把选中项校验和聚合更新合并成一个原子操作。
    pub fn compare_and_swap_selected_workspace(
        &self,
        expected_selected_id: Uuid,
        expected_revision: u64,
        record: &WorkspaceRecord,
    ) -> Result<(), InfrastructureError> {
        if record.id != expected_selected_id {
            return Err(InfrastructureError::RevisionConflict);
        }
        let mut connection = self.connection.lock();
        let transaction = connection
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .map_err(|source| InfrastructureError::Database { source })?;
        let changed = transaction
            .execute(
                "UPDATE workspaces SET revision = ?1, json = ?2, updated_at = ?3
                 WHERE id = ?4 AND revision = ?5
                   AND EXISTS (
                     SELECT 1 FROM workspace_state
                     WHERE singleton_id = 1 AND selected_id = ?4
                   )",
                params![
                    revision_to_i64(record.revision)?,
                    record.value.to_string(),
                    record.updated_at.to_rfc3339(),
                    record.id.to_string(),
                    revision_to_i64(expected_revision)?,
                ],
            )
            .map_err(|source| InfrastructureError::Database { source })?;
        if changed != 1 {
            return Err(InfrastructureError::RevisionConflict);
        }
        transaction
            .commit()
            .map_err(|source| InfrastructureError::Database { source })
    }

    pub fn select_workspace(&self, id: Uuid) -> Result<(), InfrastructureError> {
        let connection = self.connection.lock();
        let changed = connection
            .execute(
                "UPDATE workspace_state SET selected_id = ?1
                 WHERE singleton_id = 1 AND EXISTS (SELECT 1 FROM workspaces WHERE id = ?1)",
                [id.to_string()],
            )
            .map_err(|source| InfrastructureError::Database { source })?;
        if changed == 1 {
            Ok(())
        } else {
            Err(InfrastructureError::RevisionConflict)
        }
    }

    /// 删除 Workspace 后选中最早创建的剩余项；全部删除时选中项为 NULL。
    pub fn delete_workspace(
        &self,
        id: Uuid,
        expected_revision: u64,
    ) -> Result<(), InfrastructureError> {
        let mut connection = self.connection.lock();
        let transaction = connection
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .map_err(|source| InfrastructureError::Database { source })?;
        let deleted = transaction
            .execute(
                "DELETE FROM workspaces WHERE id = ?1 AND revision = ?2",
                params![id.to_string(), revision_to_i64(expected_revision)?],
            )
            .map_err(|source| InfrastructureError::Database { source })?;
        if deleted != 1 {
            return Err(InfrastructureError::RevisionConflict);
        }
        transaction
            .execute(
                "UPDATE workspace_state SET selected_id = (
                    SELECT id FROM workspaces ORDER BY updated_at ASC, id ASC LIMIT 1
                 ) WHERE singleton_id = 1 AND selected_id IS NULL",
                [],
            )
            .map_err(|source| InfrastructureError::Database { source })?;
        transaction
            .commit()
            .map_err(|source| InfrastructureError::Database { source })
    }

    pub fn save_settings(
        &self,
        expected_revision: u64,
        value: &Value,
    ) -> Result<StoredSettings, InfrastructureError> {
        let mut connection = self.connection.lock();
        // IMMEDIATE 在读取 revision 前取得写入权，使“比较 + 更新”成为一个不可分割事务。
        // 若调用方版本已旧，返回 RevisionConflict，由上层重新加载，绝不后写覆盖先写。
        let transaction = connection
            .transaction_with_behavior(TransactionBehavior::Immediate)
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

    /// 在单个 IMMEDIATE 事务中替换全部可移植配置。
    ///
    /// 文档解析和领域校验在 application 完成；这里仍验证选中项确实存在。任一 SQL
    /// 失败都会回滚 Workspace、选择状态和 Settings，禁止产生半导入状态。
    pub fn replace_application_configuration(
        &self,
        selected_id: Uuid,
        records: &[WorkspaceRecord],
        settings: &Value,
    ) -> Result<(), InfrastructureError> {
        if records.is_empty() || !records.iter().any(|record| record.id == selected_id) {
            return Err(InfrastructureError::RevisionConflict);
        }
        let mut connection = self.connection.lock();
        let transaction = connection
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .map_err(|source| InfrastructureError::Database { source })?;

        transaction
            .execute("DELETE FROM workspaces", [])
            .map_err(|source| InfrastructureError::Database { source })?;
        for record in records {
            transaction
                .execute(
                    "INSERT INTO workspaces(id, revision, json, updated_at)
                     VALUES (?1, ?2, ?3, ?4)",
                    params![
                        record.id.to_string(),
                        revision_to_i64(record.revision)?,
                        record.value.to_string(),
                        record.updated_at.to_rfc3339(),
                    ],
                )
                .map_err(|source| InfrastructureError::Database { source })?;
        }
        transaction
            .execute(
                "UPDATE workspace_state SET selected_id = ?1 WHERE singleton_id = 1",
                [selected_id.to_string()],
            )
            .map_err(|source| InfrastructureError::Database { source })?;

        let current_settings_revision =
            current_revision(&transaction, "settings", "singleton_id = 1")?.unwrap_or(0);
        let next_settings_revision = current_settings_revision.saturating_add(1);
        let now = Utc::now();
        transaction
            .execute(
                "INSERT INTO settings(singleton_id, revision, json, updated_at)
                 VALUES (1, ?1, ?2, ?3)
                 ON CONFLICT(singleton_id) DO UPDATE SET
                    revision = excluded.revision,
                    json = excluded.json,
                    updated_at = excluded.updated_at",
                params![
                    revision_to_i64(next_settings_revision)?,
                    settings.to_string(),
                    now.to_rfc3339(),
                ],
            )
            .map_err(|source| InfrastructureError::Database { source })?;

        transaction
            .commit()
            .map_err(|source| InfrastructureError::Database { source })
    }

    pub fn list_rules(&self) -> Result<Vec<RuleRecord>, InfrastructureError> {
        Ok(self.load_rules_snapshot()?.records)
    }

    pub fn load_rules_snapshot(&self) -> Result<RuleCollectionSnapshot, InfrastructureError> {
        let mut connection = self.connection.lock();
        let transaction = connection
            .transaction()
            .map_err(|source| InfrastructureError::Database { source })?;
        let revision =
            current_revision(&transaction, "rule_state", "singleton_id = 1")?.unwrap_or(0);
        let records = load_rule_records(&transaction)?;
        transaction
            .commit()
            .map_err(|source| InfrastructureError::Database { source })?;
        Ok(RuleCollectionSnapshot { revision, records })
    }

    pub fn insert_rule(&self, record: &RuleRecord) -> Result<(), InfrastructureError> {
        let mut connection = self.connection.lock();
        let transaction = connection
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .map_err(|source| InfrastructureError::Database { source })?;
        let affected = insert_rule(&transaction, record)?;
        if affected != 1 {
            return Err(InfrastructureError::RevisionConflict);
        }
        advance_rule_collection_revision(&transaction)?;
        transaction
            .commit()
            .map_err(|source| InfrastructureError::Database { source })
    }

    pub fn compare_and_swap_rule(
        &self,
        expected_revision: u64,
        record: &RuleRecord,
    ) -> Result<(), InfrastructureError> {
        let mut connection = self.connection.lock();
        let transaction = connection
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .map_err(|source| InfrastructureError::Database { source })?;
        let affected = transaction
            .execute(
                "UPDATE rules
                 SET revision = ?1, enabled = ?2, json = ?3, updated_at = ?4
                 WHERE id = ?5 AND revision = ?6",
                params![
                    revision_to_i64(record.revision)?,
                    record.enabled,
                    record.value.to_string(),
                    record.updated_at.to_rfc3339(),
                    record.id.to_string(),
                    revision_to_i64(expected_revision)?
                ],
            )
            .map_err(|source| InfrastructureError::Database { source })?;
        if affected != 1 {
            return Err(InfrastructureError::RevisionConflict);
        }
        advance_rule_collection_revision(&transaction)?;
        transaction
            .commit()
            .map_err(|source| InfrastructureError::Database { source })
    }

    pub fn delete_rule(&self, id: Uuid, expected_revision: u64) -> Result<(), InfrastructureError> {
        let mut connection = self.connection.lock();
        let transaction = connection
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .map_err(|source| InfrastructureError::Database { source })?;
        let affected = transaction
            .execute(
                "DELETE FROM rules WHERE id = ?1 AND revision = ?2",
                params![id.to_string(), revision_to_i64(expected_revision)?],
            )
            .map_err(|source| InfrastructureError::Database { source })?;
        if affected != 1 {
            return Err(InfrastructureError::RevisionConflict);
        }
        advance_rule_collection_revision(&transaction)?;
        transaction
            .commit()
            .map_err(|source| InfrastructureError::Database { source })
    }

    pub fn replace_rules_atomically(
        &self,
        expected_collection_revision: u64,
        records: &[RuleRecord],
    ) -> Result<u64, InfrastructureError> {
        let mut connection = self.connection.lock();
        // 整套规则先 CAS 推进集合版本，再删除/插入；任一条失败都会回滚整个事务，读者
        // 不可能看到“旧规则已删、新规则只写了一半”的中间状态。
        let transaction = connection
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .map_err(|source| InfrastructureError::Database { source })?;
        let next_revision = expected_collection_revision.saturating_add(1);
        let affected = transaction
            .execute(
                "UPDATE rule_state SET revision = ?1
                 WHERE singleton_id = 1 AND revision = ?2",
                params![
                    revision_to_i64(next_revision)?,
                    revision_to_i64(expected_collection_revision)?
                ],
            )
            .map_err(|source| InfrastructureError::Database { source })?;
        if affected != 1 {
            return Err(InfrastructureError::RevisionConflict);
        }
        transaction
            .execute("DELETE FROM rules", [])
            .map_err(|source| InfrastructureError::Database { source })?;
        for record in records {
            if insert_rule(&transaction, record)? != 1 {
                return Err(InfrastructureError::RevisionConflict);
            }
        }
        transaction
            .commit()
            .map_err(|source| InfrastructureError::Database { source })?;
        Ok(next_revision)
    }

    pub fn compare_and_swap_rule_runtime(
        &self,
        expected_collection_revision: u64,
        expected_signature: &[(Uuid, u64)],
        updates: &[RuleRuntimeUpdate],
    ) -> Result<u64, InfrastructureError> {
        let mut connection = self.connection.lock();
        let transaction = connection
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .map_err(|source| InfrastructureError::Database { source })?;
        let current_collection_revision =
            current_revision(&transaction, "rule_state", "singleton_id = 1")?.unwrap_or(0);
        if current_collection_revision != expected_collection_revision {
            return Err(InfrastructureError::RevisionConflict);
        }
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
                    serde_json::from_str::<Value>(&json)
                        .map_err(|error| rule_record_corrupt(format!("JSON 无效：{error}")))
                })?;
            let object = value
                .as_object_mut()
                .ok_or_else(|| rule_record_corrupt("JSON 必须是对象"))?;
            object.insert("hit_count".into(), Value::from(update.hit_count));
            object.insert(
                "last_hit_at".into(),
                serde_json::to_value(update.last_hit_at)
                    .map_err(|error| rule_record_corrupt(format!("命中时间序列化失败：{error}")))?,
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
        let next_collection_revision = expected_collection_revision.saturating_add(1);
        let affected = transaction
            .execute(
                "UPDATE rule_state SET revision = ?1
                 WHERE singleton_id = 1 AND revision = ?2",
                params![
                    revision_to_i64(next_collection_revision)?,
                    revision_to_i64(expected_collection_revision)?
                ],
            )
            .map_err(|source| InfrastructureError::Database { source })?;
        if affected != 1 {
            return Err(InfrastructureError::RevisionConflict);
        }
        transaction
            .commit()
            .map_err(|source| InfrastructureError::Database { source })?;
        Ok(next_collection_revision)
    }

    pub fn compare_and_swap_certificate_materials(
        &self,
        expected_revision: u64,
        records: &[CertificateMaterialRecord],
    ) -> Result<u64, InfrastructureError> {
        let mut connection = self.connection.lock();
        let transaction = connection
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .map_err(|source| InfrastructureError::Database { source })?;
        let current_revision =
            current_revision(&transaction, "certificate_state", "singleton_id = 1")?.unwrap_or(0);
        if current_revision != expected_revision {
            return Err(InfrastructureError::RevisionConflict);
        }
        let next_revision = expected_revision.saturating_add(1);
        for record in records {
            if record.metadata.get("revision").and_then(Value::as_u64) != Some(next_revision) {
                return Err(InfrastructureError::CertificateInvalid {
                    message: "证书记录修订号与聚合修订号不一致".into(),
                });
            }
        }
        for record in records {
            put_certificate_material(&transaction, record)?;
        }
        let affected = transaction
            .execute(
                "UPDATE certificate_state SET revision = ?1
                 WHERE singleton_id = 1 AND revision = ?2",
                params![
                    revision_to_i64(next_revision)?,
                    revision_to_i64(expected_revision)?
                ],
            )
            .map_err(|source| InfrastructureError::Database { source })?;
        if affected != 1 {
            return Err(InfrastructureError::RevisionConflict);
        }
        transaction
            .commit()
            .map_err(|source| InfrastructureError::Database { source })?;
        Ok(next_revision)
    }

    pub fn load_certificate_materials_snapshot(
        &self,
        kinds: &[&str],
    ) -> Result<CertificateMaterialSnapshot, InfrastructureError> {
        let mut connection = self.connection.lock();
        let transaction = connection
            .transaction()
            .map_err(|source| InfrastructureError::Database { source })?;
        let revision =
            current_revision(&transaction, "certificate_state", "singleton_id = 1")?.unwrap_or(0);
        let records = kinds
            .iter()
            .filter_map(|kind| {
                load_certificate_material(&transaction, kind)
                    .transpose()
                    .map(|result| result.map(|record| (kind, record)))
            })
            .map(|result| result.map(|(_, record)| record))
            .collect::<Result<Vec<_>, _>>()?;
        transaction
            .commit()
            .map_err(|source| InfrastructureError::Database { source })?;
        Ok(CertificateMaterialSnapshot { revision, records })
    }

    pub fn load_certificate_material(
        &self,
        kind: &str,
    ) -> Result<Option<CertificateMaterialRecord>, InfrastructureError> {
        let connection = self.connection.lock();
        load_certificate_material(&connection, kind)
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
mod tests {
    use std::sync::{Arc, Barrier};

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
                "certificate_state",
                "protected_secrets",
                "rule_state",
                "rules",
                "schema_migrations",
                "settings",
                "workspace_state",
                "workspaces"
            ]
        );
        assert!(!tables.iter().any(|name| {
            name.contains("payload") || name.contains("session") || name.contains("breakpoint")
        }));
        let secret_columns = store
            .connection
            .lock()
            .prepare("PRAGMA table_info(protected_secrets)")
            .and_then(|mut statement| {
                statement
                    .query_map([], |row| row.get::<_, String>(1))?
                    .collect::<Result<Vec<_>, _>>()
            })
            .expect("protected secret columns");
        assert!(secret_columns.contains(&"protected_blob".to_owned()));
        assert!(!secret_columns.iter().any(|column| {
            column.contains("password") || column.contains("username") || column == "plaintext"
        }));
    }

    #[test]
    fn workspaces_persist_selection_and_use_optimistic_revisions() {
        let store = SqliteStore::in_memory().expect("store");
        let first_id = Uuid::new_v4();
        let first = WorkspaceRecord {
            id: first_id,
            revision: 1,
            value: json!({"id": first_id, "name": "first", "revision": 1}),
            updated_at: Utc::now(),
        };
        store.insert_workspace(&first).expect("insert first");
        let snapshot = store.load_workspaces().expect("snapshot");
        assert_eq!(snapshot.selected_id, Some(first_id));
        assert_eq!(snapshot.records, vec![first.clone()]);

        let mut updated = first.clone();
        updated.revision = 2;
        updated.value["revision"] = json!(2);
        store
            .compare_and_swap_workspace(1, &updated)
            .expect("advance revision");
        assert!(matches!(
            store.compare_and_swap_workspace(1, &updated),
            Err(InfrastructureError::RevisionConflict)
        ));

        let second_id = Uuid::new_v4();
        store
            .insert_workspace(&WorkspaceRecord {
                id: second_id,
                revision: 1,
                value: json!({"id": second_id, "name": "second", "revision": 1}),
                updated_at: Utc::now(),
            })
            .expect("insert second");
        store.select_workspace(second_id).expect("select second");
        assert!(matches!(
            store.compare_and_swap_selected_workspace(first_id, 2, &updated),
            Err(InfrastructureError::RevisionConflict)
        ));
        let second_updated = WorkspaceRecord {
            id: second_id,
            revision: 2,
            value: json!({"id": second_id, "name": "second updated", "revision": 2}),
            updated_at: Utc::now(),
        };
        store
            .compare_and_swap_selected_workspace(second_id, 1, &second_updated)
            .expect("update selected workspace");
        assert_eq!(
            store
                .load_workspaces()
                .expect("selected snapshot")
                .selected_id,
            Some(second_id)
        );
        store.delete_workspace(second_id, 2).expect("delete second");
        assert_eq!(
            store
                .load_workspaces()
                .expect("fallback selection")
                .selected_id,
            Some(first_id)
        );
    }

    #[test]
    fn full_configuration_replace_rolls_back_all_tables_on_failure() {
        let store = SqliteStore::in_memory().expect("store");
        let original_id = Uuid::new_v4();
        let original = WorkspaceRecord {
            id: original_id,
            revision: 1,
            value: json!({"id": original_id, "name": "original", "revision": 1}),
            updated_at: Utc::now(),
        };
        store.insert_workspace(&original).expect("seed workspace");
        store
            .save_settings(0, &json!({"name": "original settings"}))
            .expect("seed settings");

        let duplicate_id = Uuid::new_v4();
        let duplicate = WorkspaceRecord {
            id: duplicate_id,
            revision: 1,
            value: json!({"id": duplicate_id, "name": "replacement", "revision": 1}),
            updated_at: Utc::now(),
        };
        let error = store
            .replace_application_configuration(
                duplicate_id,
                &[duplicate.clone(), duplicate],
                &json!({"name": "replacement settings"}),
            )
            .expect_err("duplicate insert must fail inside transaction");
        assert!(matches!(error, InfrastructureError::Database { .. }));

        let snapshot = store.load_workspaces().expect("workspace snapshot");
        assert_eq!(snapshot.selected_id, Some(original_id));
        assert_eq!(snapshot.records, vec![original]);
        assert_eq!(
            store
                .load_settings()
                .expect("settings")
                .expect("stored")
                .value,
            json!({"name": "original settings"})
        );
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
    fn corrupt_settings_revision_json_and_time_use_persistence_error_classification() {
        for (column, value) in [
            ("revision", "-1"),
            ("json", "{not-json"),
            ("updated_at", "not-a-time"),
        ] {
            let store = SqliteStore::in_memory().expect("store");
            store
                .save_settings(0, &json!({"channel": "alpha"}))
                .expect("seed settings");
            let sql = format!("UPDATE settings SET {column} = ?1 WHERE singleton_id = 1");
            store
                .connection
                .lock()
                .execute(&sql, [value])
                .expect("corrupt settings");

            let error = store
                .load_settings()
                .expect_err("corrupt settings must fail");
            assert_eq!(
                error.code(),
                crate::InfrastructureErrorCode::PersistenceCorrupt,
                "wrong classification for {column}"
            );
            assert!(!matches!(
                error,
                InfrastructureError::CertificateInvalid { .. }
            ));
        }
    }

    #[test]
    fn corrupt_rule_id_json_and_time_use_persistence_error_classification() {
        for (column, value) in [
            ("id", "not-a-uuid"),
            ("json", "{not-json"),
            ("updated_at", "not-a-time"),
        ] {
            let store = SqliteStore::in_memory().expect("store");
            let id = Uuid::new_v4().to_string();
            let json = json!({"name": "rule"}).to_string();
            let updated_at = Utc::now().to_rfc3339();
            let (id, json, updated_at) = match column {
                "id" => (value.to_owned(), json, updated_at),
                "json" => (id, value.to_owned(), updated_at),
                "updated_at" => (id, json, value.to_owned()),
                _ => unreachable!(),
            };
            store
                .connection
                .lock()
                .execute(
                    "INSERT INTO rules(id, revision, enabled, json, updated_at)
                     VALUES (?1, 1, 1, ?2, ?3)",
                    params![id, json, updated_at],
                )
                .expect("insert corrupt rule");

            let error = store.list_rules().expect_err("corrupt rule must fail");
            assert_eq!(
                error.code(),
                crate::InfrastructureErrorCode::PersistenceCorrupt,
                "wrong classification for {column}"
            );
            assert!(!matches!(
                error,
                InfrastructureError::CertificateInvalid { .. }
            ));
        }
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
                metadata: json!({"revision": 1}),
                updated_at: now,
            },
            CertificateMaterialRecord {
                kind: "proxy_leaf".into(),
                protected_blob: vec![2],
                metadata: json!({"revision": 1}),
                updated_at: now,
            },
        ];

        assert!(
            store
                .compare_and_swap_certificate_materials(0, &records)
                .is_err()
        );
        assert!(
            store
                .load_certificate_material("local_root_ca")
                .expect("load")
                .is_none()
        );
    }

    #[test]
    fn certificate_aggregate_has_cross_connection_cas_and_atomic_snapshot() {
        let directory = tempfile::tempdir().expect("tempdir");
        let path = directory.path().join("state.sqlite");
        SqliteStore::open(&path).expect("initialize store");
        let barrier = std::sync::Arc::new(std::sync::Barrier::new(2));
        let stores = [
            SqliteStore::open(&path).expect("first writer store"),
            SqliteStore::open(&path).expect("second writer store"),
        ];
        let writers = stores
            .into_iter()
            .zip([[1_u8, 2_u8], [3_u8, 4_u8]])
            .map(|(store, payloads)| {
                let barrier = barrier.clone();
                std::thread::spawn(move || {
                    let now = Utc::now();
                    let records = [
                        CertificateMaterialRecord {
                            kind: "local_root_ca".into(),
                            protected_blob: vec![payloads[0]],
                            metadata: json!({"revision": 1}),
                            updated_at: now,
                        },
                        CertificateMaterialRecord {
                            kind: "proxy_leaf".into(),
                            protected_blob: vec![payloads[1]],
                            metadata: json!({"revision": 1}),
                            updated_at: now,
                        },
                    ];
                    barrier.wait();
                    store.compare_and_swap_certificate_materials(0, &records)
                })
            })
            .collect::<Vec<_>>();
        let results = writers
            .into_iter()
            .map(|writer| writer.join().expect("writer thread"))
            .collect::<Vec<_>>();
        assert_eq!(results.iter().filter(|result| result.is_ok()).count(), 1);
        assert_eq!(
            results
                .iter()
                .filter(|result| matches!(result, Err(InfrastructureError::RevisionConflict)))
                .count(),
            1
        );

        let snapshot = SqliteStore::open(&path)
            .expect("reader store")
            .load_certificate_materials_snapshot(&["local_root_ca", "proxy_leaf"])
            .expect("snapshot");
        assert_eq!(snapshot.revision, 1);
        let protected_blobs = snapshot
            .records
            .iter()
            .map(|record| record.protected_blob.as_slice())
            .collect::<Vec<_>>();
        assert!(
            protected_blobs == vec![&[1][..], &[2][..]]
                || protected_blobs == vec![&[3][..], &[4][..]]
        );
        assert!(snapshot.records.iter().all(|record| {
            record.metadata.get("revision").and_then(Value::as_u64) == Some(snapshot.revision)
        }));
    }

    #[test]
    fn certificate_reader_never_observes_a_mixed_writer_generation() {
        use std::sync::{
            Arc,
            atomic::{AtomicBool, Ordering},
        };

        fn records(revision: u64) -> [CertificateMaterialRecord; 2] {
            let payload = u8::try_from(revision).expect("small test revision");
            let now = Utc::now();
            [
                CertificateMaterialRecord {
                    kind: "local_root_ca".into(),
                    protected_blob: vec![payload],
                    metadata: json!({"revision": revision}),
                    updated_at: now,
                },
                CertificateMaterialRecord {
                    kind: "proxy_leaf".into(),
                    protected_blob: vec![payload],
                    metadata: json!({"revision": revision}),
                    updated_at: now,
                },
            ]
        }

        let directory = tempfile::tempdir().expect("tempdir");
        let path = directory.path().join("state.sqlite");
        let writer = SqliteStore::open(&path).expect("writer store");
        writer
            .compare_and_swap_certificate_materials(0, &records(1))
            .expect("seed aggregate");
        let reader = SqliteStore::open(&path).expect("reader store");
        let finished = Arc::new(AtomicBool::new(false));
        let reader_finished = finished.clone();

        let reader_task = std::thread::spawn(move || {
            let mut snapshots = 0;
            while !reader_finished.load(Ordering::Acquire) {
                let snapshot = reader
                    .load_certificate_materials_snapshot(&["local_root_ca", "proxy_leaf"])
                    .expect("atomic snapshot");
                assert_eq!(snapshot.records.len(), 2);
                assert_eq!(
                    snapshot.records[0].protected_blob,
                    snapshot.records[1].protected_blob
                );
                assert!(snapshot.records.iter().all(|record| {
                    record.metadata.get("revision").and_then(Value::as_u64)
                        == Some(snapshot.revision)
                }));
                snapshots += 1;
            }
            snapshots
        });

        for expected_revision in 1..25 {
            writer
                .compare_and_swap_certificate_materials(
                    expected_revision,
                    &records(expected_revision + 1),
                )
                .expect("advance aggregate");
        }
        finished.store(true, Ordering::Release);
        assert!(reader_task.join().expect("reader thread") > 0);
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
            .replace_rules_atomically(0, std::slice::from_ref(&record))
            .expect("replace");
        assert_eq!(store.list_rules().expect("list"), vec![record.clone()]);

        let duplicate = RuleRecord {
            id: Uuid::new_v4(),
            revision: 1,
            enabled: true,
            value: json!({"name": "duplicate"}),
            updated_at: Utc::now(),
        };
        assert!(matches!(
            store.replace_rules_atomically(1, &[duplicate.clone(), duplicate]),
            Err(InfrastructureError::RevisionConflict)
        ));
        let snapshot = store
            .load_rules_snapshot()
            .expect("snapshot after rollback");
        assert_eq!(snapshot.revision, 1);
        assert_eq!(snapshot.records, vec![record]);
    }

    #[test]
    fn independent_stores_preserve_unrelated_rules_and_reject_stale_writes() {
        let directory = tempfile::tempdir().expect("temp directory");
        let path = directory.path().join("rules.sqlite3");
        let first_store = Arc::new(SqliteStore::open(&path).expect("first store"));
        let second_store = Arc::new(SqliteStore::open(&path).expect("second store"));
        let barrier = Arc::new(Barrier::new(3));
        let first = RuleRecord {
            id: Uuid::new_v4(),
            revision: 1,
            enabled: true,
            value: json!({"name": "first", "revision": 1}),
            updated_at: Utc::now(),
        };
        let second = RuleRecord {
            id: Uuid::new_v4(),
            revision: 1,
            enabled: true,
            value: json!({"name": "second", "revision": 1}),
            updated_at: Utc::now(),
        };

        let first_insert = {
            let store = Arc::clone(&first_store);
            let barrier = Arc::clone(&barrier);
            let record = first.clone();
            std::thread::spawn(move || {
                barrier.wait();
                store.insert_rule(&record)
            })
        };
        let second_insert = {
            let store = Arc::clone(&second_store);
            let barrier = Arc::clone(&barrier);
            let record = second.clone();
            std::thread::spawn(move || {
                barrier.wait();
                store.insert_rule(&record)
            })
        };
        barrier.wait();
        first_insert
            .join()
            .expect("first thread")
            .expect("first insert");
        second_insert
            .join()
            .expect("second thread")
            .expect("second insert");

        let snapshot = first_store.load_rules_snapshot().expect("snapshot");
        assert_eq!(snapshot.revision, 2);
        assert_eq!(snapshot.records.len(), 2);
        assert!(snapshot.records.iter().any(|record| record.id == first.id));
        assert!(snapshot.records.iter().any(|record| record.id == second.id));

        let mut winner = first.clone();
        winner.revision = 2;
        winner.value = json!({"name": "winner", "revision": 2});
        second_store
            .compare_and_swap_rule(1, &winner)
            .expect("winning update");

        let mut stale = first;
        stale.revision = 2;
        stale.value = json!({"name": "stale", "revision": 2});
        assert!(matches!(
            first_store.compare_and_swap_rule(1, &stale),
            Err(InfrastructureError::RevisionConflict)
        ));

        first_store
            .delete_rule(second.id, 1)
            .expect("delete unrelated rule");
        assert!(matches!(
            second_store.delete_rule(second.id, 1),
            Err(InfrastructureError::RevisionConflict)
        ));
        let after_delete = first_store.list_rules().expect("rules after delete");
        assert_eq!(after_delete.len(), 1);
        assert_eq!(after_delete[0].id, winner.id);

        let stale_collection_revision = first_store
            .load_rules_snapshot()
            .expect("pre-insert snapshot")
            .revision;
        let third = RuleRecord {
            id: Uuid::new_v4(),
            revision: 1,
            enabled: true,
            value: json!({"name": "third", "revision": 1}),
            updated_at: Utc::now(),
        };
        second_store.insert_rule(&third).expect("third insert");
        assert!(matches!(
            first_store.replace_rules_atomically(stale_collection_revision, &snapshot.records),
            Err(InfrastructureError::RevisionConflict)
        ));
        let final_rules = first_store.list_rules().expect("final rules");
        assert_eq!(final_rules.len(), 2);
        assert!(final_rules.iter().any(|record| record.id == winner.id));
        assert!(final_rules.iter().any(|record| record.id == third.id));
    }

    #[test]
    fn concurrent_runtime_hits_conflict_then_retry_without_lost_updates() {
        let directory = tempfile::tempdir().expect("temp directory");
        let path = directory.path().join("runtime-rules.sqlite3");
        let first_store = Arc::new(SqliteStore::open(&path).expect("first store"));
        let second_store = Arc::new(SqliteStore::open(&path).expect("second store"));
        let rule_id = Uuid::new_v4();
        first_store
            .insert_rule(&RuleRecord {
                id: rule_id,
                revision: 1,
                enabled: true,
                value: json!({
                    "revision": 1,
                    "enabled": true,
                    "hit_count": 0,
                    "last_hit_at": null
                }),
                updated_at: Utc::now(),
            })
            .expect("seed rule");
        let barrier = Arc::new(Barrier::new(3));
        let writers = [Arc::clone(&first_store), Arc::clone(&second_store)].map(|store| {
            let barrier = Arc::clone(&barrier);
            std::thread::spawn(move || {
                barrier.wait();
                store.compare_and_swap_rule_runtime(
                    1,
                    &[(rule_id, 1)],
                    &[RuleRuntimeUpdate {
                        id: rule_id,
                        expected_revision: 1,
                        revision: 1,
                        enabled: true,
                        hit_count: 1,
                        last_hit_at: None,
                    }],
                )
            })
        });
        barrier.wait();
        let results = writers.map(|writer| writer.join().expect("writer thread"));
        assert_eq!(results.iter().filter(|result| result.is_ok()).count(), 1);
        assert_eq!(
            results
                .iter()
                .filter(|result| matches!(result, Err(InfrastructureError::RevisionConflict)))
                .count(),
            1
        );

        let current = first_store.load_rules_snapshot().expect("current snapshot");
        assert_eq!(current.revision, 2);
        assert_eq!(
            current.records[0]
                .value
                .get("hit_count")
                .and_then(Value::as_u64),
            Some(1)
        );
        first_store
            .compare_and_swap_rule_runtime(
                current.revision,
                &[(rule_id, 1)],
                &[RuleRuntimeUpdate {
                    id: rule_id,
                    expected_revision: 1,
                    revision: 1,
                    enabled: true,
                    hit_count: 2,
                    last_hit_at: None,
                }],
            )
            .expect("retry from refreshed snapshot");
        let final_snapshot = second_store.load_rules_snapshot().expect("final snapshot");
        assert_eq!(final_snapshot.revision, 3);
        assert_eq!(
            final_snapshot.records[0]
                .value
                .get("hit_count")
                .and_then(Value::as_u64),
            Some(2)
        );
    }
}
