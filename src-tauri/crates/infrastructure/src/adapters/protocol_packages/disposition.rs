//! 已验证包相对当前注册表快照的只读处置判断。

use uuid::Uuid;

use super::{
    PreparedProtocolPackage, ProtocolPackageRepositoryAdapter, ProtocolPackageStorageError,
};
use crate::sqlite::protocol_packages::{
    StoredProtocolPackageFiles, StoredProtocolPackageValidation,
};

/// prepare 阶段针对当前存储快照得到的只读身份处置。
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(in crate::adapters) enum PreparedProtocolPackageDisposition {
    New,
    Reusable,
    IdentityConflict,
}

impl ProtocolPackageRepositoryAdapter {
    /// 只读比较已验证包与当前精确身份；最终 commit 仍必须在写事务中重复这项判断。
    pub(in crate::adapters) fn prepared_disposition(
        &self,
        prepared: &PreparedProtocolPackage,
    ) -> Result<PreparedProtocolPackageDisposition, ProtocolPackageStorageError> {
        let package = prepared.compiled.package();
        let Some(existing) = self.store.load_protocol_package(package)? else {
            return Ok(PreparedProtocolPackageDisposition::New);
        };
        let manifest = prepared.compiled.manifest();
        let expected_files = prepared
            .files
            .iter()
            .map(|(path, bytes)| (path.as_str().to_owned(), bytes.to_vec()))
            .collect::<Vec<_>>();
        let reusable = existing.header.name == manifest.package().name()
            && existing.header.host_api == manifest.api()
            && existing.header.generation != Uuid::nil()
            && !matches!(
                existing.header.validation,
                StoredProtocolPackageValidation::Invalid(ref code)
                    if code == "PERSISTENCE_CORRUPT"
            )
            && existing.files == StoredProtocolPackageFiles::Valid(expected_files);
        Ok(if reusable {
            PreparedProtocolPackageDisposition::Reusable
        } else {
            PreparedProtocolPackageDisposition::IdentityConflict
        })
    }
}
