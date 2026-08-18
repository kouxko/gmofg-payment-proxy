//! Prepared application-backup import models and the non-authoritative pending boundary.

use std::{fmt, io::Write, time::Duration};

use async_trait::async_trait;
use intercept_proxy_domain::{ProtocolPackageRef, ProxyWorkspace, Revision, WorkspaceId};
use serde::{Deserialize, Serialize};
use specta::Type;
use uuid::Uuid;

use crate::{
    AppResult, MigrationReport, PortableApplicationProtocolPackage, PortableCertificateMaterial,
    PortableSettings,
};

#[derive(
    Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize, Deserialize, Type,
)]
#[serde(transparent)]
pub struct ApplicationBackupImportToken(Uuid);

impl ApplicationBackupImportToken {
    #[must_use]
    pub const fn from_uuid(value: Uuid) -> Self {
        Self(value)
    }

    #[must_use]
    pub const fn as_uuid(self) -> Uuid {
        self.0
    }
}

#[derive(Clone, Eq, PartialEq)]
pub struct ApplicationBackupImportCandidate {
    pub selected_workspace_id: WorkspaceId,
    pub workspaces: Vec<ProxyWorkspace>,
    pub settings: PortableSettings,
    pub protocol_packages: Vec<PortableApplicationProtocolPackage>,
    pub certificate_materials: Vec<PortableCertificateMaterial>,
    pub migration_report: MigrationReport,
}

impl ApplicationBackupImportCandidate {
    pub fn logical_bytes(&self) -> AppResult<u64> {
        let mut structured = LogicalByteCounter::default();
        serde_json::to_writer(
            &mut structured,
            &(
                self.selected_workspace_id,
                &self.workspaces,
                &self.settings,
                &self.protocol_packages,
                &self.certificate_materials,
                &self.migration_report,
            ),
        )
        .map_err(|_| {
            crate::AppError::new(
                "APPLICATION_BACKUP_IMPORT_INVALID",
                "应用备份候选的逻辑大小无法安全计算。",
            )
        })?;
        Ok(structured.0)
    }
}

#[derive(Default)]
struct LogicalByteCounter(u64);

impl Write for LogicalByteCounter {
    fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
        self.0 = self
            .0
            .checked_add(bytes.len() as u64)
            .ok_or_else(|| std::io::Error::other("logical byte count overflow"))?;
        Ok(bytes.len())
    }

    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

impl fmt::Debug for ApplicationBackupImportCandidate {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("ApplicationBackupImportCandidate")
            .field("selected_workspace_id", &self.selected_workspace_id)
            .field("workspace_count", &self.workspaces.len())
            .field("protocol_package_count", &self.protocol_packages.len())
            .field(
                "certificate_material_count",
                &self.certificate_materials.len(),
            )
            .field("migration_report", &self.migration_report)
            .finish_non_exhaustive()
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ApplicationBackupWorkspaceBaseline {
    pub workspace_id: WorkspaceId,
    pub revision: Revision,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ApplicationBackupProtocolPackageBaseline {
    pub package: ProtocolPackageRef,
    pub enabled: bool,
    pub generation: Uuid,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ApplicationBackupImportBaseline {
    pub selected_workspace_id: WorkspaceId,
    pub workspaces: Vec<ApplicationBackupWorkspaceBaseline>,
    pub settings_revision: Revision,
    pub protocol_packages: Vec<ApplicationBackupProtocolPackageBaseline>,
    pub listener_certificate_generation: [u8; 32],
}

#[derive(Clone, Eq, PartialEq)]
pub struct PreparedApplicationBackup {
    pub candidate: ApplicationBackupImportCandidate,
    pub baseline: ApplicationBackupImportBaseline,
}

impl fmt::Debug for PreparedApplicationBackup {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("PreparedApplicationBackup")
            .field("candidate", &self.candidate)
            .field("baseline_workspace_count", &self.baseline.workspaces.len())
            .field(
                "baseline_protocol_package_count",
                &self.baseline.protocol_packages.len(),
            )
            .finish_non_exhaustive()
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Type)]
pub struct ApplicationBackupPackagePreview {
    pub package: ProtocolPackageRef,
    pub enabled: bool,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Type)]
#[allow(clippy::struct_excessive_bools)]
pub struct ApplicationBackupReplacementScope {
    pub replaces_all_workspaces: bool,
    pub replaces_selected_workspace: bool,
    pub replaces_portable_settings: bool,
    pub replaces_protocol_package_registry: bool,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Type)]
pub struct ApplicationBackupImportPreview {
    pub token: ApplicationBackupImportToken,
    pub expires_in_seconds: u64,
    pub workspace_count: usize,
    pub protocol_package_count: usize,
    pub enabled_protocol_package_count: usize,
    pub portable_material_count: usize,
    pub protocol_packages: Vec<ApplicationBackupPackagePreview>,
    pub replacement_scope: ApplicationBackupReplacementScope,
    pub migration_report: MigrationReport,
    pub warnings: Vec<String>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Type)]
pub struct ApplicationBackupImportCommitOutcome {
    pub workspace_count: usize,
    pub protocol_package_count: usize,
    pub enabled_protocol_package_count: usize,
    pub portable_material_count: usize,
    pub requires_restart: bool,
}

#[async_trait]
pub trait ApplicationBackupImportPreparePort: Send + Sync + fmt::Debug {
    async fn read(&self, bytes: Vec<u8>) -> AppResult<ApplicationBackupImportCandidate>;

    async fn retain(
        &self,
        prepared: PreparedApplicationBackup,
    ) -> AppResult<(ApplicationBackupImportToken, Duration)>;

    async fn discard(&self, token: ApplicationBackupImportToken) -> AppResult<()>;

    async fn take(
        &self,
        token: ApplicationBackupImportToken,
    ) -> AppResult<PreparedApplicationBackup>;
}
