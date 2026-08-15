//! 协议包注册表与可移植 Workspace/应用配置的组合事务。
//!
//! 文件恢复、Manifest/Schema/Rhai 编译都必须在进入这里之前完成。本模块只负责在一个
//! `IMMEDIATE` 事务内再次比较持久化身份，并把协议包与配置行一起提交或一起回滚。

use std::collections::HashSet;

use rusqlite::{Transaction, TransactionBehavior, params};

use super::{
    InfrastructureError, SqliteStore, Utc, Uuid, Value, WorkspaceRecord, current_revision,
    protocol_packages::{
        StoredProtocolPackageBundleError, StoredProtocolPackageWrite,
        compare_or_insert_protocol_package, require_existing_protocol_package,
    },
    revision_to_i64,
};

impl SqliteStore {
    /// 历史 Workspace 只复用事务内仍存在且内容一致的本机包；注册表保持只读。
    pub(crate) fn insert_legacy_workspace(
        &self,
        record: &WorkspaceRecord,
        referenced_packages: &[StoredProtocolPackageWrite],
    ) -> Result<(), StoredProtocolPackageBundleError> {
        let mut connection = self.connection.lock();
        let transaction = connection
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .map_err(database_error)?;
        for package in referenced_packages {
            require_existing_protocol_package(&transaction, package)?;
        }
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
            .map_err(database_error)?;
        if inserted != 1 {
            return Err(InfrastructureError::RevisionConflict.into());
        }
        transaction
            .execute(
                "UPDATE workspace_state SET selected_id = ?1
                 WHERE singleton_id = 1 AND selected_id IS NULL",
                [record.id.to_string()],
            )
            .map_err(database_error)?;
        transaction.commit().map_err(database_error)?;
        Ok(())
    }

    /// 安装 Workspace 引用的精确包并插入 Workspace。已有相同包保留本机启用位。
    pub(crate) fn insert_workspace_bundle(
        &self,
        record: &WorkspaceRecord,
        packages: &[StoredProtocolPackageWrite],
    ) -> Result<(), StoredProtocolPackageBundleError> {
        let mut connection = self.connection.lock();
        let transaction = connection
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .map_err(database_error)?;
        for package in packages {
            compare_or_insert_protocol_package(&transaction, package, None)?;
        }
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
            .map_err(database_error)?;
        if inserted != 1 {
            return Err(InfrastructureError::RevisionConflict.into());
        }
        transaction
            .execute(
                "UPDATE workspace_state SET selected_id = ?1
                 WHERE singleton_id = 1 AND selected_id IS NULL",
                [record.id.to_string()],
            )
            .map_err(database_error)?;
        transaction.commit().map_err(database_error)?;
        Ok(())
    }

    /// 用文档内容替换整个协议包注册表、Workspace 集合、选择状态和 Settings。
    pub(crate) fn replace_application_bundle(
        &self,
        selected_id: Uuid,
        records: &[WorkspaceRecord],
        settings: &Value,
        packages: &[StoredProtocolPackageWrite],
    ) -> Result<(), StoredProtocolPackageBundleError> {
        require_selected_workspace(selected_id, records)?;
        let mut connection = self.connection.lock();
        let transaction = connection
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .map_err(database_error)?;

        let imported = packages
            .iter()
            .map(|package| package.header.package.clone())
            .collect::<HashSet<_>>();
        for package in packages {
            compare_or_insert_protocol_package(
                &transaction,
                package,
                Some(package.header.enabled),
            )?;
        }
        delete_extra_protocol_packages(&transaction, &imported)?;
        replace_workspaces_and_settings(&transaction, selected_id, records, settings)?;
        transaction.commit().map_err(database_error)?;
        Ok(())
    }

    /// 历史完整配置替换 Workspace/Settings，但完整本机协议包注册表保持只读。
    pub(crate) fn replace_legacy_application_configuration(
        &self,
        selected_id: Uuid,
        records: &[WorkspaceRecord],
        settings: &Value,
        referenced_packages: &[StoredProtocolPackageWrite],
    ) -> Result<(), StoredProtocolPackageBundleError> {
        require_selected_workspace(selected_id, records)?;
        let mut connection = self.connection.lock();
        let transaction = connection
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .map_err(database_error)?;
        for package in referenced_packages {
            require_existing_protocol_package(&transaction, package)?;
        }
        replace_workspaces_and_settings(&transaction, selected_id, records, settings)?;
        transaction.commit().map_err(database_error)?;
        Ok(())
    }

    /// 清理所有协议包与用户数据并写入唯一默认 Workspace。
    pub(crate) fn reset_application_bundle(
        &self,
        selected_id: Uuid,
        records: &[WorkspaceRecord],
        settings: &Value,
    ) -> Result<(), InfrastructureError> {
        if records.len() != 1 || records[0].id != selected_id {
            return Err(InfrastructureError::RevisionConflict);
        }
        let _completion_gate = self.capture_coordination.completion_gate.write();
        let _capture_gate = self.capture_coordination.mutation_gate.lock();
        self.capture_coordination.bump_reset().map_err(|message| {
            InfrastructureError::PersistenceCorrupt {
                entity: "socket_capture",
                message: message.to_owned(),
            }
        })?;
        let mut connection = self.connection.lock();
        let transaction = connection
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .map_err(database_error)?;
        transaction
            .execute_batch(
                "DELETE FROM rules;
                 UPDATE rule_state SET revision = revision + 1 WHERE singleton_id = 1;
                 DELETE FROM protected_secrets;
                 DELETE FROM certificate_material;
                 UPDATE certificate_state SET revision = revision + 1 WHERE singleton_id = 1;
                 DELETE FROM socket_captures;
                 DELETE FROM protocol_packages;
                 DELETE FROM workspaces;",
            )
            .map_err(database_error)?;
        replace_workspaces_and_settings(&transaction, selected_id, records, settings)?;
        transaction.commit().map_err(database_error)?;
        Ok(())
    }
}

fn replace_workspaces_and_settings(
    transaction: &Transaction<'_>,
    selected_id: Uuid,
    records: &[WorkspaceRecord],
    settings: &Value,
) -> Result<(), InfrastructureError> {
    transaction
        .execute("DELETE FROM workspaces", [])
        .map_err(database_error)?;
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
            .map_err(database_error)?;
    }
    transaction
        .execute(
            "UPDATE workspace_state SET selected_id = ?1 WHERE singleton_id = 1",
            [selected_id.to_string()],
        )
        .map_err(database_error)?;
    let current_revision =
        current_revision(transaction, "settings", "singleton_id = 1")?.unwrap_or(0);
    let next_revision =
        current_revision
            .checked_add(1)
            .ok_or_else(|| InfrastructureError::PersistenceCorrupt {
                entity: "settings",
                message: "revision 已达到上限".to_owned(),
            })?;
    transaction
        .execute(
            "INSERT INTO settings(singleton_id, revision, json, updated_at)
             VALUES (1, ?1, ?2, ?3)
             ON CONFLICT(singleton_id) DO UPDATE SET
                revision = excluded.revision,
                json = excluded.json,
                updated_at = excluded.updated_at",
            params![
                revision_to_i64(next_revision)?,
                settings.to_string(),
                Utc::now().to_rfc3339(),
            ],
        )
        .map_err(database_error)?;
    Ok(())
}

fn delete_extra_protocol_packages(
    transaction: &Transaction<'_>,
    imported: &HashSet<intercept_proxy_domain::ProtocolPackageRef>,
) -> Result<(), InfrastructureError> {
    let mut statement = transaction
        .prepare("SELECT package_id, version FROM protocol_packages")
        .map_err(database_error)?;
    let identities = statement
        .query_map([], |row| {
            Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
        })
        .map_err(database_error)?
        .collect::<Result<Vec<_>, _>>()
        .map_err(database_error)?;
    drop(statement);
    for (id, version) in identities {
        let keep = imported
            .iter()
            .any(|package| package.id.as_str() == id && package.version.as_str() == version);
        if !keep {
            transaction
                .execute(
                    "DELETE FROM protocol_packages WHERE package_id = ?1 AND version = ?2",
                    params![id, version],
                )
                .map_err(database_error)?;
        }
    }
    Ok(())
}

fn require_selected_workspace(
    selected_id: Uuid,
    records: &[WorkspaceRecord],
) -> Result<(), InfrastructureError> {
    if records.is_empty() || !records.iter().any(|record| record.id == selected_id) {
        Err(InfrastructureError::RevisionConflict)
    } else {
        Ok(())
    }
}

fn database_error(source: rusqlite::Error) -> InfrastructureError {
    InfrastructureError::Database { source }
}
