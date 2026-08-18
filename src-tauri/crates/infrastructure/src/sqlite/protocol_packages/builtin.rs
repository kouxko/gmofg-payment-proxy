//! 官方内置协议包的一次性迁移与显式恢复事务。

use chrono::Utc;
use intercept_proxy_protocol_scripting::ProtocolPackageFiles;
use rusqlite::{OptionalExtension, TransactionBehavior, params};
use uuid::Uuid;

use super::{
    SqliteStore, StoredProtocolPackageBundleError, StoredProtocolPackageHeader,
    StoredProtocolPackageWrite, database_error, insert_protocol_package, load_protocol_package,
    package_id_has_different_kind, same_immutable_content,
};

pub(crate) const BUILTIN_ISO8583_FEATURE_KEY: &str = "builtin_iso8583_ascii_standard_v1";

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum StoredBuiltinSeedOutcome {
    AlreadyInitialized,
    Ready(Uuid),
}

impl SqliteStore {
    /// 首次功能迁移：安装并启用官方精确身份，随后与 feature marker 一起提交。
    pub(crate) fn seed_builtin_protocol_package(
        &self,
        header: &StoredProtocolPackageHeader,
        files: &ProtocolPackageFiles,
    ) -> Result<StoredBuiltinSeedOutcome, StoredProtocolPackageBundleError> {
        let mut connection = self.connection.lock();
        let transaction = connection
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .map_err(database_error)?;
        let initialized = transaction
            .query_row(
                "SELECT 1 FROM application_feature_state WHERE feature_key = ?1",
                [BUILTIN_ISO8583_FEATURE_KEY],
                |_| Ok(()),
            )
            .optional()
            .map_err(database_error)?
            .is_some();
        if initialized {
            transaction.commit().map_err(database_error)?;
            return Ok(StoredBuiltinSeedOutcome::AlreadyInitialized);
        }

        if package_id_has_different_kind(&transaction, &header.package, header.kind)? {
            return Err(StoredProtocolPackageBundleError::IdentityConflict(
                header.package.clone(),
            ));
        }

        let write = StoredProtocolPackageWrite {
            header: StoredProtocolPackageHeader {
                enabled: true,
                ..header.clone()
            },
            files: files.clone(),
        };
        if let Some(existing) = load_protocol_package(&transaction, &header.package)? {
            if !same_immutable_content(&existing, &write) {
                return Err(StoredProtocolPackageBundleError::IdentityConflict(
                    header.package.clone(),
                ));
            }
            transaction
                .execute(
                    "UPDATE protocol_packages
                     SET enabled = 1, validation_state = 'valid', validation_error_code = NULL
                     WHERE package_id = ?1 AND version = ?2",
                    params![header.package.id.as_str(), header.package.version.as_str()],
                )
                .map_err(database_error)?;
        } else {
            insert_protocol_package(&transaction, &write, true)?;
        }
        transaction
            .execute(
                "INSERT INTO application_feature_state(feature_key, initialized_at)
                 VALUES (?1, ?2)",
                params![BUILTIN_ISO8583_FEATURE_KEY, Utc::now().to_rfc3339()],
            )
            .map_err(database_error)?;
        let generation = load_protocol_package(&transaction, &header.package)?
            .expect("built-in package was inserted or verified in the same transaction")
            .header
            .generation;
        transaction.commit().map_err(database_error)?;
        Ok(StoredBuiltinSeedOutcome::Ready(generation))
    }

    /// 显式恢复可替换被删除或损坏的官方身份，但只在整个文件集写入成功后提交。
    pub(crate) fn restore_builtin_protocol_package(
        &self,
        header: &StoredProtocolPackageHeader,
        files: &ProtocolPackageFiles,
    ) -> Result<Uuid, StoredProtocolPackageBundleError> {
        let mut connection = self.connection.lock();
        let transaction = connection
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .map_err(database_error)?;
        if package_id_has_different_kind(&transaction, &header.package, header.kind)? {
            return Err(StoredProtocolPackageBundleError::IdentityConflict(
                header.package.clone(),
            ));
        }
        transaction
            .execute(
                "DELETE FROM protocol_packages WHERE package_id = ?1 AND version = ?2",
                params![header.package.id.as_str(), header.package.version.as_str()],
            )
            .map_err(database_error)?;
        let write = StoredProtocolPackageWrite {
            header: StoredProtocolPackageHeader {
                enabled: true,
                ..header.clone()
            },
            files: files.clone(),
        };
        insert_protocol_package(&transaction, &write, true)?;
        transaction
            .execute(
                "INSERT INTO application_feature_state(feature_key, initialized_at)
                 VALUES (?1, ?2)
                 ON CONFLICT(feature_key) DO UPDATE SET initialized_at = excluded.initialized_at",
                params![BUILTIN_ISO8583_FEATURE_KEY, Utc::now().to_rfc3339()],
            )
            .map_err(database_error)?;
        transaction.commit().map_err(database_error)?;
        Ok(header.generation)
    }
}
