//! 协议包注册表与可移植 Workspace/应用配置的组合事务。
//!
//! 文件恢复、Manifest/Schema/Rhai 编译都必须在进入这里之前完成。本模块只负责在一个
//! `IMMEDIATE` 事务内再次比较持久化身份，并把协议包与配置行一起提交或一起回滚。

use rusqlite::{Transaction, TransactionBehavior, params};

use super::{
    InfrastructureError, SqliteStore, Utc, Uuid, Value, WorkspaceRecord, current_revision,
    protocol_packages::{
        StoredProtocolPackageBundleError, StoredProtocolPackageWrite,
        compare_or_insert_protocol_package,
    },
    revision_to_i64,
};

impl SqliteStore {
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

        // 完整应用导入的备份是协议包注册表的权威快照。同身份的本地包即使内容不同，
        // 也必须由备份版本替换；删除和后续写入位于同一事务，任何失败都会恢复原注册表。
        transaction
            .execute("DELETE FROM protocol_packages", [])
            .map_err(database_error)?;
        for package in packages {
            compare_or_insert_protocol_package(
                &transaction,
                package,
                Some(package.header.enabled),
            )?;
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
        builtin_package: Option<&StoredProtocolPackageWrite>,
    ) -> Result<(), InfrastructureError> {
        if records.len() != 1 || records[0].id != selected_id {
            return Err(InfrastructureError::RevisionConflict);
        }
        let mut connection = self.connection.lock();
        let transaction = connection
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .map_err(database_error)?;
        transaction
            .execute_batch(
                "DELETE FROM protected_secrets;
                 DELETE FROM certificate_material;
                 UPDATE certificate_state SET revision = revision + 1 WHERE singleton_id = 1;
                 DELETE FROM android_runtime_owners;
                 DELETE FROM protocol_packages;
                 DELETE FROM workspaces;",
            )
            .map_err(database_error)?;
        replace_workspaces_and_settings(&transaction, selected_id, records, settings)?;
        if let Some(package) = builtin_package {
            super::protocol_packages::compare_or_insert_protocol_package(
                &transaction,
                package,
                Some(true),
            )
            .map_err(|error| match error {
                super::protocol_packages::StoredProtocolPackageBundleError::Infrastructure(
                    error,
                ) => error,
                super::protocol_packages::StoredProtocolPackageBundleError::IdentityConflict(_) => {
                    InfrastructureError::PersistenceCorrupt {
                        entity: "builtin_protocol_package",
                        message: "重置事务无法写入官方协议包".into(),
                    }
                }
            })?;
            transaction
                .execute(
                    "INSERT INTO application_feature_state(feature_key, initialized_at)
                     VALUES (?1, ?2)
                     ON CONFLICT(feature_key) DO UPDATE
                     SET initialized_at = excluded.initialized_at",
                    params![
                        super::protocol_packages::BUILTIN_ISO8583_FEATURE_KEY,
                        chrono::Utc::now().to_rfc3339()
                    ],
                )
                .map_err(database_error)?;
        }
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
