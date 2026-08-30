//! 协议包的 `SQLite` 原子存储。
//!
//! 这里只保存 Manifest 元数据和规范化文件字节，不保存 Rhai AST 或运行时 Engine。文件在写入前已由
//! `protocol-scripting` 完整校验，读取后仍必须由上层重新执行路径门禁和编译，数据库内容不能被当作可信输入。

use intercept_proxy_domain::ProtocolPackageRef;
#[cfg(test)]
use intercept_proxy_protocol_scripting::ProtocolPackageFiles;
use intercept_proxy_protocol_scripting::{
    MAX_ARCHIVE_ENTRIES_LIMIT, MAX_FILE_BYTES_LIMIT, MAX_PACKAGE_FILE_PATH_BYTES,
    MAX_TOTAL_BYTES_LIMIT, ProtocolPackageKind,
};
#[cfg(test)]
use rusqlite::TransactionBehavior;
use rusqlite::{OptionalExtension, Transaction, params};
#[cfg(test)]
use uuid::Uuid;

use super::{InfrastructureError, SqliteStore};

#[cfg(test)]
mod test_support;

#[path = "protocol_packages/rows.rs"]
mod rows;
#[cfg(test)]
pub(crate) use rows::StoredProtocolPackageInstallOutcome;
pub(crate) use rows::{
    StoredProtocolPackage, StoredProtocolPackageFiles, StoredProtocolPackageHeader,
    StoredProtocolPackageValidation,
};
use rows::{parse_header, read_header_row};

impl SqliteStore {
    pub(crate) fn list_protocol_package_headers(
        &self,
    ) -> Result<Vec<StoredProtocolPackageHeader>, InfrastructureError> {
        let connection = self.connection.lock();
        let mut statement = connection
            .prepare(
                "SELECT package_id, version, name, host_api, kind, enabled,
                        validation_state, validation_error_code, installed_at, generation
                 FROM protocol_packages ORDER BY package_id, version",
            )
            .map_err(database_error)?;
        let rows = statement
            .query_map([], read_header_row)
            .map_err(database_error)?;
        rows.map(|row| row.map_err(database_error).and_then(parse_header))
            .collect()
    }

    pub(crate) fn load_protocol_package(
        &self,
        package: &ProtocolPackageRef,
    ) -> Result<Option<StoredProtocolPackage>, InfrastructureError> {
        let mut connection = self.connection.lock();
        let transaction = connection.transaction().map_err(database_error)?;
        let loaded = load_protocol_package(&transaction, package)?;
        transaction.commit().map_err(database_error)?;
        Ok(loaded)
    }

    pub(crate) fn load_protocol_package_header(
        &self,
        package: &ProtocolPackageRef,
    ) -> Result<Option<StoredProtocolPackageHeader>, InfrastructureError> {
        let connection = self.connection.lock();
        load_protocol_package_header(&connection, package)
    }

    #[cfg(test)]
    pub(crate) fn install_protocol_package(
        &self,
        header: &StoredProtocolPackageHeader,
        files: &ProtocolPackageFiles,
    ) -> Result<StoredProtocolPackageInstallOutcome, InfrastructureError> {
        let mut connection = self.connection.lock();
        // IMMEDIATE 在读取“是否存在”之前取得写保留锁。不同 SqliteStore 连接同时导入同一身份时，
        // 后到者只能在前一个事务提交后比较完整内容，不会观察到只有 header 的半安装状态。
        let transaction = connection
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .map_err(database_error)?;
        if exact_external_package_exists(&transaction, &header.package)? {
            transaction.commit().map_err(database_error)?;
            return Ok(StoredProtocolPackageInstallOutcome::IdentityConflict);
        }
        if package_id_has_different_kind(&transaction, &header.package, header.kind)? {
            transaction.commit().map_err(database_error)?;
            return Ok(StoredProtocolPackageInstallOutcome::IdentityConflict);
        }
        if let Some(existing) = load_protocol_package(&transaction, &header.package)? {
            let expected_files = files
                .iter()
                .map(|(path, bytes)| (path.as_str().to_owned(), bytes.to_vec()))
                .collect::<Vec<_>>();
            let same_immutable_content = existing.header.package == header.package
                && existing.header.name == header.name
                && existing.header.host_api == header.host_api
                && existing.header.kind == header.kind
                && existing.header.generation != Uuid::nil()
                && !matches!(
                    existing.header.validation,
                    StoredProtocolPackageValidation::Invalid(ref code)
                        if code == "PERSISTENCE_CORRUPT"
                )
                && existing.files == StoredProtocolPackageFiles::Valid(expected_files);
            transaction.commit().map_err(database_error)?;
            return Ok(if same_immutable_content {
                StoredProtocolPackageInstallOutcome::Reused(existing.header.generation)
            } else {
                StoredProtocolPackageInstallOutcome::IdentityConflict
            });
        }

        transaction
            .execute(
                "INSERT INTO protocol_packages(
                    package_id, version, name, host_api, kind, enabled,
                    validation_state, validation_error_code, installed_at, generation
                 ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, 'valid', NULL, ?7, ?8)",
                params![
                    header.package.id.as_str(),
                    header.package.version.as_str(),
                    header.name,
                    i64::from(header.host_api),
                    protocol_package_kind_text(header.kind),
                    header.enabled,
                    header.installed_at.to_rfc3339(),
                    header.generation.to_string(),
                ],
            )
            .map_err(database_error)?;
        for (path, contents) in files.iter() {
            transaction
                .execute(
                    "INSERT INTO protocol_package_files(package_id, version, path, contents)
                     VALUES (?1, ?2, ?3, ?4)",
                    params![
                        header.package.id.as_str(),
                        header.package.version.as_str(),
                        path.as_str(),
                        contents,
                    ],
                )
                .map_err(database_error)?;
        }
        transaction.commit().map_err(database_error)?;
        Ok(StoredProtocolPackageInstallOutcome::Installed(
            header.generation,
        ))
    }

    pub(crate) fn set_protocol_package_enabled(
        &self,
        package: &ProtocolPackageRef,
        enabled: bool,
    ) -> Result<bool, InfrastructureError> {
        let connection = self.connection.lock();
        connection
            .execute(
                "UPDATE protocol_packages SET enabled = ?3
                 WHERE package_id = ?1 AND version = ?2",
                params![package.id.as_str(), package.version.as_str(), enabled],
            )
            .map(|affected| affected == 1)
            .map_err(database_error)
    }

    pub(crate) fn set_protocol_package_validation(
        &self,
        package: &ProtocolPackageRef,
        error_code: Option<&str>,
    ) -> Result<bool, InfrastructureError> {
        let connection = self.connection.lock();
        let (state, code) = error_code.map_or(("valid", None), |code| ("invalid", Some(code)));
        connection
            .execute(
                "UPDATE protocol_packages
                 SET validation_state = ?3, validation_error_code = ?4
                 WHERE package_id = ?1 AND version = ?2",
                params![package.id.as_str(), package.version.as_str(), state, code],
            )
            .map(|affected| affected == 1)
            .map_err(database_error)
    }

    pub(crate) fn delete_protocol_package(
        &self,
        package: &ProtocolPackageRef,
    ) -> Result<bool, InfrastructureError> {
        let connection = self.connection.lock();
        connection
            .execute(
                "DELETE FROM protocol_packages WHERE package_id = ?1 AND version = ?2",
                params![package.id.as_str(), package.version.as_str()],
            )
            .map(|affected| affected == 1)
            .map_err(database_error)
    }
}

pub(super) fn load_protocol_package(
    transaction: &Transaction<'_>,
    package: &ProtocolPackageRef,
) -> Result<Option<StoredProtocolPackage>, InfrastructureError> {
    let header = load_protocol_package_header(transaction, package)?;
    let Some(header) = header else {
        return Ok(None);
    };
    if let Some(code) = preflight_protocol_package_files(transaction, package)? {
        return Ok(Some(StoredProtocolPackage {
            header,
            files: StoredProtocolPackageFiles::Rejected(code),
        }));
    }
    let mut statement = transaction
        .prepare(
            "SELECT path, contents FROM protocol_package_files
             WHERE package_id = ?1 AND version = ?2 ORDER BY path",
        )
        .map_err(database_error)?;
    let rows = statement
        .query_map(
            params![package.id.as_str(), package.version.as_str()],
            |row| Ok((row.get::<_, String>(0)?, row.get::<_, Vec<u8>>(1)?)),
        )
        .map_err(database_error)?;
    let files = rows
        .map(|row| row.map_err(database_error))
        .collect::<Result<Vec<_>, _>>()?;
    Ok(Some(StoredProtocolPackage {
        header,
        files: StoredProtocolPackageFiles::Valid(files),
    }))
}

fn load_protocol_package_header(
    connection: &rusqlite::Connection,
    package: &ProtocolPackageRef,
) -> Result<Option<StoredProtocolPackageHeader>, InfrastructureError> {
    connection
        .query_row(
            "SELECT package_id, version, name, host_api, kind, enabled,
                    validation_state, validation_error_code, installed_at, generation
             FROM protocol_packages WHERE package_id = ?1 AND version = ?2",
            params![package.id.as_str(), package.version.as_str()],
            read_header_row,
        )
        .optional()
        .map_err(database_error)?
        .map(parse_header)
        .transpose()
}

const fn protocol_package_kind_text(kind: ProtocolPackageKind) -> &'static str {
    match kind {
        ProtocolPackageKind::Http => "http",
        ProtocolPackageKind::Socket => "socket",
    }
}

fn package_id_has_different_kind(
    transaction: &Transaction<'_>,
    package: &ProtocolPackageRef,
    kind: ProtocolPackageKind,
) -> Result<bool, InfrastructureError> {
    transaction
        .query_row(
            "SELECT EXISTS(
                 SELECT 1 FROM protocol_packages
                 WHERE package_id = ?1 AND kind <> ?2
             )",
            params![package.id.as_str(), protocol_package_kind_text(kind)],
            |row| row.get(0),
        )
        .map_err(database_error)
}

#[path = "protocol_packages/bundle.rs"]
mod bundle;
pub(crate) use bundle::{
    StoredProtocolPackageBundleError, StoredProtocolPackageWrite,
    compare_or_insert_protocol_package,
};
use bundle::{insert_protocol_package, same_immutable_content};
#[path = "protocol_packages/builtin.rs"]
mod builtin;
pub(crate) use builtin::{BUILTIN_ISO8583_FEATURE_KEY, StoredBuiltinSeedOutcome};
#[path = "protocol_packages/identity.rs"]
mod identity;
#[cfg(test)]
use identity::exact_external_package_exists;

fn preflight_protocol_package_files(
    transaction: &Transaction<'_>,
    package: &ProtocolPackageRef,
) -> Result<Option<&'static str>, InfrastructureError> {
    // 先让 SQLite 只计算整数聚合，再决定是否读取 BLOB。即使数据库文件被外部篡改，宿主也不会在
    // `restore_protocol_package_files` 获得控制前就分配任意大的 path/content。
    let (count, max_path, max_file, total): (i64, i64, i64, i64) = transaction
        .query_row(
            "SELECT COUNT(*),
                    COALESCE(MAX(length(CAST(path AS BLOB))), 0),
                    COALESCE(MAX(length(contents)), 0),
                    COALESCE(SUM(length(contents)), 0)
             FROM protocol_package_files WHERE package_id = ?1 AND version = ?2",
            params![package.id.as_str(), package.version.as_str()],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?)),
        )
        .map_err(database_error)?;
    Ok(protocol_package_preflight_error_code(
        count, max_path, max_file, total,
    ))
}

pub(crate) fn protocol_package_preflight_error_code(
    count: i64,
    max_path: i64,
    max_file: i64,
    total: i64,
) -> Option<&'static str> {
    if count < 0 || usize::try_from(count).unwrap_or(usize::MAX) > MAX_ARCHIVE_ENTRIES_LIMIT {
        Some("TOO_MANY_ENTRIES")
    } else if max_path < 0
        || usize::try_from(max_path).unwrap_or(usize::MAX) > MAX_PACKAGE_FILE_PATH_BYTES
    {
        Some("INVALID_PATH")
    } else if max_file < 0 || u64::try_from(max_file).unwrap_or(u64::MAX) > MAX_FILE_BYTES_LIMIT {
        Some("FILE_TOO_LARGE")
    } else if total < 0 || u64::try_from(total).unwrap_or(u64::MAX) > MAX_TOTAL_BYTES_LIMIT {
        Some("TOTAL_TOO_LARGE")
    } else {
        None
    }
}

pub(super) fn database_error(source: rusqlite::Error) -> InfrastructureError {
    InfrastructureError::Database { source }
}
