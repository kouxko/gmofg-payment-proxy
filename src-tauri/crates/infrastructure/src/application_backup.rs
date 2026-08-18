//! Strict, bounded in-memory reader for application backup ZIP v1.

use std::{
    collections::{BTreeMap, BTreeSet},
    fmt,
    io::{Cursor, Read, Seek, SeekFrom},
};

use intercept_proxy_application::{
    ApplicationBackupDocument, PortableArchivePath, parse_application_backup_document,
};
use zip::{CompressionMethod, ZipArchive};

mod error;

pub use error::{ApplicationBackupArchiveError, ApplicationBackupArchiveErrorCode};

pub const DEFAULT_MAX_APPLICATION_BACKUP_ARCHIVE_BYTES: u64 = 256 * 1024 * 1024;
pub const DEFAULT_MAX_APPLICATION_BACKUP_ENTRIES: usize = 8_192;
pub const DEFAULT_MAX_APPLICATION_BACKUP_FILE_BYTES: u64 = 32 * 1024 * 1024;
pub const DEFAULT_MAX_APPLICATION_BACKUP_TOTAL_BYTES: u64 = 512 * 1024 * 1024;
pub const DEFAULT_MAX_APPLICATION_BACKUP_COMPRESSION_RATIO: u64 = 1_000;
pub const DEFAULT_MAX_APPLICATION_BACKUP_PATH_DEPTH: usize = 40;
pub const MAX_APPLICATION_BACKUP_ARCHIVE_BYTES: u64 = 256 * 1024 * 1024;
pub const MAX_APPLICATION_BACKUP_ENTRIES: usize = 8_192;
pub const MAX_APPLICATION_BACKUP_FILE_BYTES: u64 = 32 * 1024 * 1024;
pub const MAX_APPLICATION_BACKUP_TOTAL_BYTES: u64 = 512 * 1024 * 1024;
pub const MAX_APPLICATION_BACKUP_COMPRESSION_RATIO: u64 = 1_000;
pub const MAX_APPLICATION_BACKUP_ARCHIVE_PATH_DEPTH: usize = 40;

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ApplicationBackupArchiveLimits {
    archive_bytes: u64,
    entries: usize,
    file_bytes: u64,
    total_bytes: u64,
    compression_ratio: u64,
    path_depth: usize,
}

impl ApplicationBackupArchiveLimits {
    pub fn new(
        max_archive_bytes: u64,
        max_entries: usize,
        max_file_bytes: u64,
        max_total_bytes: u64,
        max_compression_ratio: u64,
        max_path_depth: usize,
    ) -> Result<Self, ApplicationBackupArchiveError> {
        if !(1..=MAX_APPLICATION_BACKUP_ARCHIVE_BYTES).contains(&max_archive_bytes)
            || !(1..=MAX_APPLICATION_BACKUP_ENTRIES).contains(&max_entries)
            || !(1..=MAX_APPLICATION_BACKUP_FILE_BYTES).contains(&max_file_bytes)
            || !(max_file_bytes..=MAX_APPLICATION_BACKUP_TOTAL_BYTES).contains(&max_total_bytes)
            || !(1..=MAX_APPLICATION_BACKUP_COMPRESSION_RATIO).contains(&max_compression_ratio)
            || !(1..=MAX_APPLICATION_BACKUP_ARCHIVE_PATH_DEPTH).contains(&max_path_depth)
        {
            return Err(ApplicationBackupArchiveError::archive(
                ApplicationBackupArchiveErrorCode::InvalidLimits,
            ));
        }
        Ok(Self {
            archive_bytes: max_archive_bytes,
            entries: max_entries,
            file_bytes: max_file_bytes,
            total_bytes: max_total_bytes,
            compression_ratio: max_compression_ratio,
            path_depth: max_path_depth,
        })
    }
}

impl Default for ApplicationBackupArchiveLimits {
    fn default() -> Self {
        Self {
            archive_bytes: DEFAULT_MAX_APPLICATION_BACKUP_ARCHIVE_BYTES,
            entries: DEFAULT_MAX_APPLICATION_BACKUP_ENTRIES,
            file_bytes: DEFAULT_MAX_APPLICATION_BACKUP_FILE_BYTES,
            total_bytes: DEFAULT_MAX_APPLICATION_BACKUP_TOTAL_BYTES,
            compression_ratio: DEFAULT_MAX_APPLICATION_BACKUP_COMPRESSION_RATIO,
            path_depth: DEFAULT_MAX_APPLICATION_BACKUP_PATH_DEPTH,
        }
    }
}

#[derive(Clone, PartialEq)]
pub struct ApplicationBackupArchive {
    pub document: ApplicationBackupDocument,
    /// Referenced payload files only; `application.json` is represented by `document`.
    pub files: BTreeMap<PortableArchivePath, Vec<u8>>,
}

impl fmt::Debug for ApplicationBackupArchive {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("ApplicationBackupArchive")
            .field("document", &self.document)
            .field("payload_file_count", &self.files.len())
            .field(
                "payload_total_bytes",
                &self.files.values().map(Vec::len).sum::<usize>(),
            )
            .finish()
    }
}

impl ApplicationBackupArchive {
    pub fn read(bytes: &[u8]) -> Result<Self, ApplicationBackupArchiveError> {
        Self::read_with_limits(bytes, &ApplicationBackupArchiveLimits::default())
    }

    pub fn read_with_limits(
        bytes: &[u8],
        limits: &ApplicationBackupArchiveLimits,
    ) -> Result<Self, ApplicationBackupArchiveError> {
        read_archive(Cursor::new(bytes), limits)
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum EntryKind {
    File,
    Directory,
}

#[derive(Debug)]
struct ValidatedEntry {
    path: PortableArchivePath,
    kind: EntryKind,
    declared_size: u64,
}

fn read_archive<R: Read + Seek>(
    mut reader: R,
    limits: &ApplicationBackupArchiveLimits,
) -> Result<ApplicationBackupArchive, ApplicationBackupArchiveError> {
    let archive_bytes = reader.seek(SeekFrom::End(0)).map_err(|_| invalid_zip())?;
    if archive_bytes == 0 {
        return Err(ApplicationBackupArchiveError::archive(
            ApplicationBackupArchiveErrorCode::EmptyArchive,
        ));
    }
    if archive_bytes > limits.archive_bytes {
        return Err(ApplicationBackupArchiveError::archive(
            ApplicationBackupArchiveErrorCode::ArchiveTooLarge,
        ));
    }
    let declared_entries = read_eocd_entry_count(&mut reader, archive_bytes)?;
    if declared_entries > limits.entries {
        return Err(ApplicationBackupArchiveError::archive(
            ApplicationBackupArchiveErrorCode::TooManyEntries,
        ));
    }
    reader.seek(SeekFrom::Start(0)).map_err(|_| invalid_zip())?;
    let mut archive = ZipArchive::new(reader).map_err(|_| invalid_zip())?;
    if archive.is_empty() {
        return Err(ApplicationBackupArchiveError::archive(
            ApplicationBackupArchiveErrorCode::EmptyArchive,
        ));
    }
    if archive.len() != declared_entries {
        return Err(ApplicationBackupArchiveError::archive(
            ApplicationBackupArchiveErrorCode::DuplicatePath,
        ));
    }
    if archive.has_overlapping_files().map_err(|_| invalid_zip())? {
        return Err(ApplicationBackupArchiveError::archive(
            ApplicationBackupArchiveErrorCode::OverlappingEntries,
        ));
    }

    let mut nodes = BTreeMap::new();
    let mut casefold = BTreeSet::new();
    let mut files = BTreeMap::new();
    let mut application = None;
    let mut total = 0_u64;
    for index in 0..archive.len() {
        let entry = validate_entry(&mut archive, index, limits)?;
        validate_unique(&entry, index, &nodes, &mut casefold)?;
        nodes.insert(entry.path.clone(), entry.kind);
        validate_layout(&entry, index)?;
        if entry.kind == EntryKind::Directory {
            continue;
        }
        total = total.checked_add(entry.declared_size).ok_or_else(|| {
            ApplicationBackupArchiveError::safe_path(
                ApplicationBackupArchiveErrorCode::TotalTooLarge,
                index,
                entry.path.clone(),
            )
        })?;
        if total > limits.total_bytes {
            return Err(ApplicationBackupArchiveError::safe_path(
                ApplicationBackupArchiveErrorCode::TotalTooLarge,
                index,
                entry.path,
            ));
        }
        let content = read_entry(&mut archive, index, &entry, limits)?;
        if entry.path.as_str() == "application.json" {
            application = Some(content);
        } else {
            files.insert(entry.path, content);
        }
    }
    let application = application.ok_or_else(|| {
        ApplicationBackupArchiveError::archive(
            ApplicationBackupArchiveErrorCode::ApplicationDocumentMissing,
        )
    })?;
    let document = parse_application_backup_document(&application).map_err(|_| {
        ApplicationBackupArchiveError::archive(
            ApplicationBackupArchiveErrorCode::ApplicationDocumentInvalid,
        )
    })?;
    let present = files.keys().cloned().collect::<BTreeSet<_>>();
    let referenced = document.referenced_paths();
    if let Some(path) = referenced.difference(&present).next() {
        return Err(ApplicationBackupArchiveError {
            code: ApplicationBackupArchiveErrorCode::ReferencedFileMissing,
            entry_index: None,
            path: Some(path.clone()),
        });
    }
    if let Some(path) = present.difference(&referenced).next() {
        return Err(ApplicationBackupArchiveError {
            code: ApplicationBackupArchiveErrorCode::UnreferencedFile,
            entry_index: None,
            path: Some(path.clone()),
        });
    }
    Ok(ApplicationBackupArchive { document, files })
}

fn validate_layout(
    entry: &ValidatedEntry,
    index: usize,
) -> Result<(), ApplicationBackupArchiveError> {
    let path = entry.path.as_str();
    let allowed = path == "application.json"
        || path == "protocol-packages"
        || path.starts_with("protocol-packages/")
        || path == "portable-materials"
        || path.starts_with("portable-materials/");
    if !allowed || (path == "application.json" && entry.kind != EntryKind::File) {
        return Err(ApplicationBackupArchiveError::safe_path(
            ApplicationBackupArchiveErrorCode::UnknownTopLevel,
            index,
            entry.path.clone(),
        ));
    }
    Ok(())
}

fn validate_entry<R: Read + Seek>(
    archive: &mut ZipArchive<R>,
    index: usize,
    limits: &ApplicationBackupArchiveLimits,
) -> Result<ValidatedEntry, ApplicationBackupArchiveError> {
    let file = archive.by_index_raw(index).map_err(|_| {
        ApplicationBackupArchiveError::entry(ApplicationBackupArchiveErrorCode::InvalidZip, index)
    })?;
    let raw = std::str::from_utf8(file.name_raw()).map_err(|_| {
        ApplicationBackupArchiveError::entry(ApplicationBackupArchiveErrorCode::NonUtf8Path, index)
    })?;
    if file.is_symlink() {
        return Err(ApplicationBackupArchiveError::entry(
            ApplicationBackupArchiveErrorCode::SymlinkForbidden,
            index,
        ));
    }
    if file.unix_mode().is_some_and(|mode| {
        let kind = mode & 0o170_000;
        kind != 0 && kind != 0o100_000 && kind != 0o040_000
    }) {
        return Err(ApplicationBackupArchiveError::entry(
            ApplicationBackupArchiveErrorCode::UnsupportedEntryType,
            index,
        ));
    }
    if file.encrypted() {
        return Err(ApplicationBackupArchiveError::entry(
            ApplicationBackupArchiveErrorCode::EncryptedEntry,
            index,
        ));
    }
    if !matches!(
        file.compression(),
        CompressionMethod::Stored | CompressionMethod::Deflated
    ) {
        return Err(ApplicationBackupArchiveError::entry(
            ApplicationBackupArchiveErrorCode::UnsupportedCompression,
            index,
        ));
    }
    let (kind, text) = if file.is_dir() {
        (EntryKind::Directory, raw.strip_suffix('/').unwrap_or(raw))
    } else {
        (EntryKind::File, raw)
    };
    let path = PortableArchivePath::new(text.to_owned()).map_err(|_| {
        ApplicationBackupArchiveError::entry(ApplicationBackupArchiveErrorCode::InvalidPath, index)
    })?;
    if path.as_str().split('/').count() > limits.path_depth {
        return Err(ApplicationBackupArchiveError::safe_path(
            ApplicationBackupArchiveErrorCode::PathTooDeep,
            index,
            path,
        ));
    }
    let declared_size = file.size();
    let compressed_size = file.compressed_size();
    if (kind == EntryKind::Directory && declared_size != 0) || declared_size > limits.file_bytes {
        let code = if declared_size > limits.file_bytes {
            ApplicationBackupArchiveErrorCode::FileTooLarge
        } else {
            ApplicationBackupArchiveErrorCode::InvalidZip
        };
        return Err(ApplicationBackupArchiveError::safe_path(code, index, path));
    }
    if declared_size > 0
        && (compressed_size == 0
            || declared_size > compressed_size.saturating_mul(limits.compression_ratio))
    {
        return Err(ApplicationBackupArchiveError::safe_path(
            ApplicationBackupArchiveErrorCode::CompressionRatioExceeded,
            index,
            path,
        ));
    }
    Ok(ValidatedEntry {
        path,
        kind,
        declared_size,
    })
}

fn validate_unique(
    entry: &ValidatedEntry,
    index: usize,
    nodes: &BTreeMap<PortableArchivePath, EntryKind>,
    casefold: &mut BTreeSet<String>,
) -> Result<(), ApplicationBackupArchiveError> {
    if nodes.contains_key(&entry.path) {
        return Err(ApplicationBackupArchiveError::safe_path(
            ApplicationBackupArchiveErrorCode::DuplicatePath,
            index,
            entry.path.clone(),
        ));
    }
    if !casefold.insert(entry.path.as_str().to_lowercase()) {
        return Err(ApplicationBackupArchiveError::safe_path(
            ApplicationBackupArchiveErrorCode::CaseConflict,
            index,
            entry.path.clone(),
        ));
    }
    let prefix = format!("{}/", entry.path.as_str());
    if nodes.iter().any(|(existing, kind)| {
        let existing_prefix = format!("{}/", existing.as_str());
        (entry.path.as_str().starts_with(&existing_prefix) && *kind == EntryKind::File)
            || (existing.as_str().starts_with(&prefix) && entry.kind == EntryKind::File)
    }) {
        return Err(ApplicationBackupArchiveError::safe_path(
            ApplicationBackupArchiveErrorCode::PathTypeConflict,
            index,
            entry.path.clone(),
        ));
    }
    Ok(())
}

fn read_entry<R: Read + Seek>(
    archive: &mut ZipArchive<R>,
    index: usize,
    entry: &ValidatedEntry,
    limits: &ApplicationBackupArchiveLimits,
) -> Result<Vec<u8>, ApplicationBackupArchiveError> {
    let file = archive.by_index(index).map_err(|_| invalid_zip())?;
    let mut bytes = Vec::with_capacity(usize::try_from(entry.declared_size).unwrap_or(0));
    file.take(limits.file_bytes + 1)
        .read_to_end(&mut bytes)
        .map_err(|_| invalid_zip())?;
    if bytes.len() as u64 > limits.file_bytes {
        return Err(ApplicationBackupArchiveError::safe_path(
            ApplicationBackupArchiveErrorCode::FileTooLarge,
            index,
            entry.path.clone(),
        ));
    }
    if bytes.len() as u64 != entry.declared_size {
        return Err(ApplicationBackupArchiveError::safe_path(
            ApplicationBackupArchiveErrorCode::InvalidZip,
            index,
            entry.path.clone(),
        ));
    }
    Ok(bytes)
}

fn read_eocd_entry_count<R: Read + Seek>(
    reader: &mut R,
    archive_bytes: u64,
) -> Result<usize, ApplicationBackupArchiveError> {
    const MIN: usize = 22;
    let tail_len = usize::try_from(archive_bytes.min((MIN + u16::MAX as usize) as u64))
        .map_err(|_| invalid_zip())?;
    reader
        .seek(SeekFrom::End(
            -i64::try_from(tail_len).map_err(|_| invalid_zip())?,
        ))
        .map_err(|_| invalid_zip())?;
    let mut tail = vec![0; tail_len];
    reader.read_exact(&mut tail).map_err(|_| invalid_zip())?;
    let offset = tail
        .windows(4)
        .enumerate()
        .rev()
        .find_map(|(offset, window)| {
            if window != [0x50, 0x4b, 0x05, 0x06] || tail.len() < offset + MIN {
                return None;
            }
            (offset + MIN + usize::from(read_u16(&tail, offset + 20)) == tail.len())
                .then_some(offset)
        })
        .ok_or_else(invalid_zip)?;
    let disk = read_u16(&tail, offset + 4);
    let central_disk = read_u16(&tail, offset + 6);
    let on_disk = read_u16(&tail, offset + 8);
    let total = read_u16(&tail, offset + 10);
    let central_end =
        u64::from(read_u32(&tail, offset + 16)) + u64::from(read_u32(&tail, offset + 12));
    let eocd = archive_bytes - (tail.len() - offset) as u64;
    if disk != 0 || central_disk != 0 || on_disk != total || total == u16::MAX || central_end > eocd
    {
        return Err(invalid_zip());
    }
    Ok(usize::from(total))
}

fn read_u16(bytes: &[u8], offset: usize) -> u16 {
    u16::from_le_bytes([bytes[offset], bytes[offset + 1]])
}
fn read_u32(bytes: &[u8], offset: usize) -> u32 {
    u32::from_le_bytes([
        bytes[offset],
        bytes[offset + 1],
        bytes[offset + 2],
        bytes[offset + 3],
    ])
}
fn invalid_zip() -> ApplicationBackupArchiveError {
    ApplicationBackupArchiveError::archive(ApplicationBackupArchiveErrorCode::InvalidZip)
}
