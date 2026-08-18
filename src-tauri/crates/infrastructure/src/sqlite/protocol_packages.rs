//! 协议包的 `SQLite` 原子存储。
//!
//! 这里只保存 Manifest 元数据和规范化文件字节，不保存 Rhai AST 或运行时 Engine。文件在写入前已由
//! `protocol-scripting` 完整校验，读取后仍必须由上层重新执行路径门禁和编译，数据库内容不能被当作可信输入。

use chrono::{DateTime, Utc};
use intercept_proxy_domain::{ProtocolPackageId, ProtocolPackageRef, ProtocolPackageVersion};
use intercept_proxy_protocol_scripting::{
    MAX_ARCHIVE_ENTRIES_LIMIT, MAX_FILE_BYTES_LIMIT, MAX_PACKAGE_FILE_PATH_BYTES,
    MAX_TOTAL_BYTES_LIMIT, ProtocolPackageFiles, ProtocolPackageKind,
};
use rusqlite::{OptionalExtension, Transaction, TransactionBehavior, params};
use uuid::Uuid;

use super::{InfrastructureError, SqliteStore};

#[cfg(test)]
mod test_support;

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) enum StoredProtocolPackageValidation {
    Valid,
    Invalid(String),
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct StoredProtocolPackageHeader {
    pub package: ProtocolPackageRef,
    pub name: String,
    pub host_api: u32,
    pub kind: ProtocolPackageKind,
    pub enabled: bool,
    pub validation: StoredProtocolPackageValidation,
    pub installed_at: DateTime<Utc>,
    /// 每次真正安装生成的新代际；幂等重入保留旧值，删除后重装必定变化。
    pub generation: Uuid,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct StoredProtocolPackage {
    pub header: StoredProtocolPackageHeader,
    pub files: StoredProtocolPackageFiles,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) enum StoredProtocolPackageFiles {
    Valid(Vec<(String, Vec<u8>)>),
    /// `SQLite` 被外部破坏后可能超过正常导入上限；预检失败时不读取任何 BLOB。
    Rejected(&'static str),
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum StoredProtocolPackageInstallOutcome {
    Installed(Uuid),
    Reused(Uuid),
    IdentityConflict,
}

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

type HeaderRow = (
    String,
    String,
    String,
    i64,
    String,
    i64,
    String,
    Option<String>,
    String,
    String,
);

fn read_header_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<HeaderRow> {
    Ok((
        row.get(0)?,
        row.get(1)?,
        row.get(2)?,
        row.get(3)?,
        row.get(4)?,
        row.get(5)?,
        row.get(6)?,
        row.get(7)?,
        row.get(8)?,
        row.get(9)?,
    ))
}

fn parse_header(row: HeaderRow) -> Result<StoredProtocolPackageHeader, InfrastructureError> {
    let (id, version, name, host_api, kind, enabled, state, error_code, installed_at, generation) =
        row;
    let package = ProtocolPackageRef {
        id: ProtocolPackageId::new(id).map_err(|_| corrupt_protocol_package("package_id 无效"))?,
        version: ProtocolPackageVersion::new(version)
            .map_err(|_| corrupt_protocol_package("version 无效"))?,
    };
    let mut metadata_corrupt = false;
    let name = if valid_display_name(&name) {
        name
    } else {
        metadata_corrupt = true;
        "Invalid protocol package".to_owned()
    };
    let host_api = u32::try_from(host_api).unwrap_or_else(|_| {
        metadata_corrupt = true;
        0
    });
    let kind = match kind.as_str() {
        "http" => ProtocolPackageKind::Http,
        "socket" => ProtocolPackageKind::Socket,
        _ => {
            metadata_corrupt = true;
            ProtocolPackageKind::Http
        }
    };
    let enabled = match enabled {
        0 => false,
        1 => true,
        _ => {
            metadata_corrupt = true;
            false
        }
    };
    let validation = match (state.as_str(), error_code) {
        ("valid", None) => StoredProtocolPackageValidation::Valid,
        ("invalid", Some(code)) if valid_error_code(&code) => {
            StoredProtocolPackageValidation::Invalid(code)
        }
        _ => {
            metadata_corrupt = true;
            StoredProtocolPackageValidation::Invalid("PERSISTENCE_CORRUPT".to_owned())
        }
    };
    let installed_at = DateTime::parse_from_rfc3339(&installed_at).map_or_else(
        |_| {
            metadata_corrupt = true;
            DateTime::<Utc>::UNIX_EPOCH
        },
        |value| value.with_timezone(&Utc),
    );
    let generation = Uuid::parse_str(&generation).unwrap_or_else(|_| {
        metadata_corrupt = true;
        Uuid::nil()
    });
    let validation = if metadata_corrupt {
        StoredProtocolPackageValidation::Invalid("PERSISTENCE_CORRUPT".to_owned())
    } else {
        validation
    };
    Ok(StoredProtocolPackageHeader {
        package,
        name,
        host_api,
        kind,
        enabled,
        validation,
        installed_at,
        generation,
    })
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

fn valid_error_code(code: &str) -> bool {
    !code.is_empty()
        && code.len() <= 64
        && code
            .bytes()
            .all(|byte| byte.is_ascii_uppercase() || byte.is_ascii_digit() || byte == b'_')
}

fn valid_display_name(name: &str) -> bool {
    !name.trim().is_empty() && name.chars().count() <= 128 && !name.chars().any(char::is_control)
}

pub(super) fn database_error(source: rusqlite::Error) -> InfrastructureError {
    InfrastructureError::Database { source }
}

fn corrupt_protocol_package(message: impl Into<String>) -> InfrastructureError {
    InfrastructureError::PersistenceCorrupt {
        entity: "protocol_package",
        message: message.into(),
    }
}
