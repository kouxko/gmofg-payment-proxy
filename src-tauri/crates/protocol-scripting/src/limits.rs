use serde::{Deserialize, Serialize};

use crate::{ProtocolResourceLimit, ProtocolRuntimeError, ProtocolRuntimeResult};

/// 默认单入口 Rhai 操作数上限。
pub const DEFAULT_MAX_OPERATIONS: u64 = 100_000;
/// 宿主允许配置的最大单入口 Rhai 操作数。
pub const MAX_OPERATIONS_LIMIT: u64 = 10_000_000;
/// 默认函数调用深度上限。
pub const DEFAULT_MAX_CALL_DEPTH: u64 = 32;
/// 宿主允许配置的最大函数调用深度。
pub const MAX_CALL_DEPTH_LIMIT: u64 = 64;
/// 默认单字符串 UTF-8 字节上限。
pub const DEFAULT_MAX_STRING_BYTES: u64 = 64 * 1024;
/// 宿主允许配置的最大单字符串 UTF-8 字节上限。
pub const MAX_STRING_BYTES_LIMIT: u64 = 1024 * 1024;
/// 默认单 Blob 字节上限。
pub const DEFAULT_MAX_BLOB_BYTES: u64 = 1024 * 1024;
/// 宿主允许配置的最大单 Blob 字节上限。
pub const MAX_BLOB_BYTES_LIMIT: u64 = 16 * 1024 * 1024;
/// 默认单入口墙钟执行时间上限（毫秒）。
pub const DEFAULT_MAX_WALL_TIME_MS: u64 = 250;
/// 宿主允许配置的最大单入口墙钟执行时间上限（毫秒）。
pub const MAX_WALL_TIME_MS_LIMIT: u64 = 30_000;

/// 单次协议入口调用使用的全部硬资源限制。
///
/// 所有字段都必须在 `1..=宿主硬上限` 范围内。字段保持私有，构造和反序列化统一经过
/// [`ProtocolRuntimeLimits::new`]，避免从配置文件或 IPC 绕过安全门禁。
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(
    try_from = "ProtocolRuntimeLimitsWire",
    into = "ProtocolRuntimeLimitsWire"
)]
pub struct ProtocolRuntimeLimits {
    operations: u64,
    call_depth: u64,
    string_bytes: u64,
    blob_bytes: u64,
    wall_time_ms: u64,
}

#[derive(Clone, Copy, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
// 私有 wire DTO 只承载数值；TryFrom 会重新执行每一项硬上限校验。
struct ProtocolRuntimeLimitsWire {
    #[serde(rename = "max_operations")]
    operations: u64,
    #[serde(rename = "max_call_depth")]
    call_depth: u64,
    #[serde(rename = "max_string_bytes")]
    string_bytes: u64,
    #[serde(rename = "max_blob_bytes")]
    blob_bytes: u64,
    #[serde(rename = "max_wall_time_ms")]
    wall_time_ms: u64,
}

impl ProtocolRuntimeLimits {
    /// 校验五项限制并创建不可绕过的运行时配置。
    pub fn new(
        max_operations: u64,
        max_call_depth: u64,
        max_string_bytes: u64,
        max_blob_bytes: u64,
        max_wall_time_ms: u64,
    ) -> ProtocolRuntimeResult<Self> {
        validate_limit(
            ProtocolResourceLimit::Operations,
            max_operations,
            MAX_OPERATIONS_LIMIT,
        )?;
        validate_limit(
            ProtocolResourceLimit::CallDepth,
            max_call_depth,
            MAX_CALL_DEPTH_LIMIT,
        )?;
        validate_limit(
            ProtocolResourceLimit::StringBytes,
            max_string_bytes,
            MAX_STRING_BYTES_LIMIT,
        )?;
        validate_limit(
            ProtocolResourceLimit::BlobBytes,
            max_blob_bytes,
            MAX_BLOB_BYTES_LIMIT,
        )?;
        validate_limit(
            ProtocolResourceLimit::WallTimeMs,
            max_wall_time_ms,
            MAX_WALL_TIME_MS_LIMIT,
        )?;
        Ok(Self {
            operations: max_operations,
            call_depth: max_call_depth,
            string_bytes: max_string_bytes,
            blob_bytes: max_blob_bytes,
            wall_time_ms: max_wall_time_ms,
        })
    }

    /// 返回单入口操作数上限。
    #[must_use]
    pub const fn max_operations(self) -> u64 {
        self.operations
    }

    /// 返回函数调用深度上限。
    #[must_use]
    pub const fn max_call_depth(self) -> u64 {
        self.call_depth
    }

    /// 返回单字符串 UTF-8 字节上限。
    #[must_use]
    pub const fn max_string_bytes(self) -> u64 {
        self.string_bytes
    }

    /// 返回单 Blob 字节上限。
    #[must_use]
    pub const fn max_blob_bytes(self) -> u64 {
        self.blob_bytes
    }

    /// 返回单入口墙钟执行时间上限（毫秒）。
    #[must_use]
    pub const fn max_wall_time_ms(self) -> u64 {
        self.wall_time_ms
    }
}

impl Default for ProtocolRuntimeLimits {
    fn default() -> Self {
        // 默认常量由同模块测试逐项证明位于硬上限内；保持字段直写可让 Default 无 panic。
        Self {
            operations: DEFAULT_MAX_OPERATIONS,
            call_depth: DEFAULT_MAX_CALL_DEPTH,
            string_bytes: DEFAULT_MAX_STRING_BYTES,
            blob_bytes: DEFAULT_MAX_BLOB_BYTES,
            wall_time_ms: DEFAULT_MAX_WALL_TIME_MS,
        }
    }
}

impl TryFrom<ProtocolRuntimeLimitsWire> for ProtocolRuntimeLimits {
    type Error = ProtocolRuntimeError;

    fn try_from(value: ProtocolRuntimeLimitsWire) -> Result<Self, Self::Error> {
        Self::new(
            value.operations,
            value.call_depth,
            value.string_bytes,
            value.blob_bytes,
            value.wall_time_ms,
        )
    }
}

impl From<ProtocolRuntimeLimits> for ProtocolRuntimeLimitsWire {
    fn from(value: ProtocolRuntimeLimits) -> Self {
        Self {
            operations: value.operations,
            call_depth: value.call_depth,
            string_bytes: value.string_bytes,
            blob_bytes: value.blob_bytes,
            wall_time_ms: value.wall_time_ms,
        }
    }
}

fn validate_limit(
    limit: ProtocolResourceLimit,
    value: u64,
    maximum: u64,
) -> ProtocolRuntimeResult<()> {
    if value == 0 || value > maximum {
        Err(ProtocolRuntimeError::InvalidResourceLimit {
            limit,
            value,
            maximum,
        })
    } else {
        Ok(())
    }
}
