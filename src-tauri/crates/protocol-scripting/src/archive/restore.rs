use std::collections::{BTreeMap, BTreeSet};

use crate::PackageFilePath;

use super::{
    ProtocolArchiveError, ProtocolArchiveErrorCode, ProtocolArchiveLimits, ProtocolPackageFiles,
};

/// 从持久化层读出的规范化文件行恢复协议包文件集合。
///
/// 调用方应传入数据库保存的 `(path, bytes)` 行，而不是可信的 [`PackageFilePath`]。本函数会把每一行
/// 重新当作不可信输入，复查跨平台相对路径、数量、单文件大小、累计大小、路径深度、精确重复、
/// 大小写冲突、文件父子冲突和根目录 Manifest。只有全部行通过后才构造 [`ProtocolPackageFiles`]；
/// 因此数据库缺行、重复行或被篡改时，调用方不会得到可编译的部分包。
///
/// ZIP 压缩字节数和压缩比在该边界没有对应输入，故不会使用 `max_archive_bytes` 或
/// `max_compression_ratio`；其余限制与 ZIP 导入边界保持一致。
pub fn restore_protocol_package_files(
    stored_files: Vec<(String, Vec<u8>)>,
    limits: &ProtocolArchiveLimits,
) -> Result<ProtocolPackageFiles, ProtocolArchiveError> {
    let mut paths = BTreeSet::new();
    let mut casefold_paths = BTreeSet::new();
    let mut files = BTreeMap::new();
    let mut total_bytes = 0_u64;

    for (index, (raw_path, bytes)) in stored_files.into_iter().enumerate() {
        if index >= limits.max_entries() {
            return Err(ProtocolArchiveError::archive(
                ProtocolArchiveErrorCode::TooManyEntries,
            ));
        }

        let path = PackageFilePath::new_for_field(raw_path, "$stored_file").map_err(|_| {
            ProtocolArchiveError::entry(ProtocolArchiveErrorCode::InvalidPath, index)
        })?;
        if path.as_str().split('/').count() > limits.max_path_depth() {
            return Err(ProtocolArchiveError::safe_path(
                ProtocolArchiveErrorCode::PathTooDeep,
                index,
                path,
            ));
        }
        if paths.contains(&path) {
            return Err(ProtocolArchiveError::safe_path(
                ProtocolArchiveErrorCode::DuplicatePath,
                index,
                path,
            ));
        }
        if !casefold_paths.insert(path.as_str().to_lowercase()) {
            return Err(ProtocolArchiveError::safe_path(
                ProtocolArchiveErrorCode::CaseConflict,
                index,
                path,
            ));
        }
        if has_file_parent_conflict(&path, &paths) {
            return Err(ProtocolArchiveError::safe_path(
                ProtocolArchiveErrorCode::PathTypeConflict,
                index,
                path,
            ));
        }

        let file_bytes = u64::try_from(bytes.len()).unwrap_or(u64::MAX);
        if file_bytes > limits.max_file_bytes() {
            return Err(ProtocolArchiveError::safe_path(
                ProtocolArchiveErrorCode::FileTooLarge,
                index,
                path,
            ));
        }
        // `max_entries * max_file_bytes` 受宿主硬上限约束，最大约 4 GiB，远小于 u64::MAX。
        total_bytes += file_bytes;
        if total_bytes > limits.max_total_bytes() {
            return Err(ProtocolArchiveError::safe_path(
                ProtocolArchiveErrorCode::TotalTooLarge,
                index,
                path,
            ));
        }

        paths.insert(path.clone());
        files.insert(path, bytes);
    }

    if files.is_empty() {
        return Err(ProtocolArchiveError::archive(
            ProtocolArchiveErrorCode::EmptyArchive,
        ));
    }
    if !files.keys().any(|path| path.as_str() == "manifest.toml") {
        return Err(ProtocolArchiveError::archive(
            ProtocolArchiveErrorCode::ManifestMissing,
        ));
    }
    Ok(ProtocolPackageFiles::new(files, total_bytes))
}

fn has_file_parent_conflict(
    path: &PackageFilePath,
    existing_paths: &BTreeSet<PackageFilePath>,
) -> bool {
    let path_prefix = format!("{}/", path.as_str());
    existing_paths.iter().any(|existing| {
        let existing_prefix = format!("{}/", existing.as_str());
        path.as_str().starts_with(&existing_prefix) || existing.as_str().starts_with(&path_prefix)
    })
}
