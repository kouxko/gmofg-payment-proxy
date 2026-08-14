use std::fmt;

use serde::Serialize;
use thiserror::Error;

use crate::PackageFilePath;

/// 安全 ZIP 读取与持久化文件恢复边界共用的稳定失败分类。
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum ProtocolArchiveErrorCode {
    /// 调用方提供的归档限制为零、互相矛盾或超过宿主硬上限。
    InvalidLimits,
    /// ZIP 输入本身超过压缩文件字节上限。
    ArchiveTooLarge,
    /// 输入不是结构完整的单卷 ZIP，或条目读取/校验失败。
    InvalidZip,
    /// ZIP 或持久化文件集合中没有任何条目。
    EmptyArchive,
    /// 中央目录声明的条目数或持久化文件行数超过上限。
    TooManyEntries,
    /// 条目原始名称不是 UTF-8。
    NonUtf8Path,
    /// 条目路径是绝对路径、包含父目录/反斜线/Windows 前缀或其他不安全成分。
    InvalidPath,
    /// 条目路径层级超过上限。
    PathTooDeep,
    /// 两个条目规范化后得到完全相同的路径。
    DuplicatePath,
    /// 两个条目只存在大小写差异，在大小写不敏感文件系统上会冲突。
    CaseConflict,
    /// 文件与目录层级冲突，例如文件 `scripts` 同时作为 `scripts/main.rhai` 的父路径。
    PathTypeConflict,
    /// ZIP 条目是符号链接。
    SymlinkForbidden,
    /// ZIP 条目既不是普通文件也不是目录。
    UnsupportedEntryType,
    /// ZIP 条目要求密码或声明为加密内容。
    EncryptedEntry,
    /// 条目使用 Host API v1 未启用的压缩算法。
    UnsupportedCompression,
    /// 单个解压文件或持久化文件内容超过上限。
    FileTooLarge,
    /// 所有文件累计实际字节超过上限。
    TotalTooLarge,
    /// 单个文件声明的解压/压缩比超过上限。
    CompressionRatioExceeded,
    /// 多个中央目录条目共享同一段压缩数据。
    OverlappingEntries,
    /// ZIP 根目录没有普通文件 `manifest.toml`。
    ManifestMissing,
}

impl ProtocolArchiveErrorCode {
    /// 返回无需解析中文 Display 文本的稳定机器码。
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
            Self::ManifestMissing => "MANIFEST_MISSING",
        }
    }
}

impl fmt::Display for ProtocolArchiveErrorCode {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.as_str())
    }
}

/// 协议包导入或恢复阶段可安全返回给上层的脱敏错误。
///
/// `entry_index` 定位 ZIP 中央目录序号或恢复输入行序号；`path` 仅在原始名称已经通过相对 UTF-8
/// 路径校验后出现。
/// 本类型不保存第三方错误、原始恶意路径、文件内容、绝对路径或临时目录。
#[derive(Clone, Debug, Eq, Error, PartialEq, Serialize)]
#[error("协议包 ZIP 校验失败（{code}）")]
pub struct ProtocolArchiveError {
    code: ProtocolArchiveErrorCode,
    entry_index: Option<usize>,
    path: Option<PackageFilePath>,
}

impl ProtocolArchiveError {
    /// 返回稳定错误分类。
    #[must_use]
    pub const fn code(&self) -> ProtocolArchiveErrorCode {
        self.code
    }

    /// 返回失败条目的中央目录序号；归档级错误返回 `None`。
    #[must_use]
    pub const fn entry_index(&self) -> Option<usize> {
        self.entry_index
    }

    /// 返回已经通过安全校验的包内相对路径；不安全原始名称永不回显。
    #[must_use]
    pub const fn path(&self) -> Option<&PackageFilePath> {
        self.path.as_ref()
    }

    pub(crate) const fn archive(code: ProtocolArchiveErrorCode) -> Self {
        Self {
            code,
            entry_index: None,
            path: None,
        }
    }

    pub(crate) const fn entry(code: ProtocolArchiveErrorCode, entry_index: usize) -> Self {
        Self {
            code,
            entry_index: Some(entry_index),
            path: None,
        }
    }

    pub(crate) fn safe_path(
        code: ProtocolArchiveErrorCode,
        entry_index: usize,
        path: PackageFilePath,
    ) -> Self {
        Self {
            code,
            entry_index: Some(entry_index),
            path: Some(path),
        }
    }
}
