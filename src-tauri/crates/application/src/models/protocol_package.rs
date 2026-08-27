//! 协议包生命周期的无源码应用模型。
//!
//! 这些类型只描述 Manifest 元数据、启用状态、校验结果和引用者。脚本源码、AST、ZIP
//! 原始字节与本机路径都不能进入 Application 返回模型，后续任何 UI/CLI 适配器只能基于
//! 这些经过收敛的字段展示协议包。

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use specta::Type;
use uuid::Uuid;

use super::{ListenerId, ListenerRuntimeState, ProtocolPackageId, ProtocolPackageRef, WorkspaceId};
use intercept_proxy_domain::{
    HttpBodyProcessing, ListenerDataPlane, ProxyListener, SocketPayloadProcessing,
};

pub const BUILTIN_ISO8583_PACKAGE_ID: &str = "iso8583-ascii-standard";
pub const BUILTIN_ISO8583_PACKAGE_VERSION: &str = "1.0.0";

#[must_use]
pub fn builtin_iso8583_package_ref() -> ProtocolPackageRef {
    ProtocolPackageRef {
        id: ProtocolPackageId::new(BUILTIN_ISO8583_PACKAGE_ID)
            .expect("built-in protocol package id is compile-time validated"),
        version: crate::ProtocolPackageVersion::new(BUILTIN_ISO8583_PACKAGE_VERSION)
            .expect("built-in protocol package version is compile-time validated"),
    }
}

#[must_use]
pub fn is_builtin_protocol_package(package: &ProtocolPackageRef) -> bool {
    package.id.as_str() == BUILTIN_ISO8583_PACKAGE_ID
        && package.version.as_str() == BUILTIN_ISO8583_PACKAGE_VERSION
}

/// 返回 Listener 数据面绑定的精确协议包；Plain HTTP 与 Direct Socket 不产生引用。
#[must_use]
pub fn listener_protocol_package(listener: &ProxyListener) -> Option<&ProtocolPackageRef> {
    match &listener.data_plane {
        ListenerDataPlane::Http(http) => match &http.body_processing {
            HttpBodyProcessing::Plain => None,
            HttpBodyProcessing::Protocol { package } => Some(package),
        },
        ListenerDataPlane::Socket(socket) => match &socket.processing {
            SocketPayloadProcessing::Direct => None,
            SocketPayloadProcessing::Scripted(scripted) => Some(&scripted.package),
        },
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(tag = "state", rename_all = "snake_case")]
/// 最近一次持久化校验的无源码结果。
pub enum ProtocolPackageValidationViewModel {
    Valid,
    Invalid { code: String },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(tag = "type", rename_all = "snake_case")]
/// 精确协议包版本的执行来源。
///
/// 内置 Rhai 与外部进程的可用性约束不同，因此使用 closed tagged union 表达，禁止调用方
/// 通过多个可空字段猜测来源。`built_in` 只标识官方起始包；用户导入的 Rhai 包仍属于
/// `Internal`。外部包的 `online` 是连接状态快照，与用户启用状态相互独立。
pub enum ProtocolPackageSourceViewModel {
    /// 由当前进程中的 Rhai Host 执行。
    Internal { built_in: bool },
    /// 由已注册的第三方进程通过 JSON-RPC 执行。
    External { online: bool },
}

/// 外部软件包 WebSocket 服务的启动状态。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(tag = "state", rename_all = "snake_case")]
pub enum ExternalPackageServiceStateViewModel {
    /// 服务已绑定预期地址并接受 `/packages` 连接。
    Listening,
    /// 启动绑定失败；内置协议包仍可继续使用。
    Failed { error: String },
}

/// 设置页展示的外部软件包服务运行快照。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
pub struct ExternalPackageServiceStatusViewModel {
    /// 本次进程启动时实际采用的 WebSocket 地址，不随尚未重启的设置草稿变化。
    pub websocket_url: String,
    pub fixed_path: String,
    pub online_connection_count: usize,
    pub state: ExternalPackageServiceStateViewModel,
    /// 第一版不提供认证；显式字段避免 UI 仅靠说明文字表达安全边界。
    pub authentication_enabled: bool,
}

impl ProtocolPackageSourceViewModel {
    /// 返回该精确版本是否由外部进程执行。
    #[must_use]
    pub const fn is_external(self) -> bool {
        matches!(self, Self::External { .. })
    }

    /// 返回外部连接是否在线；内置来源不依赖外部连接，固定返回 `None`。
    #[must_use]
    pub const fn external_online(self) -> Option<bool> {
        match self {
            Self::Internal { .. } => None,
            Self::External { online } => Some(online),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
/// 一个不可变协议包版本的轻量摘要。
pub struct ProtocolPackageVersionViewModel {
    pub package: ProtocolPackageRef,
    pub name: String,
    pub host_api: u32,
    /// 安装时由严格 Manifest 推断并持久化的数据平面类型。
    pub kind: ProtocolPackageKindViewModel,
    /// 该精确版本的可判别执行来源。
    #[serde(rename = "package_source")]
    pub source: ProtocolPackageSourceViewModel,
    pub enabled: bool,
    pub validation: ProtocolPackageValidationViewModel,
    pub installed_at: DateTime<Utc>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
/// 列表按协议包 ID 聚合，但每个版本仍保留自己的名称、状态和精确身份。
pub struct ProtocolPackageGroupViewModel {
    pub id: ProtocolPackageId,
    /// 最新已安装版本声明的名称，作为分组行的稳定展示名称。
    pub name: String,
    /// 同一 ID 的所有版本必须属于同一数据平面。
    pub kind: ProtocolPackageKindViewModel,
    pub versions: Vec<ProtocolPackageVersionViewModel>,
    /// 全部精确版本被已保存 Listener 引用的总次数。
    pub reference_count: usize,
    /// 其中运行态不为 Stopped 的引用次数。
    pub active_reference_count: usize,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
/// 一个已保存 Listener 对精确协议包版本的引用。
/// `runtime_state` 由 Rust 合并运行时状态；没有活动运行记录时固定为 `Stopped`。删除只看
/// 引用是否存在，停用则仅允许所有引用都已确认停止。
pub struct ProtocolPackageUsageViewModel {
    pub workspace_id: WorkspaceId,
    pub workspace_name: String,
    pub listener_id: ListenerId,
    pub listener_name: String,
    pub listener_enabled: bool,
    pub runtime_state: ListenerRuntimeState,
}

impl ProtocolPackageUsageViewModel {
    #[must_use]
    pub fn blocks_disable(&self) -> bool {
        self.runtime_state != ListenerRuntimeState::Stopped
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
/// Usage 端口一次扫描生成的精确版本计数；仅在 Application 内汇总，不直接进入 IPC。
pub struct ProtocolPackageUsageCount {
    pub package: ProtocolPackageRef,
    pub reference_count: usize,
    pub active_reference_count: usize,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
/// 精确版本详情；不包含脚本、文件列表、安装路径或编译器内部对象。
pub struct ProtocolPackageDetailViewModel {
    pub version: ProtocolPackageVersionViewModel,
    pub kind: ProtocolPackageKindViewModel,
    pub capabilities: ProtocolPackageCapabilitiesViewModel,
    pub upstream_schema: ProtocolPackageSchemaViewModel,
    pub downstream_schema: ProtocolPackageSchemaViewModel,
    pub usages: Vec<ProtocolPackageUsageViewModel>,
    /// 仅外部执行来源具有的连接、指纹和方法映射；内部包固定为 `None`。
    pub external: Option<ExternalPackageDetailViewModel>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
/// 一个方向实际调用的完整 JSON-RPC 方法名。
pub struct ExternalPackageDirectionMethodsViewModel {
    pub frame: String,
    pub decode: String,
    pub encode: String,
    pub display: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
/// 最近一次连接级错误的安全摘要；不包含第三方 payload 或未脱敏 `data`。
pub struct ExternalPackageRecentErrorViewModel {
    pub code: String,
    pub message: String,
    pub occurred_at: DateTime<Utc>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
/// 外部协议包详情的严格连接投影。
pub struct ExternalPackageDetailViewModel {
    pub remote_address: Option<String>,
    pub connection_id: Option<Uuid>,
    pub first_connected_at: DateTime<Utc>,
    pub last_connected_at: DateTime<Utc>,
    pub registration_fingerprint_sha256: String,
    pub rpc_timeout_seconds: u64,
    pub upstream_methods: ExternalPackageDirectionMethodsViewModel,
    pub downstream_methods: ExternalPackageDirectionMethodsViewModel,
    pub recent_error: Option<ExternalPackageRecentErrorViewModel>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "snake_case")]
/// Manifest 结构推导出的协议包数据平面。
pub enum ProtocolPackageKindViewModel {
    Http,
    Socket,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Type)]
/// 一个方向在 Manifest 中声明并通过编译校验的能力。
/// 编译后方向能力的只读投影。HTTP 不声明 Frame，Socket 必须声明；Decode 与 Encode
/// 均由当前严格 Manifest 提供，不再交给入口配置开关选择。
pub struct ProtocolPackageDirectionCapabilitiesViewModel {
    pub frame: bool,
    pub decode: bool,
    pub encode: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Type)]
/// 协议包两个方向与公共 Display 的完整能力投影。
pub struct ProtocolPackageCapabilitiesViewModel {
    pub upstream: ProtocolPackageDirectionCapabilitiesViewModel,
    pub downstream: ProtocolPackageDirectionCapabilitiesViewModel,
    pub display: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
/// 一个按作者声明顺序返回的 Schema 字段。
pub struct ProtocolPackageSchemaFieldViewModel {
    pub name: String,
    pub label: String,
    #[serde(rename = "type")]
    pub field_type: ProtocolPackageSchemaFieldTypeViewModel,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "snake_case")]
/// Host API v1 Schema 可向 UI 和规则目录公开的四种字段类型。
pub enum ProtocolPackageSchemaFieldTypeViewModel {
    String,
    Int,
    Bool,
    Blob,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
/// 协议包提前声明的 Document Schema；不包含 Schema 文件路径或原始 TOML。
pub struct ProtocolPackageSchemaViewModel {
    pub id: String,
    pub version: u32,
    pub title: String,
    pub fields: Vec<ProtocolPackageSchemaFieldViewModel>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
/// 基础设施从已编译包生成的安全描述，用于详情与导入结果。
pub struct ProtocolPackageDescriptionViewModel {
    /// 描述所属的精确协议包身份。Application 必须与请求身份再次比较，防止缓存串包。
    pub package: ProtocolPackageRef,
    pub kind: ProtocolPackageKindViewModel,
    pub capabilities: ProtocolPackageCapabilitiesViewModel,
    pub upstream_schema: ProtocolPackageSchemaViewModel,
    pub downstream_schema: ProtocolPackageSchemaViewModel,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
/// Listener 编辑器可以绑定的一个精确协议包版本。
///
/// 该模型只由 Rust 在读取启用状态、最近校验结果并重新取得当前编译描述后构造。前端不得
/// 根据 Host API 数字猜测兼容性，也不需要逐版本发起详情查询。
pub struct ListenerProtocolPackageOptionViewModel {
    pub package: ProtocolPackageRef,
    pub name: String,
    /// 选择器明确区分内置 Rhai 与外部进程，不从其他字段反推来源。
    #[serde(rename = "package_source")]
    pub source: ProtocolPackageSourceViewModel,
    pub kind: ProtocolPackageKindViewModel,
    pub capabilities: ProtocolPackageCapabilitiesViewModel,
    pub upstream_schema: ProtocolPackageSchemaViewModel,
    pub downstream_schema: ProtocolPackageSchemaViewModel,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
/// Listener 协议包选择器的一次权威目录快照。
///
/// `options` 只包含当前可选版本；停用、历史校验无效、无法由当前 Host 描述或返回错包
/// 描述的版本统一计入 `unavailable_version_count`，不向 `WebView` 泄漏编译器内部错误。
pub struct ListenerProtocolPackageCatalogViewModel {
    pub options: Vec<ListenerProtocolPackageOptionViewModel>,
    /// 新建按协议转发/本地应答可采用的官方精确版本。
    pub recommended_package: Option<ProtocolPackageRef>,
    pub installed_version_count: usize,
    pub unavailable_version_count: usize,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "snake_case")]
/// 原生 ZIP 导入是新安装，还是相同身份和内容的幂等复用。
pub enum ProtocolPackageImportOutcomeViewModel {
    Installed,
    Reused,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "snake_case")]
/// prepare 时针对当前注册表快照得到的权威处置结果。
/// commit 仍会在 mutation gate 与 `SQLite` 事务内重新比较，因此这个值用于决定当前预览
/// 是否可提交，而不是替代最终写入门禁。
pub enum ProtocolPackageImportDispositionViewModel {
    New,
    Reusable,
    IdentityConflict,
}

#[derive(Debug, Clone, Copy, Hash, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(transparent)]
/// 一次已验证但未安装的 pending import 随机令牌。
pub struct ProtocolPackageImportToken(Uuid);

impl ProtocolPackageImportToken {
    #[must_use]
    pub const fn from_uuid(value: Uuid) -> Self {
        Self(value)
    }

    #[must_use]
    pub const fn as_uuid(self) -> Uuid {
        self.0
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
/// ZIP 已完整校验、尚未安装时返回给确认 Dialog 的无源码预览。
pub struct ProtocolPackageImportPreviewViewModel {
    /// 冲突预览没有 token，类型层面保证它不能进入 commit。
    pub token: Option<ProtocolPackageImportToken>,
    pub disposition: ProtocolPackageImportDispositionViewModel,
    pub package: ProtocolPackageRef,
    pub name: String,
    pub host_api: u32,
    pub kind: ProtocolPackageKindViewModel,
    pub capabilities: ProtocolPackageCapabilitiesViewModel,
    pub upstream_schema: ProtocolPackageSchemaViewModel,
    pub downstream_schema: ProtocolPackageSchemaViewModel,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
/// 原生文件选择和完整校验成功后的无源码导入结果。
/// 用户取消由命令返回 `None` 表示；任何 ZIP、Manifest、Schema 或 Rhai 错误均作为
/// 稳定 `AppError` 返回，不会构造该类型。
pub struct ProtocolPackageImportViewModel {
    pub outcome: ProtocolPackageImportOutcomeViewModel,
    pub version: ProtocolPackageVersionViewModel,
    pub kind: ProtocolPackageKindViewModel,
    pub capabilities: ProtocolPackageCapabilitiesViewModel,
    pub upstream_schema: ProtocolPackageSchemaViewModel,
    pub downstream_schema: ProtocolPackageSchemaViewModel,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Type)]
/// 内置协议包 ZIP 写入用户所选文件后的结果。
pub struct ProtocolPackageExportOutcomeViewModel {
    pub bytes_written: u64,
    pub replaced_existing: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
/// Compiler port 完整重验后返回的收据。
///
/// Application 会再次比较精确身份和 Host API，防止端口误把另一个版本的成功结果用于
/// 当前启用操作。该内部模型不需要序列化给展示层。
pub struct ProtocolPackageCompilationReceipt {
    pub package: ProtocolPackageRef,
    pub host_api: u32,
    pub compatible: bool,
}
