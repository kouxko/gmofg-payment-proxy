use std::fmt;

use intercept_proxy_domain::ProtocolPackageRef;
use serde::{Deserialize, Serialize};
use thiserror::Error;

/// 切帧阶段能够配置的有界资源。
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ProtocolFramingLimit {
    /// 单个完整 Frame 允许包含的最大字节数。
    FrameBytes,
    /// 单连接、单方向 FIFO 允许保留的最大字节数。
    FifoBytes,
}

impl fmt::Display for ProtocolFramingLimit {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(match self {
            Self::FrameBytes => "frame_bytes",
            Self::FifoBytes => "fifo_bytes",
        })
    }
}

/// 切帧失败的稳定分类。
///
/// 调用方使用该枚举分支或 [`ProtocolFramingError::code`] 判断原因，不需要解析 Rhai 错误文本。
/// 错误中不包含原始报文字节、脚本源码或文件系统路径。
#[derive(Clone, Debug, Deserialize, Eq, Error, PartialEq, Serialize)]
#[serde(tag = "code", rename_all = "SCREAMING_SNAKE_CASE", deny_unknown_fields)]
pub enum ProtocolFramingError {
    /// 单项限制为零或超过宿主硬上限。
    #[error("协议切帧限制 {limit} 的值 {value} 无效；允许范围为 1..={maximum}")]
    InvalidLimit {
        /// 被拒绝的限制项。
        limit: ProtocolFramingLimit,
        /// 调用方提供的值。
        value: u64,
        /// 宿主硬上限。
        maximum: u64,
    },
    /// FIFO 上限小于单 Frame 上限，无法保证合法 Frame 能完整进入缓冲区。
    #[error("协议切帧 FIFO 上限 {fifo_bytes} 小于 Frame 上限 {frame_bytes}")]
    FifoSmallerThanFrame {
        /// 单 Frame 上限。
        frame_bytes: u64,
        /// 单方向 FIFO 上限。
        fifo_bytes: u64,
    },
    /// Reader 的读取范围越过当前只读视图。
    #[error("Reader 读取范围越界")]
    ReaderOutOfBounds,
    /// Reader.find 收到空 pattern。
    #[error("Reader.find 的 pattern 不能为空")]
    EmptyFindPattern,
    /// Reader.find 的起始 offset 不在当前可用范围内。
    #[error("Reader.find 的 start_offset 无效")]
    InvalidFindStart,
    /// 脚本给 framing 构造器传入负数或无法表示为宿主长度的整数。
    #[error("FramingDecision 的长度参数无效")]
    InvalidDecisionLength,
    /// reject 原因为空或超过宿主允许的诊断长度。
    #[error("FramingDecision 的 reject 原因无效")]
    InvalidRejectReason,
    /// `need_more` 没有请求比当前 `available` 更大的总长度。
    #[error("frame() 返回 need_more 但没有取得进展")]
    NeedMoreWithoutProgress,
    /// complete 请求了零字节。
    #[error("frame() 返回 complete(0)")]
    CompleteEmpty,
    /// complete 请求的前缀超过 Reader 当前可用字节。
    #[error("frame() 返回的 complete 长度超过当前 FIFO")]
    CompleteOutOfBounds,
    /// 脚本请求或完成的单 Frame 超过配置上限。
    #[error("Frame 长度 {frame_bytes} 超过上限 {maximum}")]
    FrameTooLarge {
        /// 脚本请求或完成的 Frame 字节数。
        frame_bytes: u64,
        /// 当前单 Frame 上限。
        maximum: u64,
    },
    /// 单方向 FIFO 无法在其硬上限内继续接收数据。
    #[error("单方向 Frame FIFO 已达到 {maximum} 字节上限")]
    FifoLimitExceeded {
        /// 当前 FIFO 上限。
        maximum: u64,
    },
    /// `frame()` 主动拒绝当前字节流。
    #[error("frame() 拒绝当前字节流：{reason}")]
    Rejected {
        /// 由协议作者提供、且已经过长度约束的诊断原因。
        reason: String,
    },
    /// `frame()` 没有返回 `FramingDecision`，或执行期间发生脚本错误。
    #[error(
        "协议包 {id}@{version} 的 frame 入口执行失败",
        id = .package.id,
        version = .package.version
    )]
    FrameEntryFailed {
        /// 失败包的精确 ID 与版本。
        package: ProtocolPackageRef,
    },
    /// 对端 EOF 时 FIFO 仍保留不完整 Frame。
    #[error("连接结束时仍有 {buffered_bytes} 字节未组成完整 Frame")]
    TruncatedFrame {
        /// EOF 时尚未消费的字节数；不包含字节内容。
        buffered_bytes: u64,
    },
}

/// 无需解析 Display 文本即可持久化或映射的切帧错误码。
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum ProtocolFramingErrorCode {
    /// 限制值不在硬范围内。
    InvalidLimit,
    /// FIFO 小于 Frame 上限。
    FifoSmallerThanFrame,
    /// Reader 读取越界。
    ReaderOutOfBounds,
    /// find pattern 为空。
    EmptyFindPattern,
    /// find 起始位置无效。
    InvalidFindStart,
    /// Decision 长度值无效。
    InvalidDecisionLength,
    /// reject 原因无效。
    InvalidRejectReason,
    /// `need_more` 没有进展。
    NeedMoreWithoutProgress,
    /// complete 长度为零。
    CompleteEmpty,
    /// complete 越过可用数据。
    CompleteOutOfBounds,
    /// 单 Frame 过大。
    FrameTooLarge,
    /// FIFO 达到上限。
    FifoLimitExceeded,
    /// 脚本主动拒绝。
    Rejected,
    /// frame 入口执行失败。
    FrameEntryFailed,
    /// EOF 时残留截断 Frame。
    TruncatedFrame,
}

impl ProtocolFramingError {
    /// 返回稳定错误码。
    #[must_use]
    pub const fn code(&self) -> ProtocolFramingErrorCode {
        match self {
            Self::InvalidLimit { .. } => ProtocolFramingErrorCode::InvalidLimit,
            Self::FifoSmallerThanFrame { .. } => ProtocolFramingErrorCode::FifoSmallerThanFrame,
            Self::ReaderOutOfBounds => ProtocolFramingErrorCode::ReaderOutOfBounds,
            Self::EmptyFindPattern => ProtocolFramingErrorCode::EmptyFindPattern,
            Self::InvalidFindStart => ProtocolFramingErrorCode::InvalidFindStart,
            Self::InvalidDecisionLength => ProtocolFramingErrorCode::InvalidDecisionLength,
            Self::InvalidRejectReason => ProtocolFramingErrorCode::InvalidRejectReason,
            Self::NeedMoreWithoutProgress => ProtocolFramingErrorCode::NeedMoreWithoutProgress,
            Self::CompleteEmpty => ProtocolFramingErrorCode::CompleteEmpty,
            Self::CompleteOutOfBounds => ProtocolFramingErrorCode::CompleteOutOfBounds,
            Self::FrameTooLarge { .. } => ProtocolFramingErrorCode::FrameTooLarge,
            Self::FifoLimitExceeded { .. } => ProtocolFramingErrorCode::FifoLimitExceeded,
            Self::Rejected { .. } => ProtocolFramingErrorCode::Rejected,
            Self::FrameEntryFailed { .. } => ProtocolFramingErrorCode::FrameEntryFailed,
            Self::TruncatedFrame { .. } => ProtocolFramingErrorCode::TruncatedFrame,
        }
    }
}

pub(crate) type ProtocolFramingResult<T> = Result<T, ProtocolFramingError>;
