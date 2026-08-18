use std::fmt;

use intercept_proxy_application::PortableArchivePath;
use serde::Serialize;
use thiserror::Error;

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum ApplicationBackupArchiveErrorCode {
    InvalidLimits,
    ArchiveTooLarge,
    InvalidZip,
    EmptyArchive,
    TooManyEntries,
    NonUtf8Path,
    InvalidPath,
    PathTooDeep,
    DuplicatePath,
    CaseConflict,
    PathTypeConflict,
    SymlinkForbidden,
    UnsupportedEntryType,
    EncryptedEntry,
    UnsupportedCompression,
    FileTooLarge,
    TotalTooLarge,
    CompressionRatioExceeded,
    OverlappingEntries,
    UnknownTopLevel,
    ApplicationDocumentMissing,
    ApplicationDocumentInvalid,
    ReferencedFileMissing,
    UnreferencedFile,
}

impl ApplicationBackupArchiveErrorCode {
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::InvalidLimits => "INVALID_LIMITS",
            Self::ArchiveTooLarge => "ARCHIVE_TOO_LARGE",
            Self::InvalidZip => "INVALID_ZIP",
            Self::EmptyArchive => "EMPTY_ARCHIVE",
            Self::TooManyEntries => "TOO_MANY_ENTRIES",
            Self::NonUtf8Path => "NON_UTF8_PATH",
            Self::InvalidPath => "INVALID_PATH",
            Self::PathTooDeep => "PATH_TOO_DEEP",
            Self::DuplicatePath => "DUPLICATE_PATH",
            Self::CaseConflict => "CASE_CONFLICT",
            Self::PathTypeConflict => "PATH_TYPE_CONFLICT",
            Self::SymlinkForbidden => "SYMLINK_FORBIDDEN",
            Self::UnsupportedEntryType => "UNSUPPORTED_ENTRY_TYPE",
            Self::EncryptedEntry => "ENCRYPTED_ENTRY",
            Self::UnsupportedCompression => "UNSUPPORTED_COMPRESSION",
            Self::FileTooLarge => "FILE_TOO_LARGE",
            Self::TotalTooLarge => "TOTAL_TOO_LARGE",
            Self::CompressionRatioExceeded => "COMPRESSION_RATIO_EXCEEDED",
            Self::OverlappingEntries => "OVERLAPPING_ENTRIES",
            Self::UnknownTopLevel => "UNKNOWN_TOP_LEVEL",
            Self::ApplicationDocumentMissing => "APPLICATION_DOCUMENT_MISSING",
            Self::ApplicationDocumentInvalid => "APPLICATION_DOCUMENT_INVALID",
            Self::ReferencedFileMissing => "REFERENCED_FILE_MISSING",
            Self::UnreferencedFile => "UNREFERENCED_FILE",
        }
    }
}

impl fmt::Display for ApplicationBackupArchiveErrorCode {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.as_str())
    }
}

#[derive(Clone, Debug, Eq, Error, PartialEq, Serialize)]
#[error("应用备份 ZIP 校验失败（{code}）")]
pub struct ApplicationBackupArchiveError {
    pub(super) code: ApplicationBackupArchiveErrorCode,
    pub(super) entry_index: Option<usize>,
    pub(super) path: Option<PortableArchivePath>,
}

impl ApplicationBackupArchiveError {
    #[must_use]
    pub const fn code(&self) -> ApplicationBackupArchiveErrorCode {
        self.code
    }

    #[must_use]
    pub const fn entry_index(&self) -> Option<usize> {
        self.entry_index
    }

    #[must_use]
    pub const fn path(&self) -> Option<&PortableArchivePath> {
        self.path.as_ref()
    }

    pub(super) const fn archive(code: ApplicationBackupArchiveErrorCode) -> Self {
        Self {
            code,
            entry_index: None,
            path: None,
        }
    }

    pub(super) const fn entry(code: ApplicationBackupArchiveErrorCode, entry_index: usize) -> Self {
        Self {
            code,
            entry_index: Some(entry_index),
            path: None,
        }
    }

    pub(super) fn safe_path(
        code: ApplicationBackupArchiveErrorCode,
        entry_index: usize,
        path: PortableArchivePath,
    ) -> Self {
        Self {
            code,
            entry_index: Some(entry_index),
            path: Some(path),
        }
    }
}
