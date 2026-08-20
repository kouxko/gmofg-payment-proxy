use std::fmt::{Debug, Formatter};
use std::net::SocketAddr;
use std::time::Duration;

use async_trait::async_trait;
use bytes::Bytes;
use uuid::Uuid;

use crate::transport::relay::RelayBytes;

use super::LocalResponderDiagnostics;

/// 一条完整 Frame 在 Socket 拓扑中的处理方向。
///
/// `LocalExchange` 是本地 request-response 交换，不应被解释成一次上游发送。
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SocketPayloadDirection {
    /// App 发往固定上游。
    AppToUpstream,
    /// 固定上游发回 App。
    UpstreamToApp,
    /// App request 由本机处理并向同一连接写回 response。
    LocalExchange,
}

/// Factory 创建连接级 processor 时收到的稳定、无 payload 身份信息。
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SocketConnectionIdentity {
    /// 当前 Listener run 的 epoch；重启后会变化。
    pub runtime_epoch: Uuid,
    /// 当前连接的唯一 ID。
    pub connection_id: Uuid,
    /// App 侧对端地址。
    pub peer_addr: SocketAddr,
}

/// Processor 对当前有界缓冲区的切帧判断。
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum FrameBoundary {
    /// 还需读取；`total` 是完成当前 Frame 所需的总字节数，必须大于当前长度。
    NeedMore { total: usize },
    /// 缓冲区开头已有完整 Frame；`bytes` 是需要精确消费的字节数。
    Complete { bytes: usize },
    /// 当前输入不可能构成合法 Frame。原因只供内部诊断，不进入公开 observer。
    Reject { reason: String },
}

/// Frame Pump 的稳定失败分类。
///
/// 该分类不携带原始 payload、脚本源码或第三方错误文本；Handler 只把稳定 code 发送给
/// observer。网络写入失败时，failure 内部还会保存已经成功提交的字节计数。
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SocketProcessingFailureKind {
    InvalidLimits,
    InvalidFrameBoundary,
    FrameRejected,
    BufferLimitExceeded,
    TruncatedFrame,
    ReadFailed,
    ReadTimeout,
    DecodeFailed,
    RuleFailed,
    EncodeFailed,
    ProcessingFailed,
    ProcessingTimeout,
    ProcessorPanicked,
    EmptyOutput,
    OutputLimitExceeded,
    WriteFailed,
    WriteTimeout,
    Cancelled,
}

impl SocketProcessingFailureKind {
    /// 返回供 terminal observer 和上层错误映射使用的稳定 code。
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::InvalidLimits => "INVALID_LIMITS",
            Self::InvalidFrameBoundary => "INVALID_FRAME_BOUNDARY",
            Self::FrameRejected => "FRAME_REJECTED",
            Self::BufferLimitExceeded => "BUFFER_LIMIT_EXCEEDED",
            Self::TruncatedFrame => "TRUNCATED_FRAME",
            Self::ReadFailed => "READ_FAILED",
            Self::ReadTimeout => "READ_TIMEOUT",
            Self::DecodeFailed => "DECODE_FAILED",
            Self::RuleFailed => "RULE_FAILED",
            Self::EncodeFailed => "ENCODE_FAILED",
            Self::ProcessingFailed => "PROCESSING_FAILED",
            Self::ProcessingTimeout => "PROCESSING_TIMEOUT",
            Self::ProcessorPanicked => "PROCESSOR_PANICKED",
            Self::EmptyOutput => "EMPTY_OUTPUT",
            Self::OutputLimitExceeded => "OUTPUT_LIMIT_EXCEEDED",
            Self::WriteFailed => "WRITE_FAILED",
            Self::WriteTimeout => "WRITE_TIMEOUT",
            Self::Cancelled => "CANCELLED",
        }
    }
}

#[derive(Clone, Eq, PartialEq)]
pub struct SocketProcessingFailure {
    /// 稳定失败分类。
    pub kind: SocketProcessingFailureKind,
    /// 失败所属方向；Pump 会覆盖 processor 自行填写的方向，防止误归因。
    pub direction: Option<SocketPayloadDirection>,
    _message: String,
    bytes: RelayBytes,
}

impl SocketProcessingFailure {
    /// 创建 processor 可返回的 typed failure。
    ///
    /// `message` 仅保留在 Proxy 内部且不会出现在 `Debug` 或 observer 事件中。
    pub fn new(kind: SocketProcessingFailureKind, message: impl Into<String>) -> Self {
        Self {
            kind,
            direction: None,
            _message: message.into(),
            bytes: RelayBytes::default(),
        }
    }

    /// 将失败绑定到 Pump 已知的真实方向。
    #[must_use]
    pub fn in_direction(mut self, direction: SocketPayloadDirection) -> Self {
        self.direction = Some(direction);
        self
    }

    /// 返回不含动态文本的稳定错误码。
    pub const fn stable_code(&self) -> &'static str {
        self.kind.as_str()
    }

    pub(crate) fn with_bytes(mut self, bytes: RelayBytes) -> Self {
        self.bytes = bytes;
        self
    }

    pub(crate) const fn bytes(&self) -> RelayBytes {
        self.bytes
    }
}

impl Debug for SocketProcessingFailure {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("SocketProcessingFailure")
            .field("kind", &self.kind)
            .field("direction", &self.direction)
            .field("bytes", &self.bytes)
            .finish_non_exhaustive()
    }
}

/// 每连接、每方向的硬资源限制。
///
/// Buffer/output 上限在开始下一次读取或写入前检查；processing timeout 同时覆盖
/// `inspect` 与 `process`。读写 timeout 来自 Listener 配置，由 Frame Pump 调用方提供。
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct SocketFramePumpLimits {
    max_buffer_bytes: usize,
    max_output_bytes: usize,
    read_chunk_bytes: usize,
    processing_timeout: Duration,
}

impl SocketFramePumpLimits {
    /// 构造严格非零的限制；read chunk 不能大于总 buffer 上限。
    pub fn new(
        max_buffer_bytes: usize,
        max_output_bytes: usize,
        read_chunk_bytes: usize,
        processing_timeout: Duration,
    ) -> Result<Self, SocketProcessingFailure> {
        if max_buffer_bytes == 0
            || max_output_bytes == 0
            || read_chunk_bytes == 0
            || read_chunk_bytes > max_buffer_bytes
            || processing_timeout.is_zero()
        {
            return Err(SocketProcessingFailure::new(
                SocketProcessingFailureKind::InvalidLimits,
                "frame pump limits must be non-zero and read chunk must fit the buffer",
            ));
        }
        Ok(Self {
            max_buffer_bytes,
            max_output_bytes,
            read_chunk_bytes,
            processing_timeout,
        })
    }

    /// 单方向允许保留的最大未消费输入字节数。
    pub const fn max_buffer_bytes(self) -> usize {
        self.max_buffer_bytes
    }

    /// 单个 processor 输出允许写入线路的最大字节数。
    pub const fn max_output_bytes(self) -> usize {
        self.max_output_bytes
    }

    /// 每次从 Socket 读取的最大字节数。
    pub const fn read_chunk_bytes(self) -> usize {
        self.read_chunk_bytes
    }

    /// 单次 `inspect` 或 `process` 的执行上限。
    pub const fn processing_timeout(self) -> Duration {
        self.processing_timeout
    }
}

/// 面向一个连接方向的、有状态 Frame processor。
///
/// 实现必须保持连接隔离，不得在方法内执行网络 I/O。Pump 保证同一实例严格串行调用：
/// 先用 `inspect` 判定完整 Frame，再把精确 origin 交给 `process`，并在完整写出返回值后
/// 才处理下一 Frame。
#[async_trait]
pub trait SocketFrameProcessor: Send {
    /// 检查当前从 Frame 起点开始的完整有界缓冲区，不消费输入。
    async fn inspect(&mut self, buffered: Bytes) -> Result<FrameBoundary, SocketProcessingFailure>;

    /// 处理一个完整 origin，返回一次且仅一次写出的完整输出 Blob。
    async fn process(&mut self, origin: Bytes) -> Result<Bytes, SocketProcessingFailure>;

    /// 注入 `LocalResponder` request 的协议中立旁路观察句柄。
    ///
    /// Relay 与既有 fake processor 使用默认空实现；LocalResponder processor 只保存句柄，
    /// 并在 request Frame/可选 Decode 成功后发布有界预览。
    fn set_local_diagnostics(&mut self, _diagnostics: LocalResponderDiagnostics) {}

    /// 通知 processor：上一次 `process` 的输出已经完整写入并 flush 成功。
    ///
    /// 默认实现为空，现有 Direct/fake processor 无需感知。通知发生在 Writing 之后，因而只能
    /// 用于 Display、捕获等旁路工作；实现不得再修改线路输出，也不得把失败升级为连接失败。
    fn output_committed(&mut self) {}

    /// 通知 processor：上一次 `process` 的输出未能完整写入。
    ///
    /// `written_bytes` 是 Pump 已确认成功写出的 response 前缀长度。默认实现为空；该通知
    /// 只允许形成失败诊断或失败 capture，不得重试写入，也不得发布成功完成事件。
    fn output_failed(&mut self, _failure: &SocketProcessingFailure, _written_bytes: usize) {}
}

/// 为 Scripted Relay 的两个方向分别创建连接级 processor。
///
/// 该同步方法必须快速、无阻塞；Factory panic 会被隔离成当前连接的 typed failure。
pub trait ScriptedRelayProcessorFactory: Send + Sync {
    /// 每个连接的每个 Relay 方向恰好调用一次。
    fn create_direction(
        &self,
        connection: SocketConnectionIdentity,
        direction: SocketPayloadDirection,
    ) -> Box<dyn SocketFrameProcessor>;
}

/// 为 `LocalResponder` 创建一体化 request-response processor。
///
/// 它不是某个伪造的 upstream 方向 processor；同一连接只创建一个 exchange，并由 Pump
/// 严格串行执行 request -> response write/flush。
pub trait LocalResponderProcessorFactory: Send + Sync {
    /// 每个 `LocalResponder` 连接恰好调用一次。
    fn create_exchange(
        &self,
        connection: SocketConnectionIdentity,
    ) -> Box<dyn SocketFrameProcessor>;
}
