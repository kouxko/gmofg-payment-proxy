use std::fmt;

use intercept_proxy_domain::ProtocolPackageRef;
use serde::{Deserialize, Serialize};
use thiserror::Error;

/// `LocalResponder` 在 request/response bridge 上拒绝的所有权维度。
///
/// 该值只描述身份边界，不包含 Document 字段值、连接载荷或脚本源码，可安全进入结构化诊断。
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum LocalResponseOwnershipViolation {
    /// request/response 来自另一个精确协议包版本。
    Package,
    /// request/response 绑定了另一个 Document Schema。
    Schema,
    /// request/response 来自另一个 Socket 连接。
    Connection,
    /// request/response 虽有相同公开身份，但不是由当前协调器实例创建。
    Output,
}

impl fmt::Display for LocalResponseOwnershipViolation {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(match self {
            Self::Package => "package",
            Self::Schema => "schema",
            Self::Connection => "connection",
            Self::Output => "output",
        })
    }
}

/// 可被资源门禁拒绝的脚本执行维度。
///
/// 该枚举同时用于限制配置错误和运行时超限错误，调用方不需要解析消息文本来判断是哪一项门禁。
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ProtocolResourceLimit {
    /// 单次入口调用允许执行的最大 Rhai 操作数。
    Operations,
    /// 单次入口调用允许进入的最大函数调用深度。
    CallDepth,
    /// 单个脚本字符串允许占用的最大 UTF-8 字节数。
    StringBytes,
    /// 单个脚本 Blob 允许包含的最大字节数。
    BlobBytes,
    /// 单次入口调用允许使用的最大单调时钟毫秒数。
    WallTimeMs,
}

impl fmt::Display for ProtocolResourceLimit {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(match self {
            Self::Operations => "operations",
            Self::CallDepth => "call_depth",
            Self::StringBytes => "string_bytes",
            Self::BlobBytes => "blob_bytes",
            Self::WallTimeMs => "wall_time_ms",
        })
    }
}

/// Scripted Socket 运行时能够调用的协议入口。
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ProtocolEntryPoint {
    /// 从方向私有 FIFO 中判断一个完整 Frame 的边界。
    Frame,
    /// 把完整 Frame 解码为 Schema 绑定 Document。
    Decode,
    /// 把原始 Frame 与 Document 编码为完整输出 Frame。
    Encode,
    /// 根据 Document 生成隔离展示内容。
    Display,
}

impl fmt::Display for ProtocolEntryPoint {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(match self {
            Self::Frame => "frame",
            Self::Decode => "decode",
            Self::Encode => "encode",
            Self::Display => "display",
        })
    }
}

/// 协议包编译与入口执行的稳定错误契约。
///
/// Wire 形式使用 `code` 标签，不序列化脚本源码、绝对路径或底层引擎错误。详细内部原因后续只进入
/// 受控 Rust 日志；此类型为 Application/Infrastructure 提供可穷举、可脱敏的失败分类。
#[derive(Clone, Debug, Deserialize, Eq, Error, PartialEq, Serialize)]
#[serde(tag = "code", rename_all = "SCREAMING_SNAKE_CASE", deny_unknown_fields)]
pub enum ProtocolRuntimeError {
    /// 某项资源限制为零或超过宿主硬上限。
    #[error("协议脚本资源限制 {limit} 的值 {value} 无效；允许范围为 1..={maximum}")]
    InvalidResourceLimit {
        /// 被拒绝的限制维度。
        limit: ProtocolResourceLimit,
        /// 调用方提供的值。
        value: u64,
        /// 宿主允许的硬上限。
        maximum: u64,
    },
    /// 包内容未能生成可执行编译产物。
    #[error(
        "协议包 {id}@{version} 编译失败",
        id = .package.id,
        version = .package.version
    )]
    CompilationFailed {
        /// 失败包的精确 ID 与版本。
        package: ProtocolPackageRef,
    },
    /// 某个声明入口执行失败，但未触发资源门禁。
    #[error(
        "协议包 {id}@{version} 的 {entry} 入口执行失败",
        id = .package.id,
        version = .package.version
    )]
    EntryPointFailed {
        /// 失败包的精确 ID 与版本。
        package: ProtocolPackageRef,
        /// 失败的入口阶段。
        entry: ProtocolEntryPoint,
    },
    /// Decode 后交给宿主的 Document 变换阶段失败。
    ///
    /// 该阶段用于类型安全的 协议报文规则；错误不携带字段值、规则内容或第三方文本。
    #[error(
        "协议包 {id}@{version} 的 Document 变换失败",
        id = .package.id,
        version = .package.version
    )]
    DocumentTransformFailed {
        /// 失败包的精确 ID 与版本。
        package: ProtocolPackageRef,
    },
    /// `LocalResponder` 收到了不属于当前包、Schema、连接或协调器的桥接对象。
    #[error(
        "协议包 {id}@{version} 的 LocalResponder 拒绝外部 {violation} 对象",
        id = .package.id,
        version = .package.version
    )]
    LocalResponseOwnershipViolation {
        /// 当前协调器绑定的精确包版本。
        package: ProtocolPackageRef,
        /// 被拒绝的身份维度。
        violation: LocalResponseOwnershipViolation,
    },
    /// `LocalResponder` 在写入前得到空 response。
    #[error(
        "协议包 {id}@{version} 的 LocalResponder 生成了空 response",
        id = .package.id,
        version = .package.version
    )]
    LocalResponseEmpty {
        /// 生成空 response 的精确包版本。
        package: ProtocolPackageRef,
    },
    /// `LocalResponder` 在规则或 Echo 阶段观察到连接取消。
    #[error(
        "协议包 {id}@{version} 的 LocalResponder 执行已取消",
        id = .package.id,
        version = .package.version
    )]
    LocalResponseCancelled {
        /// 被取消的精确包版本。
        package: ProtocolPackageRef,
    },
    /// 调用方取消了正在运行或即将开始的协议入口。
    #[error(
        "协议包 {id}@{version} 的 {entry} 入口执行已取消",
        id = .package.id,
        version = .package.version
    )]
    ExecutionCancelled {
        /// 被取消包的精确 ID 与版本。
        package: ProtocolPackageRef,
        /// 被取消的入口阶段。
        entry: ProtocolEntryPoint,
    },
    /// 某个入口触发操作数、深度、数据大小或时间硬门禁。
    #[error(
        "协议包 {id}@{version} 的 {entry} 入口超过 {limit} 限制",
        id = .package.id,
        version = .package.version
    )]
    ResourceLimitExceeded {
        /// 失败包的精确 ID 与版本。
        package: ProtocolPackageRef,
        /// 触发限制的入口阶段。
        entry: ProtocolEntryPoint,
        /// 被触发的限制维度。
        limit: ProtocolResourceLimit,
    },
}

impl ProtocolRuntimeError {
    /// 返回无需解析 Display 文本即可持久化或映射的稳定错误码。
    #[must_use]
    pub const fn code(&self) -> &'static str {
        match self {
            Self::InvalidResourceLimit { .. } => "INVALID_RESOURCE_LIMIT",
            Self::CompilationFailed { .. } => "COMPILATION_FAILED",
            Self::EntryPointFailed { .. } => "ENTRY_POINT_FAILED",
            Self::DocumentTransformFailed { .. } => "DOCUMENT_TRANSFORM_FAILED",
            Self::LocalResponseOwnershipViolation { .. } => "LOCAL_RESPONSE_OWNERSHIP_VIOLATION",
            Self::LocalResponseEmpty { .. } => "LOCAL_RESPONSE_EMPTY",
            Self::LocalResponseCancelled { .. } => "LOCAL_RESPONSE_CANCELLED",
            Self::ExecutionCancelled { .. } => "EXECUTION_CANCELLED",
            Self::ResourceLimitExceeded { .. } => "RESOURCE_LIMIT_EXCEEDED",
        }
    }
}

/// 协议脚本编译与执行 API 的统一结果类型。
pub type ProtocolRuntimeResult<T> = Result<T, ProtocolRuntimeError>;
