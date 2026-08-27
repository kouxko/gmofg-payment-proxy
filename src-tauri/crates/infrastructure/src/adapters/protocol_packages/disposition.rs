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
    pub(in crate::adapters) async fn prepared_disposition_async(
        &self,
        prepared: &PreparedProtocolPackage,
    ) -> Result<PreparedProtocolPackageDisposition, ProtocolPackageStorageError> {
        let package = prepared.compiled.package().clone();
        let manifest_name = prepared.compiled.manifest().package().name().to_owned();
        let host_api = prepared.compiled.manifest().api();
        let expected_files = prepared
            .files
            .iter()
            .map(|(path, bytes)| (path.as_str().to_owned(), bytes.to_vec()))
            .collect::<Vec<_>>();
        let Some(existing) = self
            .executor
            .execute(move |store| {
                store
                    .load_protocol_package(&package)
                    .map_err(ProtocolPackageStorageError::from)
            })
            .await?
        else {
            return Ok(PreparedProtocolPackageDisposition::New);
        };
        let reusable = existing.header.name == manifest_name
            && existing.header.host_api == host_api
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
