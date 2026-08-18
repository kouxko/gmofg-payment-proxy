//! 已安装协议包的精确引用解析与只读校验。

use intercept_proxy_domain::ProtocolPackageRef;

use crate::sqlite::protocol_packages::StoredProtocolPackageFiles;

use super::{
    PreparedProtocolPackage, ProtocolPackageRepositoryAdapter, ProtocolPackageStorageError,
    compare_identities,
};

pub(super) fn prepare_installed_references(
    repository: &ProtocolPackageRepositoryAdapter,
    mut referenced: Vec<ProtocolPackageRef>,
) -> Result<Vec<PreparedProtocolPackage>, ProtocolPackageStorageError> {
    referenced.sort_by(compare_identities);
    referenced
        .into_iter()
        .map(|package| {
            let stored = repository
                .store
                .load_protocol_package(&package)?
                .ok_or_else(|| ProtocolPackageStorageError::NotFound {
                    package: package.clone(),
                })?;
            let rows = match stored.files {
                StoredProtocolPackageFiles::Valid(rows) => rows,
                StoredProtocolPackageFiles::Rejected(code) => {
                    return Err(ProtocolPackageStorageError::StoredPackageInvalid {
                        package,
                        code: code.to_owned(),
                    });
                }
            };
            repository.prepare_rows(&package, rows)
        })
        .collect()
}
