//! 可复制、可序列化的只读故障复现报告模型。
//!
//! 报告保留精确 Workspace/Listener 配置与有限条数的运行证据。各观测端口失败不会
//! 丢弃已经取得的现场信息，而是进入 `collection_errors`，供 MCP、CLI 或桌面导出层
//! 明确说明缺失原因。

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use specta::Type;
use uuid::Uuid;

use crate::{
    AndroidNetworkStatusViewModel, AndroidRuntimeEndpointViewModel, AndroidRuntimeOwnerViewModel,
};

use super::{
    DiagnosticLogRowViewModel, ExternalPackageServiceStatusViewModel, ListenerId,
    ListenerStatusViewModel, ProtocolPackageDetailViewModel, ProxyListener, ProxyWorkspace,
    RuleDefinition, SettingsViewModel, WorkspaceId,
};

/// 单份报告最多包含的入口诊断行数。
pub const DIAGNOSTIC_REPORT_MAX_DIAGNOSTICS: usize = 100;
/// Markdown 投影的字符上限；结构化 bundle 仍保留有界的完整字段。
pub const DIAGNOSTIC_REPORT_MARKDOWN_MAX_CHARS: usize = 64 * 1024;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(deny_unknown_fields)]
/// 生成故障复现报告所需的精确范围。
pub struct DiagnosticReportQuery {
    pub workspace_id: WorkspaceId,
    pub listener_id: ListenerId,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "snake_case")]
/// 报告采集失败所属的稳定分区。
pub enum DiagnosticReportSection {
    RuntimeStatus,
    Settings,
    ProtocolPackageDetail,
    ExternalPackageService,
    AndroidNetworkStatus,
    AndroidRuntimeOwner,
    AndroidRuntimeEndpoints,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
pub struct DiagnosticReportCollectionError {
    pub section: DiagnosticReportSection,
    pub code: String,
    pub message: String,
    pub entity_id: Option<String>,
    pub runtime_epoch: Option<Uuid>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
pub struct DiagnosticReportEnvironment {
    pub product_name: String,
    pub application_version: String,
    pub operating_system: String,
    pub architecture: String,
    /// 稳定的仓库相对路径，帮助诊断方快速定位聚合边界和运行数据平面。
    pub architecture_refs: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Type)]
pub struct DiagnosticReportBundle {
    pub generated_at: DateTime<Utc>,
    pub workspace: ProxyWorkspace,
    pub listener: ProxyListener,
    pub runtime_status: Option<ListenerStatusViewModel>,
    pub settings: Option<SettingsViewModel>,
    pub rule_definitions: Vec<RuleDefinition>,
    pub protocol_package_detail: Option<ProtocolPackageDetailViewModel>,
    pub external_package_service: Option<ExternalPackageServiceStatusViewModel>,
    pub diagnostics: Vec<DiagnosticLogRowViewModel>,
    pub android_network_statuses: Vec<AndroidNetworkStatusViewModel>,
    pub android_runtime_owners: Vec<AndroidRuntimeOwnerViewModel>,
    pub android_runtime_endpoints: Vec<AndroidRuntimeEndpointViewModel>,
    pub environment: DiagnosticReportEnvironment,
    pub reproduction_steps: Vec<String>,
    pub collection_errors: Vec<DiagnosticReportCollectionError>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Type)]
pub struct DiagnosticReportViewModel {
    pub bundle: DiagnosticReportBundle,
    /// 可直接复制或保存为 `.md` 的有界投影。
    pub markdown: String,
}
