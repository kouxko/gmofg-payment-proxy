//! 动态 Workspace Listener 的网络运行时适配器。

use std::{
    collections::{BTreeMap, BTreeSet},
    net::{IpAddr, SocketAddr},
    sync::Arc,
    time::Duration,
};

#[cfg(test)]
use std::{
    fs,
    io::{Cursor, Read},
    path::{Path, PathBuf},
};

use intercept_proxy_application::{
    AppError, AppResult, ListenerId, ListenerRuntimePort, ListenerRuntimeState,
    ListenerStatusViewModel, ListenerUpstreamTlsTestViewModel, ProxyListener, UiTone, WorkspaceId,
};
use intercept_proxy_domain::{
    CertificateReference, CertificateReferenceId, DownstreamClientAuthentication,
    FixedServerSettings, ForwardProxyAuthentication, ProxyWorkspace,
    normalize_android_network_destination,
};
use intercept_proxy_runtime::{
    ChannelId as RuntimeChannelId, DEFAULT_MAX_CONNECTIONS, ForwardAuthenticationMode,
    ForwardMitmConfig, ForwardProxyAuthenticator, ForwardProxyConfig, ForwardProxyService,
    MessageLimits, MitmCertificateAuthority, NativeRootMitmConnector, NoAuthentication,
    PipelinePorts, ReverseClientIdentity, ReverseDownstreamTls, ReverseProxyConfig,
    ReverseProxyService, ReverseUpstreamTls,
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

#[cfg(test)]
use super::common::app_error;
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
    pipeline_ports: RwLock<Option<Arc<dyn PipelinePorts>>>,
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
            pipeline_ports: RwLock::new(None),
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
        }
    }

    async fn start_fixed_server(
        &self,
        workspace: &ProxyWorkspace,
        listener: &ProxyListener,
    ) -> AppResult<(TcpListener, ReverseProxyService, String)> {
        let persisted = workspace
            .listeners
            .iter()
            .find(|candidate| candidate.id == listener.id)
            .ok_or_else(|| {
                AppError::new("LISTENER_NOT_FOUND", "Workspace 中不存在该 Listener。")
                    .entity(listener.id.to_string())
            })?;
        if persisted != listener {
            return Err(AppError::new(
                "REVISION_CONFLICT",
                "Listener 配置与当前 Workspace 快照不一致，请重新加载。",
            )
            .entity(listener.id.to_string()));
        }

        let bind_addr = parse_bind_address(&listener.bind_address, listener.port, listener.id)?;
        let fixed_server = listener.fixed_server.as_ref().ok_or_else(|| {
            AppError::new(
                "FIXED_SERVER_NOT_CONFIGURED",
                "该代理监听未配置固定 Server。",
            )
            .entity(listener.id.to_string())
        })?;
        let downstream_tls = self.downstream_tls(workspace, listener)?;
        let upstream_tls = self.upstream_tls(workspace, fixed_server)?;

        let mut service = ReverseProxyService::build(ReverseProxyConfig {
            bind_addr,
            allowed_client_cidrs: listener.allowed_client_cidrs.clone(),
            upstream_origin: fixed_server.upstream_url.clone(),
            downstream_tls,
            upstream_tls,
            connect_timeout: Duration::from_millis(listener.connect_timeout_ms),
            read_timeout: Duration::from_millis(listener.read_timeout_ms),
            write_timeout: Duration::from_millis(listener.write_timeout_ms),
        })
        .await
        .map_err(|error| {
            AppError::new(error.code, error.message).entity(listener.id.to_string())
        })?;
        let pipeline_ports = self.pipeline_ports.read().clone().ok_or_else(|| {
            AppError::new(
                "LISTENER_RUNTIME_NOT_READY",
                "通用规则、抓包与断点管线尚未完成装配。",
            )
            .entity(listener.id.to_string())
        })?;
        let channel = RuntimeChannelId::new(listener.id.to_string())
            .map_err(|error| AppError::new(error.code, error.message))?;
        service = service
            .with_pipeline(
                channel,
                pipeline_ports,
                MessageLimits::default(),
                DEFAULT_MAX_CONNECTIONS,
            )
            .map_err(|error| {
                AppError::new(error.code, error.message).entity(listener.id.to_string())
            })?;
        let tcp_listener = bind_tcp_listener(bind_addr, listener.id).await?;
        Ok((tcp_listener, service, bind_addr.to_string()))
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

mod helpers;
mod port;
mod tls_material;

use helpers::{bind_tcp_listener, parse_bind_address, running_status, upstream_tls_test_error};
#[cfg(test)]
use tls_material::normalize_sni_pattern;

#[cfg(test)]
mod test_support;
#[cfg(test)]
use test_support::*;

#[cfg(test)]
#[path = "listener_runtime/tests.rs"]
mod tests;
