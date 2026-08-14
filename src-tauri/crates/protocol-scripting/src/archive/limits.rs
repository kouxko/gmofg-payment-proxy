use serde::{Deserialize, Serialize};

use super::{ProtocolArchiveError, ProtocolArchiveErrorCode};

/// 默认允许的 ZIP 压缩输入字节数：8 MiB。
pub const DEFAULT_MAX_ARCHIVE_BYTES: u64 = 8 * 1024 * 1024;
/// 默认允许的中央目录条目数。
pub const DEFAULT_MAX_ARCHIVE_ENTRIES: usize = 64;
/// 默认允许的单个解压文件字节数：1 MiB。
pub const DEFAULT_MAX_FILE_BYTES: u64 = 1024 * 1024;
/// 默认允许的累计解压文件字节数：4 MiB。
pub const DEFAULT_MAX_TOTAL_BYTES: u64 = 4 * 1024 * 1024;
/// 默认允许的单文件解压/压缩比。
pub const DEFAULT_MAX_COMPRESSION_RATIO: u64 = 100;
/// 默认允许的相对路径段数。
pub const DEFAULT_MAX_PATH_DEPTH: usize = 8;

/// 宿主允许配置的 ZIP 输入字节硬上限：64 MiB。
pub const MAX_ARCHIVE_BYTES_LIMIT: u64 = 64 * 1024 * 1024;
/// 宿主允许配置的中央目录条目硬上限。
pub const MAX_ARCHIVE_ENTRIES_LIMIT: usize = 512;
/// 宿主允许配置的单文件解压字节硬上限：8 MiB。
pub const MAX_FILE_BYTES_LIMIT: u64 = 8 * 1024 * 1024;
/// 宿主允许配置的累计解压字节硬上限：32 MiB。
pub const MAX_TOTAL_BYTES_LIMIT: u64 = 32 * 1024 * 1024;
/// 宿主允许配置的解压/压缩比硬上限。
pub const MAX_COMPRESSION_RATIO_LIMIT: u64 = 1000;
/// 宿主允许配置的相对路径深度硬上限。
pub const MAX_PATH_DEPTH_LIMIT: usize = 32;

/// 普通协议包 ZIP 的全部资源门禁。
///
/// 字段保持私有，构造和反序列化都会重新校验。`max_file_bytes` 不能大于
/// `max_total_bytes`，因此单文件成功路径必然也能被累计上限表达。
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(
    try_from = "ProtocolArchiveLimitsWire",
    into = "ProtocolArchiveLimitsWire"
)]
pub struct ProtocolArchiveLimits {
    archive_bytes: u64,
    entries: usize,
    file_bytes: u64,
    total_bytes: u64,
    compression_ratio: u64,
    path_depth: usize,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
#[allow(clippy::struct_field_names)] // `max_*` 是面向配置文件的明确 Wire 契约，不是内部命名重复。
struct ProtocolArchiveLimitsWire {
    max_archive_bytes: u64,
    max_entries: usize,
    max_file_bytes: u64,
    max_total_bytes: u64,
    max_compression_ratio: u64,
    max_path_depth: usize,
}

impl ProtocolArchiveLimits {
    /// 创建一组受宿主硬上限保护的 ZIP 门禁。
    pub fn new(
        max_archive_bytes: u64,
        max_entries: usize,
        max_file_bytes: u64,
        max_total_bytes: u64,
        max_compression_ratio: u64,
        max_path_depth: usize,
    ) -> Result<Self, ProtocolArchiveError> {
        let valid = (1..=MAX_ARCHIVE_BYTES_LIMIT).contains(&max_archive_bytes)
            && (1..=MAX_ARCHIVE_ENTRIES_LIMIT).contains(&max_entries)
            && (1..=MAX_FILE_BYTES_LIMIT).contains(&max_file_bytes)
            && (max_file_bytes..=MAX_TOTAL_BYTES_LIMIT).contains(&max_total_bytes)
            && (1..=MAX_COMPRESSION_RATIO_LIMIT).contains(&max_compression_ratio)
            && (1..=MAX_PATH_DEPTH_LIMIT).contains(&max_path_depth);
        if !valid {
            return Err(ProtocolArchiveError::archive(
                ProtocolArchiveErrorCode::InvalidLimits,
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

    /// 返回完整 ZIP 压缩输入字节上限。
    #[must_use]
    pub const fn max_archive_bytes(&self) -> u64 {
        self.archive_bytes
    }

    /// 返回中央目录条目数量上限，目录条目也计数。
    #[must_use]
    pub const fn max_entries(&self) -> usize {
        self.entries
    }

    /// 返回单个普通文件解压字节上限。
    #[must_use]
    pub const fn max_file_bytes(&self) -> u64 {
        self.file_bytes
    }

    /// 返回全部普通文件累计解压字节上限。
    #[must_use]
    pub const fn max_total_bytes(&self) -> u64 {
        self.total_bytes
    }

    /// 返回单个普通文件允许的最大解压/压缩比。
    #[must_use]
    pub const fn max_compression_ratio(&self) -> u64 {
        self.compression_ratio
    }

    /// 返回条目相对路径允许的最大段数。
    #[must_use]
    pub const fn max_path_depth(&self) -> usize {
        self.path_depth
    }
}

impl Default for ProtocolArchiveLimits {
    fn default() -> Self {
        Self {
            archive_bytes: DEFAULT_MAX_ARCHIVE_BYTES,
            entries: DEFAULT_MAX_ARCHIVE_ENTRIES,
            file_bytes: DEFAULT_MAX_FILE_BYTES,
            total_bytes: DEFAULT_MAX_TOTAL_BYTES,
            compression_ratio: DEFAULT_MAX_COMPRESSION_RATIO,
            path_depth: DEFAULT_MAX_PATH_DEPTH,
        }
    }
}

impl TryFrom<ProtocolArchiveLimitsWire> for ProtocolArchiveLimits {
    type Error = ProtocolArchiveError;

    fn try_from(wire: ProtocolArchiveLimitsWire) -> Result<Self, Self::Error> {
        Self::new(
            wire.max_archive_bytes,
            wire.max_entries,
            wire.max_file_bytes,
            wire.max_total_bytes,
            wire.max_compression_ratio,
            wire.max_path_depth,
        )
    }
}

impl From<ProtocolArchiveLimits> for ProtocolArchiveLimitsWire {
    fn from(limits: ProtocolArchiveLimits) -> Self {
        Self {
            max_archive_bytes: limits.archive_bytes,
            max_entries: limits.entries,
            max_file_bytes: limits.file_bytes,
            max_total_bytes: limits.total_bytes,
            max_compression_ratio: limits.compression_ratio,
            max_path_depth: limits.path_depth,
        }
    }
}
