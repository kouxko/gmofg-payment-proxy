//! Application backup export boundary and immutable snapshot.

use std::{collections::BTreeMap, fmt};

use async_trait::async_trait;
use serde::Serialize;
use specta::Type;

use crate::{AppResult, ApplicationBackupDocument, PortableArchivePath};

/// Complete immutable input to the filesystem export boundary.
#[derive(Clone, Eq, PartialEq)]
pub struct ApplicationBackupExportSnapshot {
    pub document: ApplicationBackupDocument,
    pub files: BTreeMap<PortableArchivePath, Vec<u8>>,
}

impl fmt::Debug for ApplicationBackupExportSnapshot {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("ApplicationBackupExportSnapshot")
            .field("document", &self.document)
            .field("payload_file_count", &self.files.len())
            .field(
                "payload_total_bytes",
                &self.files.values().map(Vec::len).sum::<usize>(),
            )
            .finish()
    }
}

/// Safe result of writing a caller-selected backup target.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Type)]
pub struct ApplicationBackupExportOutcome {
    pub bytes_written: u64,
    pub replaced_existing: bool,
}

#[async_trait]
pub trait ApplicationBackupExportPort: Send + Sync + fmt::Debug {
    async fn write(
        &self,
        snapshot: ApplicationBackupExportSnapshot,
    ) -> AppResult<ApplicationBackupExportOutcome>;
}
