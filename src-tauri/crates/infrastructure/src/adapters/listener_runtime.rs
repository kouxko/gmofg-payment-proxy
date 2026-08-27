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
#[cfg(test)]
#[async_trait::async_trait]
pub(crate) trait InstallationServerIdentityProvider: std::fmt::Debug + Send + Sync {
    async fn load_installation_server_identity(&self) -> AppResult<ReverseClientIdentity>;
}

#[async_trait::async_trait]
pub(crate) trait ListenerMitmAuthorityProvider: std::fmt::Debug + Send + Sync {
    async fn freeze_installation_tls_material(&self) -> AppResult<InstallationTlsMaterial>;
}

pub(crate) struct InstallationTlsMaterial {
    pub(crate) server_identity: ReverseClientIdentity,
    pub(crate) dynamic_authority: Arc<dyn MitmCertificateAuthority>,
}

#[derive(Debug)]
struct RunningListener {
    /// Unique identity of this exact start operation. Unlike the Workspace epoch, this token
    /// changes on every stop/start cycle, even while another Listener keeps the Workspace alive.
    run_token: Uuid,
    runtime_epoch: Uuid,
    cancellation: CancellationToken,
    task: JoinHandle<()>,
    listen_address: String,
    fault: Arc<RwLock<Option<String>>>,
    /// Immutable configuration identity used by this task. Keeping the snapshot makes runtime
    /// ownership explicit and prevents later Workspace edits from silently changing live traffic.
    workspace: ProxyWorkspace,
    socket_service: Option<Arc<SocketRelayService>>,
    scripted_snapshot: Option<Arc<scripted_snapshot::ScriptedSocketRuntimeSnapshot>>,
    external_socket_snapshot: Option<Arc<external_relay::ExternalSocketRuntimeSnapshot>>,
    http_protocol_snapshot: Option<Arc<http_protocol_pipeline::HttpProtocolRuntimeSnapshot>>,
}

#[derive(Clone, Copy, Debug)]
struct PendingListenerStart {
    workspace_id: WorkspaceId,
    runtime_epoch: Uuid,
}

#[derive(Clone, Copy, Debug)]
struct StoppingListener {
    runtime_epoch: Uuid,
}

#[derive(Clone, Debug)]
struct RuntimePipelineServices {
    ports: Arc<dyn PipelinePorts>,
}

#[derive(Clone, Debug)]
pub struct ListenerRuntimeAdapter {
    environment_apply_resource_gates: Arc<super::EnvironmentApplyResourceGateRegistry>,
    running: Arc<tokio::sync::Mutex<BTreeMap<ListenerId, RunningListener>>>,
    pending_starts: Arc<RwLock<BTreeMap<ListenerId, PendingListenerStart>>>,
    stopping: Arc<RwLock<BTreeMap<Uuid, StoppingListener>>>,
    /// Each Workspace owns an independent rules/session generation.
    runtime_epochs: Arc<RwLock<BTreeMap<WorkspaceId, Uuid>>>,
    _store: Arc<SqliteStore>,
    mitm_certificate_authority: Option<Arc<dyn ListenerMitmAuthorityProvider>>,
    protected_secrets: Arc<ProtectedSecretAdapter>,
    managed_listener_certificates: Option<Arc<ManagedListenerCertificateAdapter>>,
    protocol_packages: Arc<ProtocolPackageRepositoryAdapter>,
    document_rule_compiler: DocumentRuleCompiler,
    external_package_provider:
        Arc<RwLock<Option<Arc<dyn external_relay::ExternalSocketPackageProvider>>>>,
    pipeline_services: Arc<RwLock<Option<RuntimePipelineServices>>>,
    body_codec_resolver: Arc<RwLock<Option<Arc<super::WorkspaceBodyCodecResolver>>>>,
    socket_diagnostic_events: Arc<RwLock<Arc<EventHub>>>,
    #[cfg(test)]
    stop_barriers: Arc<tokio::sync::Mutex<BTreeMap<Uuid, StopBarrier>>>,
    #[cfg(test)]
    start_barriers: Arc<tokio::sync::Mutex<BTreeMap<ListenerId, StopBarrier>>>,
    #[cfg(test)]
    activation_barriers: Arc<tokio::sync::Mutex<BTreeMap<ListenerId, StopBarrier>>>,
}

#[cfg(test)]
#[derive(Clone, Debug)]
struct StopBarrier {
    reached: Arc<tokio::sync::Notify>,
    release: Arc<tokio::sync::Notify>,
    completed: Arc<tokio::sync::Notify>,
}

impl ListenerRuntimeAdapter {
    #[must_use]
    pub fn new(
        store: Arc<SqliteStore>,
        protected_secrets: Arc<ProtectedSecretAdapter>,
        protocol_packages: Arc<ProtocolPackageRepositoryAdapter>,
    ) -> Self {
        Self {
            environment_apply_resource_gates: Arc::new(
                super::EnvironmentApplyResourceGateRegistry::default(),
            ),
            running: Arc::new(tokio::sync::Mutex::new(BTreeMap::new())),
            pending_starts: Arc::new(RwLock::new(BTreeMap::new())),
            stopping: Arc::new(RwLock::new(BTreeMap::new())),
            runtime_epochs: Arc::new(RwLock::new(BTreeMap::new())),
            _store: store,
            mitm_certificate_authority: None,
            protected_secrets,
            managed_listener_certificates: None,
            protocol_packages,
            document_rule_compiler: DocumentRuleCompiler::new(4),
            external_package_provider: Arc::new(RwLock::new(None)),
            pipeline_services: Arc::new(RwLock::new(None)),
            body_codec_resolver: Arc::new(RwLock::new(None)),
            socket_diagnostic_events: Arc::new(RwLock::new(Arc::new(EventHub::default()))),
            #[cfg(test)]
            stop_barriers: Arc::new(tokio::sync::Mutex::new(BTreeMap::new())),
            #[cfg(test)]
            start_barriers: Arc::new(tokio::sync::Mutex::new(BTreeMap::new())),
            #[cfg(test)]
            activation_barriers: Arc::new(tokio::sync::Mutex::new(BTreeMap::new())),
        }
    }

    pub(crate) fn with_environment_apply_resource_gates(
        mut self,
        gates: Arc<super::EnvironmentApplyResourceGateRegistry>,
    ) -> Self {
        self.environment_apply_resource_gates = gates;
        self
    }

    pub(super) async fn compile_document_rules_on_blocking_owner<T, F>(
        &self,
        compile: F,
    ) -> AppResult<T>
    where
        T: Send + 'static,
        F: FnOnce() -> AppResult<T> + Send + 'static,
    {
        self.document_rule_compiler.compile(compile).await
    }

    /// 注入外部协议包在线注册表。
    ///
    /// Provider 只在 Listener start 时解析一次精确版本；已运行连接持有冻结的注册合同和 actor
    /// 句柄，不会因目录刷新而在一帧中途切换实现。
    pub(crate) fn set_external_package_provider(
        &self,
        provider: Arc<dyn external_relay::ExternalSocketPackageProvider>,
    ) {
        *self.external_package_provider.write() = Some(provider);
    }

    #[must_use]
    pub fn with_managed_listener_certificates(
        mut self,
        certificates: Arc<ManagedListenerCertificateAdapter>,
    ) -> Self {
        self.managed_listener_certificates = Some(certificates);
        self
    }

    /// 注入安装级 MITM Root CA 签发能力。
    ///
    /// 单元测试和只使用透明隧道的宿主可以不注入；只有 Workspace 明确启用 MITM 时
    /// 才会校验并使用此依赖，避免普通 CONNECT 启动时触碰证书私钥。
    #[must_use]
    pub fn with_mitm_certificate_authority(
        mut self,
        authority: Arc<dyn ListenerMitmAuthorityProvider>,
    ) -> Self {
        self.mitm_certificate_authority = Some(authority);
        self
    }

    /// 由 Host 在通用规则、会话和断点服务完成装配后注入共享管线。
    ///
    /// `InfrastructureServiceBundle` 创建时这些服务尚未全部存在，因此使用一次性显式
    /// setter；运行中的 Listener 会克隆不可变 `Arc`，不会在连接处理中热换实现。
    pub fn set_pipeline_ports<T>(&self, ports: Arc<T>)
    where
        T: PipelinePorts + 'static,
    {
        *self.pipeline_services.write() = Some(RuntimePipelineServices { ports });
    }

    pub(crate) fn set_body_codec_resolver(&self, resolver: Arc<super::WorkspaceBodyCodecResolver>) {
        *self.body_codec_resolver.write() = Some(resolver);
    }

    #[cfg(test)]
    async fn install_stop_barrier(
        &self,
        listener_id: ListenerId,
    ) -> (
        Arc<tokio::sync::Notify>,
        Arc<tokio::sync::Notify>,
        Arc<tokio::sync::Notify>,
    ) {
        let run_token = self.running.lock().await[&listener_id].run_token;
        let barrier = StopBarrier {
            reached: Arc::new(tokio::sync::Notify::new()),
            release: Arc::new(tokio::sync::Notify::new()),
            completed: Arc::new(tokio::sync::Notify::new()),
        };
        self.stop_barriers
            .lock()
            .await
            .insert(run_token, barrier.clone());
        (barrier.reached, barrier.release, barrier.completed)
    }

    #[cfg(test)]
    async fn install_start_barrier(
        &self,
        listener_id: ListenerId,
    ) -> (
        Arc<tokio::sync::Notify>,
        Arc<tokio::sync::Notify>,
        Arc<tokio::sync::Notify>,
    ) {
        let barrier = StopBarrier {
            reached: Arc::new(tokio::sync::Notify::new()),
            release: Arc::new(tokio::sync::Notify::new()),
            completed: Arc::new(tokio::sync::Notify::new()),
        };
        self.start_barriers
            .lock()
            .await
            .insert(listener_id, barrier.clone());
        (barrier.reached, barrier.release, barrier.completed)
    }

    #[cfg(test)]
    async fn install_activation_barrier(
        &self,
        listener_id: ListenerId,
    ) -> (
        Arc<tokio::sync::Notify>,
        Arc<tokio::sync::Notify>,
        Arc<tokio::sync::Notify>,
    ) {
        let barrier = StopBarrier {
            reached: Arc::new(tokio::sync::Notify::new()),
            release: Arc::new(tokio::sync::Notify::new()),
            completed: Arc::new(tokio::sync::Notify::new()),
        };
        self.activation_barriers
            .lock()
            .await
            .insert(listener_id, barrier.clone());
        (barrier.reached, barrier.release, barrier.completed)
    }

    pub fn set_socket_diagnostic_events(&self, events: Arc<EventHub>) {
        *self.socket_diagnostic_events.write() = events;
    }

    fn runtime_epoch_for_start(&self, workspace_id: WorkspaceId) -> Uuid {
        let mut epochs = self.runtime_epochs.write();
        *epochs.entry(workspace_id).or_insert_with(Uuid::new_v4)
    }

    async fn reserve_start(
        &self,
        workspace_id: WorkspaceId,
        listener_id: ListenerId,
    ) -> AppResult<Uuid> {
        let running = self.running.lock().await;
        if running.contains_key(&listener_id)
            || self.pending_starts.read().contains_key(&listener_id)
        {
            return Err(
                AppError::new("LISTENER_ALREADY_RUNNING", "Listener 已在运行。")
                    .entity(listener_id.to_string()),
            );
        }
        let runtime_epoch = self.runtime_epoch_for_start(workspace_id);
        self.pending_starts.write().insert(
            listener_id,
            PendingListenerStart {
                workspace_id,
                runtime_epoch,
            },
        );
        drop(running);
        Ok(runtime_epoch)
    }

    async fn release_start(&self, listener_id: ListenerId) {
        let running = self.running.lock().await;
        let Some(pending) = self.pending_starts.write().remove(&listener_id) else {
            return;
        };
        let active_epoch_owned = running
            .values()
            .any(|candidate| candidate.runtime_epoch == pending.runtime_epoch)
            || self
                .pending_starts
                .read()
                .values()
                .any(|candidate| candidate.runtime_epoch == pending.runtime_epoch);
        if !active_epoch_owned {
            self.retire_runtime_epoch(pending.workspace_id, pending.runtime_epoch);
        }
        let epoch_owned = active_epoch_owned
            || self
                .stopping
                .read()
                .values()
                .any(|candidate| candidate.runtime_epoch == pending.runtime_epoch);
        drop(running);
        if !epoch_owned {
            self.cleanup_runtime_epoch(pending.runtime_epoch).await;
        }
    }

    async fn cleanup_runtime_epoch(&self, runtime_epoch: Uuid) {
        if let Some(resolver) = self.body_codec_resolver.read().clone() {
            resolver.remove_epoch(runtime_epoch);
        }
        let pipeline_services = self.pipeline_services.read().clone();
        if let Some(services) = pipeline_services {
            services.ports.runtime_stopping(runtime_epoch).await;
        }
    }

    async fn release_stopping(&self, run_token: Uuid) -> Option<Uuid> {
        let running = self.running.lock().await;
        let stopped = self.stopping.write().remove(&run_token)?;
        let epoch_owned = running
            .values()
            .any(|candidate| candidate.runtime_epoch == stopped.runtime_epoch)
            || self
                .pending_starts
                .read()
                .values()
                .any(|candidate| candidate.runtime_epoch == stopped.runtime_epoch)
            || self
                .stopping
                .read()
                .values()
                .any(|candidate| candidate.runtime_epoch == stopped.runtime_epoch);
        drop(running);
        (!epoch_owned).then_some(stopped.runtime_epoch)
    }

    fn retire_runtime_epoch(
        &self,
        workspace_id: WorkspaceId,
        expected_epoch: Uuid,
    ) -> Option<Uuid> {
        let mut epochs = self.runtime_epochs.write();
        (epochs.get(&workspace_id) == Some(&expected_epoch))
            .then(|| epochs.remove(&workspace_id))
            .flatten()
    }

    fn stopped(listener_id: ListenerId, listen_address: String) -> ListenerStatusViewModel {
        ListenerStatusViewModel {
            listener_id,
            runtime_epoch: None,
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
        if let Some(running) = Arc::get_mut(&mut self.running) {
            for handle in running.get_mut().values() {
                handle.cancellation.cancel();
                handle.task.abort();
            }
        }
        let epochs = Arc::get_mut(&mut self.runtime_epochs)
            .map(|epochs| epochs.get_mut().values().copied().collect::<Vec<_>>())
            .unwrap_or_default();
        if let Some(resolver) = Arc::get_mut(&mut self.body_codec_resolver)
            .and_then(|resolver| resolver.get_mut().as_ref())
        {
            for epoch in epochs {
                resolver.remove_epoch(epoch);
            }
        }
    }
}

mod document_rule_compiler;
mod document_rules;
mod external_relay;
mod helpers;
mod http_protocol_pipeline;
mod lifecycle;
mod plan;
mod port;
mod scripted_relay;
mod scripted_snapshot;
mod socket_diagnostics;
mod socket_plan;
mod start;
mod tls_material;

use document_rule_compiler::DocumentRuleCompiler;
pub use document_rules::{ProtocolDocumentRuleConnection, ProtocolDocumentRuleConnectionFactory};
pub(crate) use external_relay::{
    ExternalSocketPackageProvider,
    RuntimeExternalSocketPackageBinding as ExternalSocketPackageBinding,
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
