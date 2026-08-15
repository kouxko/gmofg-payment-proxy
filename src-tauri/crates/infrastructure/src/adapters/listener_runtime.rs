//! 动态 Workspace Listener 的网络运行时适配器。

use std::{
    collections::{BTreeMap, BTreeSet},
    net::{IpAddr, SocketAddr},
    sync::Arc,
};

#[cfg(test)]
use std::{
    fs,
    io::{Cursor, Read},
    path::{Path, PathBuf},
    time::Duration,
};

use intercept_proxy_application::{
    AppError, AppResult, EventHub, ListenerId, ListenerRuntimeState, ListenerStatusViewModel,
    ProxyListener, UiTone, WorkspaceId,
};
use intercept_proxy_domain::{
    CertificateReference, CertificateReferenceId, DownstreamClientAuthentication,
    FixedServerSettings, ProxyWorkspace, normalize_android_network_destination,
};
use intercept_proxy_runtime::{
    MitmCertificateAuthority, PipelinePorts, ReverseClientIdentity, ReverseDownstreamTls,
    ReverseUpstreamTls, SocketRelayService,
};
use parking_lot::RwLock;
use tokio::{net::TcpListener, task::JoinHandle};
use tokio_util::sync::CancellationToken;
use uuid::Uuid;
#[cfg(test)]
use zeroize::Zeroizing;

#[cfg(test)]
use crate::CertificateService;
use crate::SqliteStore;

use super::ProtocolPackageRepositoryAdapter;
#[cfg(test)]
use super::common::{app_error, encode_workspace_record};
use super::{ManagedListenerCertificateAdapter, ProtectedSecretAdapter};

/// 读取当前安装实例在证书管理页签发的服务端叶子证书。
///
/// 该端口只在 infrastructure 内部流转 TLS 私钥，Workspace 与 IPC 只用 `None` 表示
/// “使用本机叶子证书”，不会接触或复制私钥字节。
pub(crate) trait InstallationServerIdentityProvider: std::fmt::Debug + Send + Sync {
    fn load_installation_server_identity(&self) -> AppResult<ReverseClientIdentity>;
}

#[derive(Debug)]
struct RunningListener {
    cancellation: CancellationToken,
    task: JoinHandle<()>,
    listen_address: String,
    fault: Arc<RwLock<Option<String>>>,
    /// Immutable configuration identity used by this task. Keeping the snapshot makes runtime
    /// ownership explicit and prevents later Workspace edits from silently changing live traffic.
    workspace: ProxyWorkspace,
    socket_service: Option<Arc<SocketRelayService>>,
}

#[derive(Debug)]
pub struct ListenerRuntimeAdapter {
    running: tokio::sync::Mutex<BTreeMap<ListenerId, RunningListener>>,
    /// Each Workspace owns an independent rules/session generation.
    runtime_epochs: RwLock<BTreeMap<WorkspaceId, Uuid>>,
    _store: Arc<SqliteStore>,
    mitm_certificate_authority: Option<Arc<dyn MitmCertificateAuthority>>,
    installation_server_identity: Option<Arc<dyn InstallationServerIdentityProvider>>,
    protected_secrets: Option<Arc<ProtectedSecretAdapter>>,
    managed_listener_certificates: Option<Arc<ManagedListenerCertificateAdapter>>,
    protocol_packages: Option<Arc<ProtocolPackageRepositoryAdapter>>,
    pipeline_ports: RwLock<Option<Arc<dyn PipelinePorts>>>,
    socket_diagnostic_events: RwLock<Arc<EventHub>>,
}

impl ListenerRuntimeAdapter {
    #[must_use]
    pub fn new(store: Arc<SqliteStore>) -> Self {
        Self {
            running: tokio::sync::Mutex::new(BTreeMap::new()),
            runtime_epochs: RwLock::new(BTreeMap::new()),
            _store: store,
            mitm_certificate_authority: None,
            installation_server_identity: None,
            protected_secrets: None,
            managed_listener_certificates: None,
            protocol_packages: None,
            pipeline_ports: RwLock::new(None),
            socket_diagnostic_events: RwLock::new(Arc::new(EventHub::default())),
        }
    }

    #[must_use]
    pub fn with_managed_listener_certificates(
        mut self,
        certificates: Arc<ManagedListenerCertificateAdapter>,
    ) -> Self {
        self.managed_listener_certificates = Some(certificates);
        self
    }

    /// 注入协议包注册表，仅 Scripted Socket 启动计划会访问；Direct 分支不会触碰它。
    #[must_use]
    pub fn with_protocol_packages(
        mut self,
        protocol_packages: Arc<ProtocolPackageRepositoryAdapter>,
    ) -> Self {
        self.protocol_packages = Some(protocol_packages);
        self
    }

    #[must_use]
    pub(crate) fn with_installation_server_identity(
        mut self,
        provider: Arc<dyn InstallationServerIdentityProvider>,
    ) -> Self {
        self.installation_server_identity = Some(provider);
        self
    }

    #[must_use]
    pub fn with_protected_secrets(
        mut self,
        protected_secrets: Arc<ProtectedSecretAdapter>,
    ) -> Self {
        self.protected_secrets = Some(protected_secrets);
        self
    }

    /// 注入安装级 MITM Root CA 签发能力。
    ///
    /// 单元测试和只使用透明隧道的宿主可以不注入；只有 Workspace 明确启用 MITM 时
    /// 才会校验并使用此依赖，避免普通 CONNECT 启动时触碰证书私钥。
    #[must_use]
    pub fn with_mitm_certificate_authority(
        mut self,
        authority: Arc<dyn MitmCertificateAuthority>,
    ) -> Self {
        self.mitm_certificate_authority = Some(authority);
        self
    }

    /// 由 Host 在通用规则、会话和断点服务完成装配后注入共享管线。
    ///
    /// `InfrastructureServiceBundle` 创建时这些服务尚未全部存在，因此使用一次性显式
    /// setter；运行中的 Listener 会克隆不可变 `Arc`，不会在连接处理中热换实现。
    pub fn set_pipeline_ports(&self, ports: Arc<dyn PipelinePorts>) {
        *self.pipeline_ports.write() = Some(ports);
    }

    pub fn set_socket_diagnostic_events(&self, events: Arc<EventHub>) {
        *self.socket_diagnostic_events.write() = events;
    }

    fn runtime_epoch_for_start(&self, workspace_id: WorkspaceId) -> Uuid {
        let mut epochs = self.runtime_epochs.write();
        *epochs.entry(workspace_id).or_insert_with(Uuid::new_v4)
    }

    fn stopped(listener_id: ListenerId, listen_address: String) -> ListenerStatusViewModel {
        ListenerStatusViewModel {
            listener_id,
            state: ListenerRuntimeState::Stopped,
            state_text: "已停止".into(),
            ui_tone: UiTone::Neutral,
            listen_address,
            fault_reason: None,
            can_start: true,
            can_stop: false,
            active_connections: 0,
            client_to_server_bytes: 0,
            server_to_client_bytes: 0,
            retained_diagnostic_evictions: 0,
        }
    }
}

impl Drop for ListenerRuntimeAdapter {
    fn drop(&mut self) {
        // Host 正常关闭应逐个调用 stop；这里仍提供最后一道同步兜底，避免展示适配器或
        // 测试直接丢弃 Application 时 Tokio 任务继续占用端口。
        for handle in self.running.get_mut().values() {
            handle.cancellation.cancel();
            handle.task.abort();
        }
    }
}

mod document_rules;
mod helpers;
mod plan;
mod port;
mod scripted_relay;
mod scripted_snapshot;
mod socket_diagnostics;
mod socket_plan;
mod tls_material;

pub use document_rules::{
    BoundSocketDocument, SocketDocumentRuleConnection, SocketDocumentRuleConnectionFactory,
};
use helpers::{bind_tcp_listener, parse_bind_address, running_status, upstream_tls_test_error};
use plan::{ListenerRuntimePlanBuilder, PreparedListenerRuntime};
#[cfg(test)]
use tls_material::normalize_sni_pattern;

#[cfg(test)]
mod test_support;
#[cfg(test)]
use test_support::*;

#[cfg(test)]
#[path = "listener_runtime/tests.rs"]
mod tests;
