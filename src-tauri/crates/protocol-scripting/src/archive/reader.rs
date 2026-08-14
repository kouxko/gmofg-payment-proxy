use std::{
    collections::{BTreeMap, BTreeSet},
    io::{Read, Seek, SeekFrom},
};

use zip::{CompressionMethod, ZipArchive};

use crate::PackageFilePath;

use super::{
    ProtocolArchiveError, ProtocolArchiveErrorCode, ProtocolArchiveLimits, ProtocolPackageFiles,
};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum EntryKind {
    File,
    Directory,
}

#[derive(Debug)]
struct ValidatedEntry {
    path: PackageFilePath,
    kind: EntryKind,
    declared_size: u64,
}

/// 流式读取普通协议包 ZIP，并仅在全部条目通过后返回确定性内存文件集合。
///
/// `reader` 必须表示从偏移零开始的完整单卷 ZIP，并实现 `Seek` 以读取中央目录。函数不会访问文件系统；
/// 调用失败时，已解压的局部 `Vec` 和 Map 会随栈释放，外层无法观察到部分包状态。
pub fn read_protocol_package_zip<R: Read + Seek>(
    mut reader: R,
    limits: &ProtocolArchiveLimits,
) -> Result<ProtocolPackageFiles, ProtocolArchiveError> {
    let archive_bytes = reader
        .seek(SeekFrom::End(0))
        .map_err(|_| ProtocolArchiveError::archive(ProtocolArchiveErrorCode::InvalidZip))?;
    if archive_bytes == 0 {
        return Err(ProtocolArchiveError::archive(
            ProtocolArchiveErrorCode::EmptyArchive,
        ));
    }
    if archive_bytes > limits.max_archive_bytes() {
        return Err(ProtocolArchiveError::archive(
            ProtocolArchiveErrorCode::ArchiveTooLarge,
        ));
    }
    let declared_entries = read_eocd_entry_count(&mut reader, archive_bytes)?;
    if declared_entries > limits.max_entries() {
        return Err(ProtocolArchiveError::archive(
            ProtocolArchiveErrorCode::TooManyEntries,
        ));
    }
    reader
        .seek(SeekFrom::Start(0))
        .map_err(|_| ProtocolArchiveError::archive(ProtocolArchiveErrorCode::InvalidZip))?;

    let mut archive = ZipArchive::new(reader)
        .map_err(|_| ProtocolArchiveError::archive(ProtocolArchiveErrorCode::InvalidZip))?;
    if archive.is_empty() {
        return Err(ProtocolArchiveError::archive(
            ProtocolArchiveErrorCode::EmptyArchive,
        ));
    }
    if archive.len() != declared_entries {
        // zip crate 按原始文件名建立 IndexMap；同名中央目录项会覆盖旧值。EOCD 计数更大时，
        // 这正是我们必须显式拒绝的重复规范路径，而不是接受“最后一个覆盖前一个”。
        return Err(ProtocolArchiveError::archive(
            ProtocolArchiveErrorCode::DuplicatePath,
        ));
    }
    if archive
        .has_overlapping_files()
        .map_err(|_| ProtocolArchiveError::archive(ProtocolArchiveErrorCode::InvalidZip))?
    {
        return Err(ProtocolArchiveError::archive(
            ProtocolArchiveErrorCode::OverlappingEntries,
        ));
    }

    let mut nodes = BTreeMap::new();
    let mut casefold_paths = BTreeSet::new();
    let mut files = BTreeMap::new();
    let mut total_bytes = 0_u64;

    for index in 0..archive.len() {
        let entry = validate_entry(&mut archive, index, limits)?;
        validate_path_uniqueness(&entry, index, &nodes, &mut casefold_paths)?;
        nodes.insert(entry.path.clone(), entry.kind);
        if entry.kind == EntryKind::Directory {
            continue;
        }

        // max_entries * max_file_bytes 受 limits 硬上限约束，远小于 u64::MAX，不存在算术溢出。
        total_bytes += entry.declared_size;
        if total_bytes > limits.max_total_bytes() {
            return Err(ProtocolArchiveError::safe_path(
                ProtocolArchiveErrorCode::TotalTooLarge,
                index,
                entry.path,
            ));
        }

        let bytes = read_entry_bytes(&mut archive, index, &entry, limits)?;
        files.insert(entry.path, bytes);
    }

    if !files.keys().any(|path| path.as_str() == "manifest.toml") {
        return Err(ProtocolArchiveError::archive(
            ProtocolArchiveErrorCode::ManifestMissing,
        ));
    }
    Ok(ProtocolPackageFiles::new(files, total_bytes))
}

fn read_eocd_entry_count<R: Read + Seek>(
    reader: &mut R,
    archive_bytes: u64,
) -> Result<usize, ProtocolArchiveError> {
    const EOCD_MIN_BYTES: usize = 22;
    const MAX_ZIP_COMMENT_BYTES: usize = u16::MAX as usize;
    const EOCD_SIGNATURE: [u8; 4] = [0x50, 0x4b, 0x05, 0x06];

    let tail_len =
        usize::try_from(archive_bytes.min((EOCD_MIN_BYTES + MAX_ZIP_COMMENT_BYTES) as u64))
            .map_err(|_| ProtocolArchiveError::archive(ProtocolArchiveErrorCode::InvalidZip))?;
    let tail_seek = i64::try_from(tail_len)
        .map_err(|_| ProtocolArchiveError::archive(ProtocolArchiveErrorCode::InvalidZip))?;
    reader
        .seek(SeekFrom::End(-tail_seek))
        .map_err(|_| ProtocolArchiveError::archive(ProtocolArchiveErrorCode::InvalidZip))?;
    let mut tail = vec![0_u8; tail_len];
    reader
        .read_exact(&mut tail)
        .map_err(|_| ProtocolArchiveError::archive(ProtocolArchiveErrorCode::InvalidZip))?;
    // ZIP comment 可以包含与 EOCD 相同的四字节序列，因此不能简单选择最后一次出现。只有其
    // comment_length 恰好指向输入结尾的候选才可能是真实 EOCD。
    let offset = tail
        .windows(EOCD_SIGNATURE.len())
        .enumerate()
        .rev()
        .find_map(|(offset, window)| {
            if window != EOCD_SIGNATURE || tail.len() < offset + EOCD_MIN_BYTES {
                return None;
            }
            let comment_len = read_u16(&tail, offset + 20) as usize;
            (offset + EOCD_MIN_BYTES + comment_len == tail.len()).then_some(offset)
        })
        .ok_or_else(|| ProtocolArchiveError::archive(ProtocolArchiveErrorCode::InvalidZip))?;

    let disk = read_u16(&tail, offset + 4);
    let central_disk = read_u16(&tail, offset + 6);
    let entries_on_disk = read_u16(&tail, offset + 8);
    let total_entries = read_u16(&tail, offset + 10);
    let central_size = u64::from(read_u32(&tail, offset + 12));
    let central_offset = u64::from(read_u32(&tail, offset + 16));
    let eocd_absolute = archive_bytes - (tail.len() - offset) as u64;
    // 两项都来自 u32 EOCD 字段，提升到 u64 后相加不可能溢出。
    let central_end = central_offset + central_size;
    // 协议包限制远低于 ZIP64 阈值，因此拒绝 ZIP64 sentinel 和多卷 ZIP，保持输入模型单一。
    if disk != 0
        || central_disk != 0
        || entries_on_disk != total_entries
        || total_entries == u16::MAX
        || central_end > eocd_absolute
    {
        return Err(ProtocolArchiveError::archive(
            ProtocolArchiveErrorCode::InvalidZip,
        ));
    }
    Ok(usize::from(total_entries))
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

fn validate_entry<R: Read + Seek>(
    archive: &mut ZipArchive<R>,
    index: usize,
    limits: &ProtocolArchiveLimits,
) -> Result<ValidatedEntry, ProtocolArchiveError> {
    let file = archive
        .by_index_raw(index)
        .map_err(|_| ProtocolArchiveError::entry(ProtocolArchiveErrorCode::InvalidZip, index))?;
    let raw_name = std::str::from_utf8(file.name_raw())
        .map_err(|_| ProtocolArchiveError::entry(ProtocolArchiveErrorCode::NonUtf8Path, index))?;
    if file.is_symlink() {
        return Err(ProtocolArchiveError::entry(
            ProtocolArchiveErrorCode::SymlinkForbidden,
            index,
        ));
    }
    // `zip::ZipFile::is_file` 仅排除目录和符号链接；Unix FIFO、设备和 socket 也会被它视为文件。
    // 显式检查 st_mode 文件类型位，避免后续平台把特殊节点当作普通脚本内容处理。
    if file.unix_mode().is_some_and(|mode| {
        let file_type = mode & 0o170_000;
        file_type != 0 && file_type != 0o100_000 && file_type != 0o040_000
    }) {
        return Err(ProtocolArchiveError::entry(
            ProtocolArchiveErrorCode::UnsupportedEntryType,
            index,
        ));
    }
    if file.encrypted() {
        return Err(ProtocolArchiveError::entry(
            ProtocolArchiveErrorCode::EncryptedEntry,
            index,
        ));
    }
    if !matches!(
        file.compression(),
        CompressionMethod::Stored | CompressionMethod::Deflated
    ) {
        return Err(ProtocolArchiveError::entry(
            ProtocolArchiveErrorCode::UnsupportedCompression,
            index,
        ));
    }

    let (kind, path_text) = if file.is_dir() {
        (
            EntryKind::Directory,
            raw_name.strip_suffix('/').unwrap_or(raw_name),
        )
    } else {
        // 符号链接和 Unix 特殊类型已在上方拒绝；zip crate 的 is_file 正是剩余情况。
        debug_assert!(file.is_file());
        (EntryKind::File, raw_name)
    };
    let path = PackageFilePath::new_for_field(path_text.to_owned(), "$archive_entry")
        .map_err(|_| ProtocolArchiveError::entry(ProtocolArchiveErrorCode::InvalidPath, index))?;
    if path.as_str().split('/').count() > limits.max_path_depth() {
        return Err(ProtocolArchiveError::safe_path(
            ProtocolArchiveErrorCode::PathTooDeep,
            index,
            path,
        ));
    }

    let declared_size = file.size();
    let compressed_size = file.compressed_size();
    if kind == EntryKind::Directory && declared_size != 0 {
        return Err(ProtocolArchiveError::safe_path(
            ProtocolArchiveErrorCode::InvalidZip,
            index,
            path,
        ));
    }
    if declared_size > limits.max_file_bytes() {
        return Err(ProtocolArchiveError::safe_path(
            ProtocolArchiveErrorCode::FileTooLarge,
            index,
            path,
        ));
    }
    if declared_size > 0
        && (compressed_size == 0
            || declared_size > compressed_size.saturating_mul(limits.max_compression_ratio()))
    {
        return Err(ProtocolArchiveError::safe_path(
            ProtocolArchiveErrorCode::CompressionRatioExceeded,
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

fn validate_path_uniqueness(
    entry: &ValidatedEntry,
    index: usize,
    nodes: &BTreeMap<PackageFilePath, EntryKind>,
    casefold_paths: &mut BTreeSet<String>,
) -> Result<(), ProtocolArchiveError> {
    // 完全相同的中央目录原始名称会被 zip crate 的 IndexMap 合并，已由 EOCD 条目数不一致拒绝。
    if !casefold_paths.insert(entry.path.as_str().to_lowercase()) {
        return Err(ProtocolArchiveError::safe_path(
            ProtocolArchiveErrorCode::CaseConflict,
            index,
            entry.path.clone(),
        ));
    }

    let path_prefix = format!("{}/", entry.path.as_str());
    let hierarchy_conflict = nodes.iter().any(|(existing, kind)| {
        let existing_prefix = format!("{}/", existing.as_str());
        (entry.path.as_str().starts_with(&existing_prefix) && *kind == EntryKind::File)
            || (existing.as_str().starts_with(&path_prefix) && entry.kind == EntryKind::File)
    });
    if hierarchy_conflict {
        return Err(ProtocolArchiveError::safe_path(
            ProtocolArchiveErrorCode::PathTypeConflict,
            index,
            entry.path.clone(),
        ));
    }
    Ok(())
}

fn read_entry_bytes<R: Read + Seek>(
    archive: &mut ZipArchive<R>,
    index: usize,
    entry: &ValidatedEntry,
    limits: &ProtocolArchiveLimits,
) -> Result<Vec<u8>, ProtocolArchiveError> {
    let file = archive
        .by_index(index)
        .map_err(|_| invalid_zip_entry(index, &entry.path))?;
    let capacity = usize::try_from(entry.declared_size).unwrap_or(0);
    let mut bytes = Vec::with_capacity(capacity);
    file.take(limits.max_file_bytes() + 1)
        .read_to_end(&mut bytes)
        .map_err(|_| invalid_zip_entry(index, &entry.path))?;
    if bytes.len() as u64 > limits.max_file_bytes() {
        return Err(ProtocolArchiveError::safe_path(
            ProtocolArchiveErrorCode::FileTooLarge,
            index,
            entry.path.clone(),
        ));
    }
    if bytes.len() as u64 != entry.declared_size {
        return Err(ProtocolArchiveError::safe_path(
            ProtocolArchiveErrorCode::InvalidZip,
            index,
            entry.path.clone(),
        ));
    }
    Ok(bytes)
}

fn invalid_zip_entry(index: usize, path: &PackageFilePath) -> ProtocolArchiveError {
    ProtocolArchiveError::safe_path(ProtocolArchiveErrorCode::InvalidZip, index, path.clone())
}
