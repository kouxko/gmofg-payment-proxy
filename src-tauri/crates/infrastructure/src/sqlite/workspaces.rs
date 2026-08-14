use super::{
    InfrastructureError, OptionalExtension, SqliteStore, StoredSettings, TransactionBehavior, Utc,
    Uuid, Value, WorkspaceCollectionSnapshot, WorkspaceRecord, current_revision,
    load_workspace_records, params, revision_to_i64,
};

impl SqliteStore {
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

    /// 在一个事务中清空用户数据并写入干净默认配置。
    ///
    /// schema 迁移记录与单例状态行属于数据库结构，不删除；其他 Workspace、
    /// Settings、规则、证书材料和受保护秘密全部重置。应用重启时会重建安装级 CA。
    pub fn reset_application_data(
        &self,
        selected_id: Uuid,
        records: &[WorkspaceRecord],
        settings: &Value,
    ) -> Result<(), InfrastructureError> {
        if records.len() != 1 || records[0].id != selected_id {
            return Err(InfrastructureError::RevisionConflict);
        }
        let mut connection = self.connection.lock();
        let transaction = connection
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .map_err(|source| InfrastructureError::Database { source })?;

        transaction
            .execute_batch(
                "DELETE FROM rules;
                 UPDATE rule_state SET revision = revision + 1 WHERE singleton_id = 1;
                 DELETE FROM protected_secrets;
                 DELETE FROM certificate_material;
                 UPDATE certificate_state SET revision = revision + 1 WHERE singleton_id = 1;
                 DELETE FROM protocol_packages;
                 DELETE FROM workspaces;",
            )
            .map_err(|source| InfrastructureError::Database { source })?;
        let record = &records[0];
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
        transaction
            .execute(
                "UPDATE workspace_state SET selected_id = ?1 WHERE singleton_id = 1",
                [selected_id.to_string()],
            )
            .map_err(|source| InfrastructureError::Database { source })?;

        let current_settings_revision =
            current_revision(&transaction, "settings", "singleton_id = 1")?.unwrap_or(0);
        let next_settings_revision = current_settings_revision.saturating_add(1);
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
                    Utc::now().to_rfc3339(),
                ],
            )
            .map_err(|source| InfrastructureError::Database { source })?;

        transaction
            .commit()
            .map_err(|source| InfrastructureError::Database { source })
    }
}
