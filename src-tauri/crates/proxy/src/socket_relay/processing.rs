use std::fmt::{Debug, Formatter};
use std::net::SocketAddr;

use async_trait::async_trait;
use intercept_proxy_exchange::{
    Decode, Direction, Display, Downstream, Encode, Error, ExternalPackageCallFailure, Frame,
    Rules, Socket, SocketContext, Upstream,
};
use uuid::Uuid;

use crate::transport::relay::RelayBytes;

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

/// 一次 Socket Document 与 Encode 的联合事务输入。
///
/// 具体协议包、原始字节和规则 Program 由 Infrastructure 持有；Proxy/Application 边界只负责让
/// 统一规则 actor 在持久化生命周期前完成 gate 与 Encode。
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct JointConditionEvaluation {
    pub matched: bool,
    pub eligible_without_nth: bool,
    pub contains_nth: bool,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum JointRuleConditionEvaluation {
    UnifiedOwned(JointConditionEvaluation),
    NotOwned,
}

#[async_trait]
pub trait SocketJointEvaluation: Send + Sync {
    fn gate(
        &mut self,
        rule_id: Uuid,
        nth_attempt: u64,
    ) -> crate::Result<JointRuleConditionEvaluation>;
    async fn encode(self: Box<Self>) -> Result<SocketContext, Error>;
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
    /// 还需读取，但处理器无法提前知道完整 Frame 的总长度。
    ///
    /// 该语义适用于由外部协议实现逐步检查分隔符或密文边界的场景。Socket Pipeline 仍拥有
    /// 读取节奏与最大缓冲区限制；处理器不得用伪造的 `current + 1` 长度表达未知边界。
    NeedMoreUnknown,
    /// 缓冲区开头已有完整 Frame；`bytes` 是需要精确消费的字节数。
    Complete { bytes: usize },
    /// 当前输入不可能构成合法 Frame。原因只供内部诊断，不进入公开 observer。
    Reject { reason: String },
}

/// Socket Pipeline 的稳定失败分类。
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
    pub external_package_call: Option<Box<ExternalPackageCallFailure>>,
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
            external_package_call: None,
        }
    }

    #[must_use]
    pub fn with_external_package_call(mut self, failure: ExternalPackageCallFailure) -> Self {
        self.external_package_call = Some(Box::new(failure));
        self
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
/// Buffer/output 上限在开始下一次读取或写入前检查。读写 timeout 来自 Listener 配置；
/// package Hook 不增加执行期限。
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct SocketPipelineLimits {
    max_buffer: usize,
    max_output: usize,
    read_chunk: usize,
}

impl SocketPipelineLimits {
    /// 构造严格非零的限制；read chunk 不能大于总 buffer 上限。
    pub fn new(
        max_buffer_bytes: usize,
        max_output_bytes: usize,
        read_chunk_bytes: usize,
    ) -> Result<Self, SocketProcessingFailure> {
        if max_buffer_bytes == 0
            || max_output_bytes == 0
            || read_chunk_bytes == 0
            || read_chunk_bytes > max_buffer_bytes
        {
            return Err(SocketProcessingFailure::new(
                SocketProcessingFailureKind::InvalidLimits,
                "socket pipeline limits must be non-zero and read chunk must fit the buffer",
            ));
        }
        Ok(Self {
            max_buffer: max_buffer_bytes,
            max_output: max_output_bytes,
            read_chunk: read_chunk_bytes,
        })
    }

    /// 单方向允许保留的最大未消费输入字节数。
    pub const fn max_buffer_bytes(self) -> usize {
        self.max_buffer
    }

    /// 单个 processor 输出允许写入线路的最大字节数。
    pub const fn max_output_bytes(self) -> usize {
        self.max_output
    }

    /// 每次从 Socket 读取的最大字节数。
    pub const fn read_chunk_bytes(self) -> usize {
        self.read_chunk
    }
}

/// 一个连接方向独占的五项协议能力。
///
/// 类型参数把方向固定在装配期；运行时不能把 upstream 能力误装到 downstream Pipeline。
/// 每个字段是真实执行边界，不允许再用组合 `process()` 或 Identity adapter 冒充阶段。
pub struct SocketDirectionCapabilities<D: Direction> {
    pub frame: Box<dyn Frame<D>>,
    pub decode: Box<dyn Decode<Socket, D>>,
    pub display: Box<dyn Display>,
    pub rules: Box<dyn Rules>,
    pub encode: Box<dyn Encode<Socket, D>>,
}

impl<D: Direction> Debug for SocketDirectionCapabilities<D> {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("SocketDirectionCapabilities")
            .field("direction", &D::KIND)
            .finish_non_exhaustive()
    }
}

/// Factory 拥有的稳定 Listener 观测归属；连接字段由 accept loop 的 identity 补齐。
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SocketObservationMetadata {
    pub workspace_id: String,
    pub listener_id: String,
}

impl SocketObservationMetadata {
    /// Creates the connection-level parent span shared by raw and protocol Socket exchanges.
    ///
    /// All fields are recorded as primitive strings so the UI tracing layer can export the
    /// metadata without depending on Socket runtime types.
    pub(crate) fn exchange_span(
        &self,
        identity: &SocketConnectionIdentity,
        endpoint: &str,
    ) -> tracing::Span {
        let runtime_epoch = identity.runtime_epoch.to_string();
        let connection_id = identity.connection_id.to_string();
        let peer = identity.peer_addr.to_string();
        tracing::info_span!(
            target: "intercept_proxy::exchange",
            "socket_connection",
            workspace_id = self.workspace_id.as_str(),
            listener_id = self.listener_id.as_str(),
            runtime_epoch = runtime_epoch.as_str(),
            connection_id = connection_id.as_str(),
            peer = peer.as_str(),
            protocol = "socket",
            endpoint,
        )
    }
}

impl<D: Direction> SocketDirectionCapabilities<D> {
    pub fn new(
        frame: Box<dyn Frame<D>>,
        decode: Box<dyn Decode<Socket, D>>,
        display: Box<dyn Display>,
        rules: Box<dyn Rules>,
        encode: Box<dyn Encode<Socket, D>>,
    ) -> Self {
        Self {
            frame,
            decode,
            display,
            rules,
            encode,
        }
    }
}

/// 为每个 Socket connection 创建两组方向强类型的真实 Pipeline 能力。
///
/// Factory 只冻结连接级协议状态，不读取或写入业务 Socket。构造失败直接结束该 Exchange，
/// 不生成失败 processor，也不降级为透明转发。
pub trait SocketProtocolCapabilityFactory: Send + Sync {
    fn observation_metadata(&self) -> SocketObservationMetadata;

    fn create_upstream(
        &self,
        connection: SocketConnectionIdentity,
    ) -> Result<SocketDirectionCapabilities<Upstream>, SocketProcessingFailure>;

    fn create_downstream(
        &self,
        connection: SocketConnectionIdentity,
    ) -> Result<SocketDirectionCapabilities<Downstream>, SocketProcessingFailure>;
}

#[cfg(test)]
mod observation_tests {
    use std::collections::BTreeMap;
    use std::sync::{Arc, Mutex};

    use tracing::field::{Field, Visit};
    use tracing::span::{Attributes, Id, Record};
    use tracing::{Event, Metadata, Subscriber};

    use super::{SocketConnectionIdentity, SocketObservationMetadata};

    #[test]
    fn exchange_parent_span_exports_stable_primitive_metadata() {
        let fields = Arc::new(Mutex::new(BTreeMap::new()));
        let subscriber = RecordingSubscriber {
            fields: Arc::clone(&fields),
        };
        let connection_id = uuid::Uuid::parse_str("aaaaaaaa-bbbb-cccc-dddd-eeeeeeeeeeee")
            .expect("fixed connection UUID");
        let runtime_epoch = uuid::Uuid::parse_str("11111111-2222-3333-4444-555555555555")
            .expect("fixed runtime UUID");
        let identity = SocketConnectionIdentity {
            runtime_epoch,
            connection_id,
            peer_addr: "10.0.28.197:43210".parse().expect("fixed peer address"),
        };
        let metadata = SocketObservationMetadata {
            workspace_id: "workspace-1".to_owned(),
            listener_id: "listener-1".to_owned(),
        };

        tracing::subscriber::with_default(subscriber, || {
            let span = metadata.exchange_span(&identity, "10.0.34.151:9000");
            let span_metadata = span.metadata().expect("enabled parent span metadata");
            assert_eq!(span_metadata.target(), "intercept_proxy::exchange");
            assert_eq!(span_metadata.name(), "socket_connection");
        });

        assert_eq!(
            *fields.lock().expect("recorded span fields"),
            BTreeMap::from([
                ("connection_id".to_owned(), connection_id.to_string()),
                ("endpoint".to_owned(), "10.0.34.151:9000".to_owned()),
                ("listener_id".to_owned(), "listener-1".to_owned()),
                ("peer".to_owned(), "10.0.28.197:43210".to_owned()),
                ("protocol".to_owned(), "socket".to_owned()),
                ("runtime_epoch".to_owned(), runtime_epoch.to_string()),
                ("workspace_id".to_owned(), "workspace-1".to_owned()),
            ])
        );
    }

    struct RecordingSubscriber {
        fields: Arc<Mutex<BTreeMap<String, String>>>,
    }

    impl Subscriber for RecordingSubscriber {
        fn enabled(&self, _metadata: &Metadata<'_>) -> bool {
            true
        }

        fn new_span(&self, attributes: &Attributes<'_>) -> Id {
            attributes.record(&mut FieldRecorder(&self.fields));
            Id::from_u64(1)
        }

        fn record(&self, _span: &Id, values: &Record<'_>) {
            values.record(&mut FieldRecorder(&self.fields));
        }

        fn record_follows_from(&self, _span: &Id, _follows: &Id) {}

        fn event(&self, _event: &Event<'_>) {}

        fn enter(&self, _span: &Id) {}

        fn exit(&self, _span: &Id) {}
    }

    struct FieldRecorder<'a>(&'a Mutex<BTreeMap<String, String>>);

    impl Visit for FieldRecorder<'_> {
        fn record_str(&mut self, field: &Field, value: &str) {
            self.0
                .lock()
                .expect("span fields lock")
                .insert(field.name().to_owned(), value.to_owned());
        }

        fn record_debug(&mut self, field: &Field, value: &dyn std::fmt::Debug) {
            self.0
                .lock()
                .expect("span fields lock")
                .insert(field.name().to_owned(), format!("{value:?}"));
        }
    }
}
