use serde::{Deserialize, Serialize};

use super::{ProtocolFramingError, ProtocolFramingLimit, ProtocolFramingResult};

/// 默认单 Frame 上限：1 MiB。
pub(crate) const DEFAULT_MAX_FRAME_BYTES: u64 = 1024 * 1024;
/// 宿主允许配置的最大单 Frame 上限：16 MiB。
pub(crate) const MAX_FRAME_BYTES_LIMIT: u64 = 16 * 1024 * 1024;
/// 默认单连接、单方向 FIFO 上限：2 MiB。
pub(crate) const DEFAULT_MAX_FRAME_FIFO_BYTES: u64 = 2 * 1024 * 1024;
/// 宿主允许配置的最大单方向 FIFO 上限：32 MiB。
pub(crate) const MAX_FRAME_FIFO_BYTES_LIMIT: u64 = 32 * 1024 * 1024;

/// 单连接、单方向切帧器的硬资源限制。
///
/// FIFO 上限必须不小于 Frame 上限，否则恰好位于合法上限的 Frame 永远无法完整进入缓冲区。
/// 字段保持私有，构造与反序列化都经过相同校验。
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(
    try_from = "ProtocolFramingLimitsWire",
    into = "ProtocolFramingLimitsWire"
)]
pub struct ProtocolFramingLimits {
    frame_bytes: u64,
    fifo_bytes: u64,
}

#[derive(Clone, Copy, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct ProtocolFramingLimitsWire {
    #[serde(rename = "max_frame_bytes")]
    frame_bytes: u64,
    #[serde(rename = "max_fifo_bytes")]
    fifo_bytes: u64,
}

impl ProtocolFramingLimits {
    /// 校验并创建切帧限制。
    pub fn new(max_frame_bytes: u64, max_fifo_bytes: u64) -> ProtocolFramingResult<Self> {
        validate_limit(
            ProtocolFramingLimit::FrameBytes,
            max_frame_bytes,
            MAX_FRAME_BYTES_LIMIT,
        )?;
        validate_limit(
            ProtocolFramingLimit::FifoBytes,
            max_fifo_bytes,
            MAX_FRAME_FIFO_BYTES_LIMIT,
        )?;
        if max_fifo_bytes < max_frame_bytes {
            return Err(ProtocolFramingError::FifoSmallerThanFrame {
                frame_bytes: max_frame_bytes,
                fifo_bytes: max_fifo_bytes,
            });
        }
        Ok(Self {
            frame_bytes: max_frame_bytes,
            fifo_bytes: max_fifo_bytes,
        })
    }

    /// 返回单个完整 Frame 上限。
    #[must_use]
    pub const fn max_frame_bytes(self) -> u64 {
        self.frame_bytes
    }

    /// 返回单连接、单方向 FIFO 上限。
    #[must_use]
    pub const fn max_fifo_bytes(self) -> u64 {
        self.fifo_bytes
    }

    pub(super) fn max_frame_usize(self) -> usize {
        usize::try_from(self.frame_bytes).unwrap_or(usize::MAX)
    }
}

impl Default for ProtocolFramingLimits {
    fn default() -> Self {
        Self {
            frame_bytes: DEFAULT_MAX_FRAME_BYTES,
            fifo_bytes: DEFAULT_MAX_FRAME_FIFO_BYTES,
        }
    }
}

impl TryFrom<ProtocolFramingLimitsWire> for ProtocolFramingLimits {
    type Error = ProtocolFramingError;

    fn try_from(value: ProtocolFramingLimitsWire) -> Result<Self, Self::Error> {
        Self::new(value.frame_bytes, value.fifo_bytes)
    }
}

impl From<ProtocolFramingLimits> for ProtocolFramingLimitsWire {
    fn from(value: ProtocolFramingLimits) -> Self {
        Self {
            frame_bytes: value.frame_bytes,
            fifo_bytes: value.fifo_bytes,
        }
    }
}

fn validate_limit(
    limit: ProtocolFramingLimit,
    value: u64,
    maximum: u64,
) -> ProtocolFramingResult<()> {
    if value == 0 || value > maximum {
        Err(ProtocolFramingError::InvalidLimit {
            limit,
            value,
            maximum,
        })
    } else {
        Ok(())
    }
}
