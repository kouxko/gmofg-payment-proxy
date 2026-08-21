use std::fmt;

use serde_json::Value;
use tokio_tungstenite::tungstenite::Error as WebSocketError;

/// 外部软件包连接的致命协议错误分类。
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ExternalPackageFatalProtocolError {
    /// 对端发送的文本不是合法 JSON。
    InvalidJson,
    /// JSON-RPC envelope 不符合 2.0 响应合同。
    InvalidResponse,
    /// 响应 ID 不属于当前连接代次中的任何调用。
    WrongResponseId,
    /// 对端对已经完成的调用重复发送响应。
    DuplicateResponse,
    /// 注册结果不符合严格领域合同。
    InvalidRegistration,
    /// 注册阶段收到非文本业务消息或异常关闭。
    RegistrationProtocolViolation,
}

/// 第三方返回的标准 JSON-RPC error。
///
/// `Debug` 特意不输出 `data`，避免日志或崩溃报告意外泄露第三方 payload。诊断层若确有需要，
/// 应通过 [`Self::data`] 显式读取并执行自己的脱敏策略。
#[derive(Clone)]
pub struct ExternalPackageRemoteError {
    code: i64,
    message: String,
    data: Option<Value>,
}

impl ExternalPackageRemoteError {
    pub(crate) fn new(code: i64, message: String, data: Option<Value>) -> Self {
        Self {
            code,
            message,
            data,
        }
    }

    /// 返回第三方错误码。
    #[must_use]
    pub const fn code(&self) -> i64 {
        self.code
    }

    /// 返回第三方错误消息。
    #[must_use]
    pub fn message(&self) -> &str {
        &self.message
    }

    /// 返回第三方附带的数据；调用方必须先决定如何脱敏再记录。
    #[must_use]
    pub const fn data(&self) -> Option<&Value> {
        self.data.as_ref()
    }
}

impl fmt::Debug for ExternalPackageRemoteError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("ExternalPackageRemoteError")
            .field("code", &self.code)
            .field("message", &self.message)
            .field("data", &self.data.as_ref().map(|_| "<redacted>"))
            .finish()
    }
}

/// 外部软件包连接建立或调用失败。
#[derive(Clone)]
pub enum ExternalPackageConnectionError {
    /// 单软件包并发额度已经耗尽；调用未排队也未发送。
    Busy,
    /// 调用超过本地配置的期限；Proxy 不会自动重试。
    Timeout {
        /// JSON-RPC 请求 ID。
        request_id: String,
        /// 被调用的方法。
        method: String,
    },
    /// WebSocket 已断开或 actor 已停止。
    Disconnected,
    /// 第三方为对应调用返回标准 JSON-RPC error；连接仍保持在线。
    Remote {
        /// JSON-RPC 请求 ID。
        request_id: String,
        /// 被调用的方法。
        method: String,
        /// 严格解析后的远端错误。
        error: ExternalPackageRemoteError,
    },
    /// 当前请求或响应超过配置的消息大小限制。
    MessageTooLarge {
        /// 实际 UTF-8 JSON 字节数。
        actual_bytes: usize,
        /// 当前调用允许的最大 UTF-8 JSON 字节数。
        limit_bytes: usize,
    },
    /// JSON 参数或结果无法按调用类型序列化/反序列化。
    InvalidPayload(String),
    /// 协议已失效；actor 会关闭整个连接。
    Fatal(ExternalPackageFatalProtocolError),
    /// WebSocket 握手或传输失败。
    Transport(String),
}

impl fmt::Debug for ExternalPackageConnectionError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Busy => formatter.write_str("Busy"),
            Self::Timeout { request_id, method } => formatter
                .debug_struct("Timeout")
                .field("request_id", request_id)
                .field("method", method)
                .finish(),
            Self::Disconnected => formatter.write_str("Disconnected"),
            Self::Remote {
                request_id,
                method,
                error,
            } => formatter
                .debug_struct("Remote")
                .field("request_id", request_id)
                .field("method", method)
                .field("error", error)
                .finish(),
            Self::MessageTooLarge {
                actual_bytes,
                limit_bytes,
            } => formatter
                .debug_struct("MessageTooLarge")
                .field("actual_bytes", actual_bytes)
                .field("limit_bytes", limit_bytes)
                .finish(),
            Self::InvalidPayload(_) => formatter.write_str("InvalidPayload(<redacted>)"),
            Self::Fatal(kind) => formatter.debug_tuple("Fatal").field(kind).finish(),
            Self::Transport(_) => formatter.write_str("Transport(<redacted>)"),
        }
    }
}

impl fmt::Display for ExternalPackageConnectionError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Busy => formatter.write_str("外部软件包繁忙"),
            Self::Timeout { .. } => formatter.write_str("外部软件包调用超时"),
            Self::Disconnected => formatter.write_str("外部软件包连接已断开"),
            Self::Remote { error, .. } => write!(
                formatter,
                "外部软件包返回 JSON-RPC error {}: {}",
                error.code, error.message
            ),
            Self::MessageTooLarge { .. } => formatter.write_str("外部软件包消息超过大小限制"),
            Self::InvalidPayload(_) => formatter.write_str("外部软件包 payload 结构无效"),
            Self::Fatal(kind) => write!(formatter, "外部软件包协议失效: {kind:?}"),
            Self::Transport(_) => formatter.write_str("外部软件包 WebSocket 传输失败"),
        }
    }
}

impl std::error::Error for ExternalPackageConnectionError {}

impl From<WebSocketError> for ExternalPackageConnectionError {
    fn from(error: WebSocketError) -> Self {
        Self::Transport(error.to_string())
    }
}
