//! 组合导入事务使用的协议包比较与写入原语。

use intercept_proxy_domain::ProtocolPackageRef;
use intercept_proxy_protocol_scripting::ProtocolPackageFiles;
use rusqlite::{Transaction, params};

use super::{
    InfrastructureError, StoredProtocolPackageFiles, StoredProtocolPackageHeader,
    StoredProtocolPackageValidation, database_error, identity::exact_external_package_exists,
    load_protocol_package, package_id_has_different_kind, protocol_package_kind_text,
};

/// 组合导入事务中的已恢复、已编译协议包。
#[derive(Clone, Debug)]
pub(crate) struct StoredProtocolPackageWrite {
    pub header: StoredProtocolPackageHeader,
    pub files: ProtocolPackageFiles,
}

#[derive(Debug, thiserror::Error)]
pub(crate) enum StoredProtocolPackageBundleError {
    #[error("相同协议包身份的内容不同")]
    IdentityConflict(ProtocolPackageRef),
    #[error(transparent)]
    Infrastructure(#[from] InfrastructureError),
}

/// 在调用者持有的组合事务内比较或安装一个协议包。
///
/// `enabled = None` 表示 Workspace 导入：相同内容复用并保留本机启用位，新安装使用
/// header 中的 `false`。`Some` 表示完整配置恢复：相同内容也按文档恢复启用位。
pub(crate) fn compare_or_insert_protocol_package(
    transaction: &Transaction<'_>,
    package: &StoredProtocolPackageWrite,
    enabled: Option<bool>,
) -> Result<(), StoredProtocolPackageBundleError> {
    if exact_external_package_exists(transaction, &package.header.package)? {
        return Err(StoredProtocolPackageBundleError::IdentityConflict(
            package.header.package.clone(),
        ));
    }
    if package_id_has_different_kind(transaction, &package.header.package, package.header.kind)? {
        return Err(StoredProtocolPackageBundleError::IdentityConflict(
            package.header.package.clone(),
        ));
    }
    if let Some(existing) = load_protocol_package(transaction, &package.header.package)? {
        if !same_immutable_content(&existing, package) {
            return Err(StoredProtocolPackageBundleError::IdentityConflict(
                package.header.package.clone(),
            ));
        }
        transaction
            .execute(
                "UPDATE protocol_packages
                 SET enabled = COALESCE(?3, enabled),
                     validation_state = 'valid', validation_error_code = NULL
                 WHERE package_id = ?1 AND version = ?2",
                params![
                    package.header.package.id.as_str(),
                    package.header.package.version.as_str(),
                    enabled,
                ],
            )
            .map_err(database_error)?;
        return Ok(());
    }
    insert_protocol_package(
        transaction,
        package,
        enabled.unwrap_or(package.header.enabled),
    )?;
    Ok(())
}

pub(super) fn same_immutable_content(
    existing: &super::StoredProtocolPackage,
    package: &StoredProtocolPackageWrite,
) -> bool {
    let expected_files = package
        .files
        .iter()
        .map(|(path, bytes)| (path.as_str().to_owned(), bytes.to_vec()))
        .collect::<Vec<_>>();
    existing.header.package == package.header.package
        && existing.header.name == package.header.name
        && existing.header.host_api == package.header.host_api
        && existing.header.kind == package.header.kind
        && existing.header.generation != uuid::Uuid::nil()
        && !matches!(
            existing.header.validation,
            StoredProtocolPackageValidation::Invalid(ref code) if code == "PERSISTENCE_CORRUPT"
        )
        && existing.files == StoredProtocolPackageFiles::Valid(expected_files)
}

pub(super) fn insert_protocol_package(
    transaction: &Transaction<'_>,
    package: &StoredProtocolPackageWrite,
    enabled: bool,
) -> Result<(), InfrastructureError> {
    transaction
        .execute(
            "INSERT INTO protocol_packages(
                package_id, version, name, host_api, kind, enabled,
                validation_state, validation_error_code, installed_at, generation
             ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, 'valid', NULL, ?7, ?8)",
            params![
                package.header.package.id.as_str(),
                package.header.package.version.as_str(),
                package.header.name,
                i64::from(package.header.host_api),
                protocol_package_kind_text(package.header.kind),
                enabled,
                package.header.installed_at.to_rfc3339(),
                package.header.generation.to_string(),
            ],
        )
        .map_err(database_error)?;
    for (path, contents) in package.files.iter() {
        transaction
            .execute(
                "INSERT INTO protocol_package_files(package_id, version, path, contents)
                 VALUES (?1, ?2, ?3, ?4)",
                params![
                    package.header.package.id.as_str(),
                    package.header.package.version.as_str(),
                    path.as_str(),
                    contents,
                ],
            )
            .map_err(database_error)?;
    }
    Ok(())
}
