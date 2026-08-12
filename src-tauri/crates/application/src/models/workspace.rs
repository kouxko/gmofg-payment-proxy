use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};
use specta::Type;

use super::{ListenerId, ProxyWorkspace, Revision, UiTone, WorkspaceId};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
pub struct OperationResultViewModel {
    pub success: bool,
    pub cancelled: bool,
    pub message: String,
    pub ui_tone: UiTone,
    pub entity_id: Option<String>,
    pub revision: Option<Revision>,
    pub requires_restart: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
/// Workspace 列表只返回轻量摘要，完整配置仅在用户选择或编辑时加载。
pub struct WorkspaceSummaryViewModel {
    pub id: WorkspaceId,
    pub name: String,
    pub revision: Revision,
    pub listener_count: usize,
    pub enabled_listener_count: usize,
    pub selected: bool,
}

impl WorkspaceSummaryViewModel {
    #[must_use]
    pub fn from_workspace(workspace: &ProxyWorkspace, selected: bool) -> Self {
        Self {
            id: workspace.id,
            name: workspace.name.clone(),
            revision: workspace.revision.get(),
            listener_count: workspace.listeners.len(),
            enabled_listener_count: workspace
                .listeners
                .iter()
                .filter(|listener| listener.enabled)
                .count(),
            selected,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
/// Rust Workspace 校验的完整结果。前端不得重复推导安全策略。
pub struct WorkspaceValidationViewModel {
    pub valid: bool,
    pub normalized: ProxyWorkspace,
    pub field_errors: BTreeMap<String, Vec<String>>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "snake_case")]
pub enum WorkspaceChangeKind {
    Created,
    Updated,
    Selected,
    Deleted,
    Imported,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
/// Workspace 集合变化事件；删除时 `summary` 为空，其余操作携带最新 Rust 摘要。
pub struct WorkspaceChangedViewModel {
    pub workspace_id: WorkspaceId,
    pub kind: WorkspaceChangeKind,
    pub summary: Option<WorkspaceSummaryViewModel>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "snake_case")]
pub enum ListenerRuntimeState {
    Stopped,
    Starting,
    Running,
    Stopping,
    Faulted,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
/// 单个 Workspace Listener 的运行快照。所有文案与状态均由 Rust 提供。
pub struct ListenerStatusViewModel {
    pub listener_id: ListenerId,
    pub state: ListenerRuntimeState,
    pub state_text: String,
    pub ui_tone: UiTone,
    pub listen_address: String,
    pub fault_reason: Option<String>,
    pub can_start: bool,
    pub can_stop: bool,
    pub active_connections: u32,
    pub client_to_server_bytes: u64,
    pub server_to_client_bytes: u64,
    pub retained_diagnostic_evictions: u64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "snake_case")]
pub enum ListenerDataPlaneKind {
    Http,
    Socket,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "snake_case")]
pub enum SocketTransportMode {
    Transparent,
    TcpToTls,
    TlsToTcp,
    TlsToTls,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
/// 对单条已启用 HTTPS 固定 Server 的代理监听执行真实 TCP + TLS 握手后的只读结果。
/// 该模型只包含公开的对端证书元数据，不返回证书字节、客户端私钥或安全引用内容。
/// `client_identity_configured` 只表示本次握手加载了客户端身份；Server 是否强制要求
/// 客户端证书，由握手成功或失败共同判断，前端不能自行推断。
pub struct ListenerUpstreamTlsTestViewModel {
    pub listener_id: ListenerId,
    pub upstream_origin: String,
    pub resolved_address: String,
    pub tls_version: String,
    pub cipher_suite: String,
    pub peer_subject: String,
    pub peer_sha256_fingerprint: String,
    pub hostname_verification_enabled: bool,
    pub client_identity_configured: bool,
    pub elapsed_millis: u64,
    pub message: String,
    pub ui_tone: UiTone,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
/// 对固定 Server 执行真实连接探测。HTTP 返回 TCP 证据；HTTPS 额外返回 TLS 证据。
pub struct ListenerUpstreamConnectionTestViewModel {
    pub listener_id: ListenerId,
    pub data_plane: ListenerDataPlaneKind,
    pub upstream_origin: String,
    pub resolved_address: String,
    pub scheme: String,
    pub transport: String,
    pub tls: Option<ListenerUpstreamTlsEvidenceViewModel>,
    pub socket_transport_mode: Option<SocketTransportMode>,
    pub elapsed_millis: u64,
    pub message: String,
    pub ui_tone: UiTone,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
pub struct ListenerUpstreamTlsEvidenceViewModel {
    pub tls_version: String,
    pub cipher_suite: String,
    pub peer_subject: String,
    pub peer_sha256_fingerprint: String,
    pub hostname_verification_enabled: bool,
    pub client_identity_configured: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
/// 运行监控中的单个入口行。
/// 配置与运行状态由 Rust 合并，前端不推断“缺少状态即停止”。
pub struct ListenerMonitorRowViewModel {
    pub listener_id: ListenerId,
    pub name: String,
    pub kind_text: String,
    pub listen_address: String,
    pub request_destination: String,
    pub state: ListenerRuntimeState,
    pub state_text: String,
    pub ui_tone: UiTone,
    pub fault_reason: Option<String>,
    /// Rust runtime 是否允许当前 Listener 执行启动。
    pub can_start: bool,
    /// Rust runtime 是否允许当前 Listener 执行停止。
    /// `Faulted` 仍可能为 `true`，用于释放 runtime ownership。
    pub can_stop: bool,
    pub active_connections: u32,
    pub client_to_server_bytes: u64,
    pub server_to_client_bytes: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
/// 当前 Workspace 的入口运行概览，供顶部状态栏与运行监控复用。
pub struct ListenerOverviewViewModel {
    pub workspace_id: WorkspaceId,
    pub workspace_name: String,
    pub state_text: String,
    pub ui_tone: UiTone,
    pub total_count: usize,
    pub active_count: usize,
    pub faulted_count: usize,
    pub rows: Vec<ListenerMonitorRowViewModel>,
}

impl WorkspaceValidationViewModel {
    #[must_use]
    pub fn validate(workspace: ProxyWorkspace) -> Self {
        match workspace.validate() {
            Ok(()) => Self {
                valid: true,
                normalized: workspace,
                field_errors: BTreeMap::new(),
            },
            Err(error) => Self {
                valid: false,
                normalized: workspace,
                field_errors: *error.field_errors,
            },
        }
    }
}

impl OperationResultViewModel {
    pub fn success(message: impl Into<String>) -> Self {
        Self {
            success: true,
            cancelled: false,
            message: message.into(),
            ui_tone: UiTone::Positive,
            entity_id: None,
            revision: None,
            requires_restart: false,
        }
    }
}

#[cfg(test)]
mod workspace_tests {
    use super::*;

    #[test]
    fn workspace_summary_is_computed_by_rust() {
        let workspace = ProxyWorkspace::default();
        let summary = WorkspaceSummaryViewModel::from_workspace(&workspace, true);
        assert_eq!(summary.id, workspace.id);
        assert_eq!(summary.listener_count, 1);
        assert_eq!(summary.enabled_listener_count, 0);
        assert!(summary.selected);
    }

    #[test]
    fn workspace_validation_returns_rust_field_errors() {
        let mut workspace = ProxyWorkspace::default();
        workspace.name.clear();
        let validation = WorkspaceValidationViewModel::validate(workspace);
        assert!(!validation.valid);
        assert!(validation.field_errors.contains_key("name"));
    }
}
