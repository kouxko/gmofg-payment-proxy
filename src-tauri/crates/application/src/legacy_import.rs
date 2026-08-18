//! Explicit prepare/commit boundary for supported legacy JSON documents.

use std::{fmt, time::Duration};

use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use specta::Type;
use uuid::Uuid;

use crate::{
    AppResult, ApplicationBackupImportBaseline, ApplicationConfigurationDocument, MigrationReport,
    WorkspaceDocument,
};

#[derive(
    Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize, Deserialize, Type,
)]
#[serde(transparent)]
pub struct LegacyImportToken(Uuid);

impl LegacyImportToken {
    #[must_use]
    pub const fn from_uuid(value: Uuid) -> Self {
        Self(value)
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Type)]
#[serde(rename_all = "snake_case")]
pub enum LegacyImportKind {
    ApplicationConfiguration,
    Workspace,
}

#[derive(Clone, Eq, PartialEq)]
pub enum LegacyImportCandidate {
    ApplicationConfiguration {
        source_version: u16,
        document: ApplicationConfigurationDocument,
        migration_report: MigrationReport,
        baseline: ApplicationBackupImportBaseline,
    },
    Workspace {
        source_version: u16,
        document: WorkspaceDocument,
        migration_report: MigrationReport,
    },
}

impl fmt::Debug for LegacyImportCandidate {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::ApplicationConfiguration {
                source_version,
                document,
                migration_report,
                baseline,
            } => formatter
                .debug_struct("LegacyApplicationConfigurationImport")
                .field("source_version", source_version)
                .field("workspace_count", &document.workspaces.len())
                .field("migration_report", migration_report)
                .field("baseline_workspace_count", &baseline.workspaces.len())
                .finish_non_exhaustive(),
            Self::Workspace {
                source_version,
                migration_report,
                ..
            } => formatter
                .debug_struct("LegacyWorkspaceImport")
                .field("source_version", source_version)
                .field("migration_report", migration_report)
                .finish_non_exhaustive(),
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Type)]
pub struct LegacyImportPreview {
    pub token: LegacyImportToken,
    pub expires_in_seconds: u64,
    pub kind: LegacyImportKind,
    pub source_version: u16,
    pub workspace_count: usize,
    pub portable_material_count: usize,
    pub migration_report: MigrationReport,
    pub warnings: Vec<String>,
}

#[async_trait]
pub trait LegacyImportPreparePort: Send + Sync + fmt::Debug {
    async fn retain(
        &self,
        candidate: LegacyImportCandidate,
    ) -> AppResult<(LegacyImportToken, Duration)>;
    async fn take(&self, token: LegacyImportToken) -> AppResult<LegacyImportCandidate>;
    async fn discard(&self, token: LegacyImportToken) -> AppResult<()>;
}
