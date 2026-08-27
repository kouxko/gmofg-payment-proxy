use chrono::{DateTime, Utc};
use intercept_proxy_domain::{ProtocolDirection, ProtocolPackageRef};
use serde::{Deserialize, Serialize};
use specta::Type;

use super::{SocketConnectionRouteViewModel, SocketFailureDiagnostic, UiTone};
use sanitization::{sanitize_optional, sanitize_text};

mod sanitization;

pub const DIAGNOSTIC_SUMMARY_MAX_CHARS: usize = 240;
pub const DIAGNOSTIC_DETAIL_MAX_CHARS: usize = 2_048;
const DIAGNOSTIC_CONTEXT_MAX_CHARS: usize = 160;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "snake_case")]
/// 跨桌面、ADB、Companion 与代理链路共用的诊断阶段。
pub enum DiagnosticLogStage {
    System,
    AdbForwardControl,
    AdbReverseBusiness,
    DesktopDns,
    Companion,
    Vpn,
    Tun,
    AppSelection,
    RouteActivation,
    Listener,
    DownstreamTls,
    UpstreamTls,
    Http,
    Socket,
    StopFallback,
    Cleanup,
}

impl DiagnosticLogStage {
    #[must_use]
    pub const fn display_zh(self) -> &'static str {
        match self {
            Self::System => "系统",
            Self::AdbForwardControl => "ADB 控制通道",
            Self::AdbReverseBusiness => "ADB 业务映射",
            Self::DesktopDns => "桌面 DNS",
            Self::Companion => "设备端组件",
            Self::Vpn => "VPN",
            Self::Tun => "TUN 数据面",
            Self::AppSelection => "目标应用",
            Self::RouteActivation => "透明代理路由",
            Self::Listener => "代理入口",
            Self::DownstreamTls => "客户端 TLS",
            Self::UpstreamTls => "上游 TLS",
            Self::Http => "HTTP",
            Self::Socket => "Socket",
            Self::StopFallback => "停止回退",
            Self::Cleanup => "资源清理",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "snake_case")]
pub enum DiagnosticLogLevel {
    Info,
    Warning,
    Error,
}

impl DiagnosticLogLevel {
    #[must_use]
    pub const fn display_zh(self) -> &'static str {
        match self {
            Self::Info => "信息",
            Self::Warning => "警告",
            Self::Error => "失败",
        }
    }

    #[must_use]
    pub const fn tone(self) -> UiTone {
        match self {
            Self::Info => UiTone::Info,
            Self::Warning => UiTone::Warning,
            Self::Error => UiTone::Danger,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
/// 生产者提交的有界控制面诊断。完整报文由 capture/Exchange observation 负责；密码、私钥和
/// PKCS12 字节只属于专用凭据边界。
pub struct DiagnosticLogEntryViewModel {
    pub level: DiagnosticLogLevel,
    pub stage: DiagnosticLogStage,
    pub summary: String,
    pub detail: Option<String>,
    pub device_serial: Option<String>,
    pub listener_id: Option<String>,
    pub profile_id: Option<String>,
    pub socket_context: Option<SocketDiagnosticContextViewModel>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
pub struct SocketDiagnosticContextViewModel {
    pub connection_id: Option<String>,
    pub workspace_runtime_epoch: String,
    pub listener_run_epoch: String,
    pub route: Option<SocketConnectionRouteViewModel>,
    pub socket_failure: Option<SocketFailureDiagnostic>,
    pub external_package_call: Option<ExternalPackageCallDiagnosticViewModel>,
    pub stage: SocketDiagnosticStage,
    pub direction: Option<SocketDiagnosticDirection>,
    pub client_to_server_read_bytes: u64,
    pub client_to_server_bytes: u64,
    pub server_to_client_read_bytes: u64,
    pub server_to_client_bytes: u64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "snake_case")]
pub enum ExternalPackageCallStage {
    Frame,
    Decode,
    Encode,
    Display,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
/// 外部 JSON-RPC 调用的可查询控制面诊断；业务报文由 Exchange observation 保存，远端 `data`
/// 在此仅提供有界形状，避免同一 payload 在两个生命周期不同的 store 中重复分配。
pub struct ExternalPackageCallDiagnosticViewModel {
    pub package: ProtocolPackageRef,
    pub direction: ProtocolDirection,
    pub stage: ExternalPackageCallStage,
    pub method: String,
    pub request_id: Option<String>,
    pub remote_code: Option<i64>,
    pub remote_message: Option<String>,
    /// 只允许 `null/bool/number/string(bytes=N)/array(items=N)/object(fields=N)` 形状摘要。
    pub remote_data_summary: Option<String>,
}

impl ExternalPackageCallDiagnosticViewModel {
    #[must_use]
    pub fn sanitized(mut self) -> Self {
        self.method = sanitize_text(&self.method, DIAGNOSTIC_CONTEXT_MAX_CHARS);
        self.request_id =
            sanitize_optional(self.request_id.as_deref(), DIAGNOSTIC_CONTEXT_MAX_CHARS);
        self.remote_message =
            sanitize_optional(self.remote_message.as_deref(), DIAGNOSTIC_SUMMARY_MAX_CHARS);
        self.remote_data_summary = sanitize_optional(
            self.remote_data_summary.as_deref(),
            DIAGNOSTIC_CONTEXT_MAX_CHARS,
        );
        self
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "snake_case")]
pub enum SocketDiagnosticStage {
    Admission,
    DownstreamTls,
    Dns,
    Connect,
    UpstreamTls,
    RelayRead,
    FrameInspect,
    Decode,
    Rule,
    Encode,
    FrameProcess,
    RelayWrite,
    Shutdown,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "snake_case")]
pub enum SocketDiagnosticDirection {
    Downstream,
    Upstream,
    ClientToServer,
    ServerToClient,
    LocalExchange,
}

impl DiagnosticLogEntryViewModel {
    /// 诊断 DTO 离开应用层前统一执行控制面文本长度限制。
    ///
    /// 生产者可以提交便于定位的上下文，但所有 UI、Channel 与查询结果都使用同一规范化副本；
    /// 完整 HTTP/Socket/Document 证据通过专用观测 store 查询，不在诊断 `EventHub` 中复制。
    #[must_use]
    pub fn sanitized(self) -> Self {
        Self {
            level: self.level,
            stage: self.stage,
            summary: sanitize_text(&self.summary, DIAGNOSTIC_SUMMARY_MAX_CHARS),
            detail: sanitize_optional(self.detail.as_deref(), DIAGNOSTIC_DETAIL_MAX_CHARS),
            device_serial: sanitize_optional(
                self.device_serial.as_deref(),
                DIAGNOSTIC_CONTEXT_MAX_CHARS,
            ),
            listener_id: sanitize_optional(
                self.listener_id.as_deref(),
                DIAGNOSTIC_CONTEXT_MAX_CHARS,
            ),
            profile_id: sanitize_optional(self.profile_id.as_deref(), DIAGNOSTIC_CONTEXT_MAX_CHARS),
            socket_context: self.socket_context.map(|mut context| {
                context.external_package_call = context
                    .external_package_call
                    .map(ExternalPackageCallDiagnosticViewModel::sanitized);
                context
            }),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
pub struct DiagnosticLogRowViewModel {
    pub event_id: u64,
    pub occurred_at: DateTime<Utc>,
    pub level: DiagnosticLogLevel,
    pub level_text: String,
    pub stage: DiagnosticLogStage,
    pub stage_text: String,
    pub summary: String,
    pub detail: Option<String>,
    pub device_serial: Option<String>,
    pub listener_id: Option<String>,
    pub profile_id: Option<String>,
    pub socket_context: Option<SocketDiagnosticContextViewModel>,
    pub ui_tone: UiTone,
}

impl DiagnosticLogRowViewModel {
    #[must_use]
    pub fn from_entry(
        event_id: u64,
        occurred_at: DateTime<Utc>,
        entry: &DiagnosticLogEntryViewModel,
    ) -> Self {
        let entry = entry.clone().sanitized();
        Self {
            event_id,
            occurred_at,
            level: entry.level,
            level_text: entry.level.display_zh().to_owned(),
            stage: entry.stage,
            stage_text: entry.stage.display_zh().to_owned(),
            summary: entry.summary,
            detail: entry.detail,
            device_serial: entry.device_serial,
            listener_id: entry.listener_id,
            profile_id: entry.profile_id,
            socket_context: entry.socket_context,
            ui_tone: entry.level.tone(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
pub struct DiagnosticLogQuery {
    pub keyword: Option<String>,
    pub after_event_id: Option<u64>,
    pub limit: u16,
}

impl Default for DiagnosticLogQuery {
    fn default() -> Self {
        Self {
            keyword: None,
            after_event_id: None,
            limit: 300,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
pub struct DiagnosticLogPageViewModel {
    pub rows: Vec<DiagnosticLogRowViewModel>,
    pub current_cursor: u64,
    /// `EventHub` 当前仍保留的最早全局事件；诊断行是该有界历史的类型化投影。
    pub oldest_retained_event_id: Option<u64>,
    /// `after_event_id` 早于保留窗口时为 true，调用方必须重新读取完整诊断快照。
    pub snapshot_required: bool,
    pub retained_count: usize,
    pub truncated: bool,
    pub empty_message: String,
}
