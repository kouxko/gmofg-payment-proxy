//! 无协议包载荷的历史文档所需的本机精确引用验证。

use std::collections::{BTreeSet, HashSet};

use intercept_proxy_application::{
    APPLICATION_CONFIGURATION_FORMAT_VERSION, AppError, AppResult,
    ApplicationConfigurationDocument, ProxyWorkspace, validate_certificate_materials,
    validate_portable_certificate_references,
};
use intercept_proxy_domain::{ListenerDataPlane, ProtocolPackageRef, SocketPayloadProcessing};

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

pub(super) fn referenced_packages(workspaces: &[ProxyWorkspace]) -> Vec<ProtocolPackageRef> {
    let listeners = workspaces.iter().flat_map(|workspace| {
        workspace.listeners.iter().filter_map(|listener| {
            let ListenerDataPlane::Socket(socket) = &listener.data_plane else {
                return None;
            };
            let SocketPayloadProcessing::Scripted(scripted) = &socket.processing else {
                return None;
            };
            Some(scripted.package.clone())
        })
    });
    let rules = workspaces.iter().flat_map(|workspace| {
        workspace
            .socket_rules
            .iter()
            .map(|rule| rule.package().clone())
    });
    listeners
        .chain(rules)
        .collect::<HashSet<_>>()
        .into_iter()
        .collect()
}

pub(super) fn validate_application_document(
    document: &ApplicationConfigurationDocument,
) -> AppResult<()> {
    if document.format_version != APPLICATION_CONFIGURATION_FORMAT_VERSION
        || !document.protocol_packages.is_empty()
        || document.workspaces.is_empty()
    {
        return Err(AppError::new(
            "APPLICATION_CONFIGURATION_INVALID",
            "历史完整配置的迁移结构无效。",
        ));
    }
    let mut ids = BTreeSet::new();
    for workspace in &document.workspaces {
        if !ids.insert(workspace.id) {
            return Err(AppError::new(
                "APPLICATION_CONFIGURATION_INVALID",
                "完整配置中的 Workspace ID 不能重复。",
            ));
        }
        workspace.validate().map_err(AppError::from)?;
        validate_portable_certificate_references(workspace)?;
    }
    if !ids.contains(&document.selected_workspace_id) {
        return Err(AppError::new(
            "APPLICATION_CONFIGURATION_INVALID",
            "当前选中的 Workspace 不存在于文档中。",
        ));
    }
    validate_certificate_materials(&document.workspaces, &document.certificate_materials)
}
