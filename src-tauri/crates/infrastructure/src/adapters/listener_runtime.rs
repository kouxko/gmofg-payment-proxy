//! 动态 Workspace Listener 的网络运行时适配器。

use std::{
    collections::BTreeMap,
    fs,
    io::{Cursor, Read},
    net::SocketAddr,
    path::{Path, PathBuf},
    sync::Arc,
    time::Duration,
};

use async_trait::async_trait;
use intercept_proxy_application::{
    AppError, AppResult, ListenerId, ListenerRuntimePort, ListenerRuntimeState,
    ListenerStatusViewModel, ListenerUpstreamTlsTestViewModel, ProxyListener, UiTone, WorkspaceId,
};
use intercept_proxy_domain::{
    CertificateReference, CertificateReferenceId, CertificateReferenceKind,
    DownstreamClientAuthentication, FixedServerSettings, ForwardProxyAuthentication,
    ProxyWorkspace,
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
use zeroize::Zeroizing;

use crate::{CertificateService, SqliteStore};

use super::{ManagedListenerCertificateAdapter, ProtectedSecretAdapter, common::app_error};

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
    fn downstream_tls(
        &self,
        workspace: &ProxyWorkspace,
        listener: &ProxyListener,
    ) -> AppResult<Option<ReverseDownstreamTls>> {
        if !listener.downstream_tls.enabled {
            return Ok(None);
        }

        let identity_id = listener
            .downstream_tls
            .server_identity
            .ok_or_else(|| AppError::new("CERTIFICATE_NOT_READY", "下游 TLS 服务端身份未配置。"))?;
        let server_identity = self.load_identity(certificate_reference(workspace, identity_id)?)?;
        let (client_trust_der, client_authentication_required) =
            match listener.downstream_tls.client_authentication {
                DownstreamClientAuthentication::Disabled => (Vec::new(), false),
                DownstreamClientAuthentication::Optional { trust } => (
                    self.load_trust(certificate_reference(workspace, trust)?)?,
                    false,
                ),
                DownstreamClientAuthentication::Required { trust } => (
                    self.load_trust(certificate_reference(workspace, trust)?)?,
                    true,
                ),
            };
        Ok(Some(ReverseDownstreamTls {
            server_identity,
            client_trust_der,
            client_authentication_required,
        }))
    }

    fn upstream_tls(
        &self,
        workspace: &ProxyWorkspace,
        fixed_server: &FixedServerSettings,
    ) -> AppResult<Option<ReverseUpstreamTls>> {
        if !fixed_server.upstream_url.starts_with("https://") {
            return Ok(None);
        }

        let server_trust_der = fixed_server
            .upstream_tls
            .server_trust
            .map(|id| certificate_reference(workspace, id))
            .transpose()?
            .map(|reference| self.load_trust(reference))
            .transpose()?
            .unwrap_or_default();
        let client_identity = fixed_server
            .upstream_tls
            .client_identity
            .map(|id| certificate_reference(workspace, id))
            .transpose()?
            .map(|reference| self.load_identity(reference))
            .transpose()?;
        Ok(Some(ReverseUpstreamTls {
            server_trust_der,
            client_identity,
            verify_hostname: fixed_server.upstream_tls.verify_hostname,
        }))
    }

    fn load_trust(&self, reference: &CertificateReference) -> AppResult<Vec<Vec<u8>>> {
        if let Some(result) = self
            .managed_listener_certificates
            .as_ref()
            .and_then(|resolver| resolver.resolve_trust(reference))
        {
            return result;
        }
        load_file_trust(reference)
    }

    fn load_identity(&self, reference: &CertificateReference) -> AppResult<ReverseClientIdentity> {
        if let Some(result) = self
            .managed_listener_certificates
            .as_ref()
            .and_then(|resolver| resolver.resolve_identity(reference))
        {
            return result;
        }
        load_file_identity(reference)
    }
}

fn certificate_reference(
    workspace: &ProxyWorkspace,
    id: CertificateReferenceId,
) -> AppResult<&CertificateReference> {
    workspace
        .certificate_references
        .iter()
        .find(|reference| reference.id == id)
        .ok_or_else(|| {
            AppError::new("CERTIFICATE_NOT_READY", "证书安全引用不存在。").entity(id.to_string())
        })
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

#[async_trait]
impl ListenerRuntimePort for ListenerRuntimeAdapter {
    async fn statuses(&self) -> AppResult<Vec<ListenerStatusViewModel>> {
        let running = self.running.lock().await;
        Ok(running
            .iter()
            .map(|(id, handle)| {
                let fault = handle.fault.read().clone();
                if handle.task.is_finished() || fault.is_some() {
                    ListenerStatusViewModel {
                        listener_id: *id,
                        state: ListenerRuntimeState::Faulted,
                        state_text: "故障".into(),
                        ui_tone: UiTone::Danger,
                        listen_address: handle.listen_address.clone(),
                        fault_reason: fault.or_else(|| Some("Listener 任务已意外结束。".into())),
                        can_start: false,
                        can_stop: true,
                    }
                } else {
                    ListenerStatusViewModel {
                        listener_id: *id,
                        state: ListenerRuntimeState::Running,
                        state_text: "运行中".into(),
                        ui_tone: UiTone::Positive,
                        listen_address: handle.listen_address.clone(),
                        fault_reason: None,
                        can_start: false,
                        can_stop: true,
                    }
                }
            })
            .collect())
    }

    #[allow(clippy::too_many_lines)]
    async fn start(
        &self,
        workspace: ProxyWorkspace,
        listener: ProxyListener,
    ) -> AppResult<ListenerStatusViewModel> {
        let listener_id = listener.id;
        if self.running.lock().await.contains_key(&listener_id) {
            return Err(
                AppError::new("LISTENER_ALREADY_RUNNING", "Listener 已在运行。")
                    .entity(listener_id.to_string()),
            );
        }
        workspace.validate().map_err(AppError::from)?;
        if !workspace
            .listeners
            .iter()
            .any(|candidate| candidate == &listener)
        {
            return Err(AppError::new(
                "LISTENER_NOT_FOUND",
                "启动快照中不存在完全匹配的代理入口配置。",
            )
            .entity(listener_id.to_string()));
        }
        let workspace_id = workspace.id;
        if listener.fixed_server.is_some() {
            let (tcp_listener, service, listen_address) =
                self.start_fixed_server(&workspace, &listener).await?;
            let runtime_epoch = self.runtime_epoch_for_start(workspace_id);
            let cancellation = CancellationToken::new();
            let task_cancellation = cancellation.clone();
            let fault = Arc::new(RwLock::new(None));
            let task_fault = Arc::clone(&fault);
            let task = tokio::spawn(async move {
                if let Err(error) = service
                    .serve_listener_with_epoch(tcp_listener, runtime_epoch, task_cancellation)
                    .await
                    && error.code != "BREAKPOINT_PROXY_STOPPED"
                {
                    *task_fault.write() = Some(error.message);
                }
            });
            self.running.lock().await.insert(
                listener_id,
                RunningListener {
                    cancellation,
                    task,
                    listen_address: listen_address.clone(),
                    fault,
                    workspace,
                },
            );
            return Ok(running_status(listener_id, listen_address));
        }
        let (authentication, authenticator): (
            ForwardAuthenticationMode,
            Arc<dyn ForwardProxyAuthenticator>,
        ) = match &listener.authentication {
            ForwardProxyAuthentication::None => {
                (ForwardAuthenticationMode::None, Arc::new(NoAuthentication))
            }
            ForwardProxyAuthentication::Basic { credential } => {
                let resolver = self.protected_secrets.as_ref().ok_or_else(|| {
                    AppError::new(
                        "SECRET_PROTECTOR_UNAVAILABLE",
                        "当前宿主没有提供代理认证安全引用解析能力。",
                    )
                    .entity(listener_id.to_string())
                })?;
                (
                    ForwardAuthenticationMode::Required,
                    resolver.resolve_basic_authenticator(credential)?,
                )
            }
        };
        let bind_addr = parse_bind_address(&listener.bind_address, listener.port, listener_id)?;
        let mut service = ForwardProxyService::new(
            ForwardProxyConfig {
                bind_addr,
                authentication,
                allowed_client_cidrs: listener.allowed_client_cidrs.clone(),
                connect_timeout: Duration::from_millis(listener.connect_timeout_ms),
                read_timeout: Duration::from_millis(listener.read_timeout_ms),
                write_timeout: Duration::from_millis(listener.write_timeout_ms),
                tunnel_idle_timeout: Duration::from_millis(
                    listener.read_timeout_ms.min(listener.write_timeout_ms),
                ),
            },
            authenticator,
        )
        .map_err(|error| AppError::new(error.code, error.message))?;
        if listener.mitm.enabled {
            // Workspace 校验已经保证 allowlist 与 Root CA 引用存在。这里仍 fail-closed：
            // 宿主若没有注入受保护的安装级签发器，绝不能静默降级为透明隧道。
            let certificate_authority =
                self.mitm_certificate_authority.clone().ok_or_else(|| {
                    AppError::new(
                        "CERTIFICATE_NOT_READY",
                        "MITM 已启用，但安装级 Root CA 签发能力尚未就绪。",
                    )
                    .entity(listener_id.to_string())
                })?;
            let upstream = NativeRootMitmConnector::new()
                .map_err(|error| AppError::new(error.code, error.message))?;
            service = service
                .with_mitm(
                    ForwardMitmConfig {
                        authority_allowlist: listener.mitm.authority_allowlist.clone(),
                        maximum_cached_leaf_certificates: usize::from(
                            listener.mitm.maximum_cached_leaf_certificates,
                        ),
                    },
                    certificate_authority,
                    Arc::new(upstream),
                )
                .map_err(|error| AppError::new(error.code, error.message))?;
        }
        let pipeline_ports = self.pipeline_ports.read().clone().ok_or_else(|| {
            AppError::new(
                "LISTENER_RUNTIME_NOT_READY",
                "通用规则、抓包与断点管线尚未完成装配。",
            )
            .entity(listener_id.to_string())
        })?;
        let tcp_listener = bind_tcp_listener(bind_addr, listener_id).await?;
        let channel = RuntimeChannelId::new(listener_id.to_string())
            .map_err(|error| AppError::new(error.code, error.message))?;
        let runtime_epoch = self.runtime_epoch_for_start(workspace_id);
        service = service.with_pipeline(
            channel,
            runtime_epoch,
            pipeline_ports,
            MessageLimits::default(),
        );
        let mut running = self.running.lock().await;
        let cancellation = CancellationToken::new();
        let task_cancellation = cancellation.clone();
        let fault = Arc::new(RwLock::new(None));
        let task_fault = Arc::clone(&fault);
        let task = tokio::spawn(async move {
            if let Err(error) = service
                .serve_listener(tcp_listener, task_cancellation)
                .await
                && error.code != "PROXY_STOPPED"
            {
                *task_fault.write() = Some(error.message);
            }
        });
        let listen_address = bind_addr.to_string();
        running.insert(
            listener_id,
            RunningListener {
                cancellation,
                task,
                listen_address: listen_address.clone(),
                fault,
                workspace,
            },
        );
        Ok(running_status(listener_id, listen_address))
    }

    async fn stop(&self, listener_id: ListenerId) -> AppResult<ListenerStatusViewModel> {
        let (handle, workspace_stopped) = {
            let mut running = self.running.lock().await;
            let handle = running.remove(&listener_id).ok_or_else(|| {
                AppError::new("LISTENER_NOT_RUNNING", "Listener 当前未运行。")
                    .entity(listener_id.to_string())
            })?;
            let workspace_stopped = running
                .values()
                .all(|candidate| candidate.workspace.id != handle.workspace.id);
            (handle, workspace_stopped)
        };
        handle.cancellation.cancel();
        let stop_error = match handle.task.await {
            Err(error) if !error.is_cancelled() => Some(
                AppError::new(
                    "LISTENER_STOP_FAILED",
                    format!("Listener 任务停止失败：{error}"),
                )
                .entity(listener_id.to_string()),
            ),
            _ => None,
        };
        let stopped_epoch = workspace_stopped
            .then(|| self.runtime_epochs.write().remove(&handle.workspace.id))
            .flatten();
        let pipeline_ports = self.pipeline_ports.read().clone();
        if let (Some(epoch), Some(ports)) = (stopped_epoch, pipeline_ports) {
            ports.runtime_stopping(epoch).await;
        }
        if let Some(error) = stop_error {
            return Err(error);
        }
        Ok(Self::stopped(listener_id, handle.listen_address))
    }

    async fn test_upstream_tls(
        &self,
        workspace: ProxyWorkspace,
        listener: ProxyListener,
    ) -> AppResult<ListenerUpstreamTlsTestViewModel> {
        let fixed_server = listener.fixed_server.as_ref().ok_or_else(|| {
            AppError::new(
                "FIXED_SERVER_NOT_CONFIGURED",
                "该代理监听未配置固定 Server，没有上游 TLS 可测试。",
            )
            .entity(listener.id.to_string())
        })?;
        if !fixed_server.upstream_url.starts_with("https://") {
            return Err(AppError::new(
                "UPSTREAM_TLS_NOT_ENABLED",
                "该入口使用 HTTP 上游，没有 TLS 握手可测试。",
            )
            .entity(listener.id.to_string()));
        }
        let upstream_tls = self.upstream_tls(&workspace, fixed_server)?;
        let service = ReverseProxyService::build(ReverseProxyConfig {
            bind_addr: "127.0.0.1:0"
                .parse()
                .expect("loopback probe address is valid"),
            upstream_origin: fixed_server.upstream_url.clone(),
            downstream_tls: None,
            upstream_tls,
            connect_timeout: Duration::from_secs(10),
            read_timeout: Duration::from_secs(10),
            write_timeout: Duration::from_secs(10),
        })
        .await
        .map_err(|error| upstream_tls_test_error(listener.id, &error))?;
        let result = service
            .test_upstream_tls()
            .await
            .map_err(|error| upstream_tls_test_error(listener.id, &error))?;
        Ok(ListenerUpstreamTlsTestViewModel {
            listener_id: listener.id,
            upstream_origin: fixed_server.upstream_url.clone(),
            resolved_address: result.resolved_address.to_string(),
            tls_version: result.tls_version,
            cipher_suite: result.cipher_suite,
            peer_subject: result.peer_subject,
            peer_sha256_fingerprint: result.peer_sha256_fingerprint,
            hostname_verification_enabled: result.hostname_verification_enabled,
            client_identity_configured: result.client_identity_configured,
            elapsed_millis: result.elapsed_millis,
            message: "上游 Server TLS 握手成功。".into(),
            ui_tone: UiTone::Positive,
        })
    }
}

fn upstream_tls_test_error(
    listener_id: ListenerId,
    error: &intercept_proxy_runtime::ProxyError,
) -> AppError {
    // rustls 会把“当前信任库里没有签发者”作为 UnknownIssuer 返回。这里不能自动
    // 回退到系统信任根，否则用户显式选择的私有 CA 会被静默绕过；应当把根因和
    // 两种正确处理方式明确交给 UI。
    if error.code == "TLS_HANDSHAKE_FAILED" && error.message.contains("UnknownIssuer") {
        return AppError::new(
            error.code,
            "上游 Server 证书不受当前 CA 信任。当前选择的 CA 不是该 Server 证书链的签发者。",
        )
        .entity(listener_id.to_string())
        .retryable(
            "公开 HTTPS 请选择“使用操作系统信任根”；私有 Server 请导入其真实签发 CA 后重试。",
        );
    }
    let message = match error.code {
        "CONFIG_INVALID" => format!("上游地址配置无效：{}", error.message),
        "CERTIFICATE_NOT_READY" | "CERTIFICATE_INVALID" => {
            format!("上游证书配置无效：{}", error.message)
        }
        "TLS_HANDSHAKE_FAILED" => format!("上游 Server TLS 握手失败：{}", error.message),
        "UPSTREAM_CONNECT_TIMEOUT" => format!("连接上游 Server 超时：{}", error.message),
        "IO_ERROR" => format!("无法连接上游 Server：{}", error.message),
        _ => format!("上游 TLS 测试失败：{}", error.message),
    };
    let error = AppError::new(error.code, message).entity(listener_id.to_string());
    if matches!(
        error.view_model.code.as_str(),
        "TLS_HANDSHAKE_FAILED" | "UPSTREAM_CONNECT_TIMEOUT" | "IO_ERROR"
    ) {
        error.retryable("检查 Server 地址、网络、CA、主机名和可选客户端证书后重试。")
    } else {
        error
    }
}

fn running_status(listener_id: ListenerId, listen_address: String) -> ListenerStatusViewModel {
    ListenerStatusViewModel {
        listener_id,
        state: ListenerRuntimeState::Running,
        state_text: "运行中".into(),
        ui_tone: UiTone::Positive,
        listen_address,
        fault_reason: None,
        can_start: false,
        can_stop: true,
    }
}

fn parse_bind_address(address: &str, port: u16, id: ListenerId) -> AppResult<SocketAddr> {
    format!("{address}:{port}")
        .parse::<SocketAddr>()
        .map_err(|error| {
            AppError::new("CONFIG_INVALID", format!("Listener 地址无法解析：{error}"))
                .entity(id.to_string())
        })
}

async fn bind_tcp_listener(address: SocketAddr, id: ListenerId) -> AppResult<TcpListener> {
    TcpListener::bind(address).await.map_err(|error| {
        AppError::new(
            if error.kind() == std::io::ErrorKind::AddrInUse {
                "PORT_IN_USE"
            } else {
                "LISTENER_START_FAILED"
            },
            format!("无法监听 {address}：{error}"),
        )
        .entity(id.to_string())
    })
}

fn load_file_trust(reference: &CertificateReference) -> AppResult<Vec<Vec<u8>>> {
    let path = reference_path(&reference.reference)?;
    let bytes = read_reference_file(&path)?;
    let service = CertificateService;
    let trusted = if reference.kind == CertificateReferenceKind::DownstreamClientTrust {
        service.parse_client_trust_anchor(&bytes)
    } else {
        service.parse_upstream_ca(&bytes)
    }
    .map_err(app_error)?;
    Ok(vec![trusted.certificate_der])
}

fn load_file_identity(reference: &CertificateReference) -> AppResult<ReverseClientIdentity> {
    let (path, password_environment) = identity_reference(&reference.reference)?;
    // 身份文件可能同时包含私钥；从文件读取开始就使用可清零缓冲，避免 PEM/P12
    // 原始材料先落入普通 Vec 再被包装。
    let bytes = read_identity_reference_file(&path)?;
    if let Some(variable) = password_environment {
        let password = Zeroizing::new(std::env::var(&variable).map_err(|_| {
            AppError::new(
                "CERTIFICATE_NOT_READY",
                format!("PKCS12 密码环境变量 {variable} 未设置。"),
            )
        })?);
        let mut parsed = CertificateService
            .parse_pkcs12(&bytes, password.as_str())
            .map_err(app_error)?;
        // ParsedPkcs12 自身实现 Drop 以清零私钥，不能直接移动字段。用 take 把所有权
        // 转交给运行时身份，并在原结构中留下空值，避免复制任何私钥缓冲。
        let mut chain = vec![std::mem::take(&mut parsed.certificate_der)];
        chain.extend(std::mem::take(&mut parsed.chain_der));
        return Ok(ReverseClientIdentity {
            certificate_chain_der: chain,
            private_key_pkcs8_der: std::mem::take(&mut parsed.private_key_pkcs8_der),
        });
    }

    let mut certificates = Cursor::new(bytes.as_slice());
    let certificate_chain_der = rustls_pemfile::certs(&mut certificates)
        .map(|entry| entry.map(|value| value.as_ref().to_vec()))
        .collect::<Result<Vec<_>, _>>()
        .map_err(|error| {
            AppError::new("CERTIFICATE_INVALID", format!("PEM 证书链无效：{error}"))
        })?;
    let mut private_key = Cursor::new(bytes.as_slice());
    let private_key_der = rustls_pemfile::private_key(&mut private_key)
        .map_err(|error| AppError::new("CERTIFICATE_INVALID", format!("PEM 私钥无效：{error}")))?
        .ok_or_else(|| AppError::new("CERTIFICATE_INVALID", "PEM 身份缺少私钥。"))?;
    let mut private_key_pkcs8_der =
        Zeroizing::new(Vec::with_capacity(private_key_der.secret_der().len()));
    private_key_pkcs8_der.extend_from_slice(private_key_der.secret_der());
    if certificate_chain_der.is_empty() {
        return Err(AppError::new("CERTIFICATE_INVALID", "PEM 身份缺少证书链。"));
    }
    Ok(ReverseClientIdentity {
        certificate_chain_der,
        private_key_pkcs8_der,
    })
}

fn read_identity_reference_file(path: &Path) -> AppResult<Zeroizing<Vec<u8>>> {
    let mut file = fs::File::open(path).map_err(|error| {
        AppError::new(
            "CERTIFICATE_NOT_READY",
            format!("无法读取证书安全引用 {}：{error}", path.display()),
        )
    })?;
    let mut bytes = Zeroizing::new(Vec::new());
    file.read_to_end(&mut bytes).map_err(|error| {
        AppError::new(
            "CERTIFICATE_NOT_READY",
            format!("无法读取证书安全引用 {}：{error}", path.display()),
        )
    })?;
    Ok(bytes)
}

fn read_reference_file(path: &Path) -> AppResult<Vec<u8>> {
    fs::read(path).map_err(|error| {
        AppError::new(
            "CERTIFICATE_NOT_READY",
            format!("无法读取证书安全引用 {}：{error}", path.display()),
        )
    })
}

fn reference_path(reference: &str) -> AppResult<PathBuf> {
    let value = reference.strip_prefix("file:").unwrap_or(reference);
    if value.trim().is_empty() {
        return Err(AppError::new("CERTIFICATE_NOT_READY", "证书安全引用为空。"));
    }
    Ok(PathBuf::from(value))
}

fn identity_reference(reference: &str) -> AppResult<(PathBuf, Option<String>)> {
    if let Some(value) = reference.strip_prefix("pkcs12:") {
        let (path, query) = value.split_once('?').ok_or_else(|| {
            AppError::new(
                "CERTIFICATE_NOT_READY",
                "PKCS12 引用必须提供 ?password_env=环境变量名。",
            )
        })?;
        let variable = query
            .strip_prefix("password_env=")
            .filter(|value| !value.is_empty())
            .ok_or_else(|| {
                AppError::new("CERTIFICATE_NOT_READY", "PKCS12 引用的 password_env 无效。")
            })?;
        return Ok((PathBuf::from(path), Some(variable.to_owned())));
    }
    Ok((reference_path(reference)?, None))
}

#[cfg(test)]
mod tests {
    use std::sync::atomic::{AtomicUsize, Ordering};

    use chrono::Utc;
    use intercept_proxy_application::ListenerRuntimePort;
    use intercept_proxy_domain::{
        FixedServerSettings, ListenerId, ProxyListener, ProxyWorkspace, Revision,
        UpstreamTlsSettings,
    };
    use intercept_proxy_runtime::{
        ConnectionContext, ErrorCode, FaultAction, HandshakePolicy, Message, NoopPipelinePorts,
        PipelinePorts, ProxyError,
    };
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    use tokio::net::TcpStream;

    use super::*;
    use crate::WorkspaceRecord;

    #[test]
    fn unknown_issuer_explains_system_roots_without_weakening_explicit_trust() {
        let listener_id = ListenerId::new();
        let runtime_error = ProxyError::new(
            ErrorCode::TlsHandshakeFailed,
            "invalid peer certificate: UnknownIssuer",
        );

        let error = upstream_tls_test_error(listener_id, &runtime_error);

        assert_eq!(error.view_model.code, "TLS_HANDSHAKE_FAILED");
        assert_eq!(error.view_model.entity_id, Some(listener_id.to_string()));
        assert!(
            error
                .view_model
                .message
                .contains("不是该 Server 证书链的签发者")
        );
        assert_eq!(
            error.view_model.suggested_action.as_deref(),
            Some("公开 HTTPS 请选择“使用操作系统信任根”；私有 Server 请导入其真实签发 CA 后重试。")
        );
    }

    #[derive(Debug, Default)]
    struct CountingPipeline {
        requests: AtomicUsize,
        responses: AtomicUsize,
    }

    #[test]
    fn pem_identity_source_is_zeroizing_and_parse_errors_do_not_echo_secret_bytes() {
        let marker = "PRIVATE-MARKER-MUST-NOT-LEAK";
        let path = std::env::temp_dir().join(format!("intercept-identity-{}.pem", Uuid::new_v4()));
        fs::write(&path, format!("-----BEGIN PRIVATE KEY-----\n{marker}\n")).unwrap();

        let bytes = read_identity_reference_file(&path).unwrap();
        let _: &Zeroizing<Vec<u8>> = &bytes;
        assert!(String::from_utf8_lossy(&bytes).contains(marker));
        drop(bytes);

        let reference = CertificateReference {
            id: CertificateReferenceId::new(),
            label: "invalid identity".into(),
            kind: intercept_proxy_domain::CertificateReferenceKind::UpstreamClientIdentity,
            reference: format!("file:{}", path.display()),
        };
        let error = load_file_identity(&reference).unwrap_err();
        assert!(!error.view_model.message.contains(marker));
        let _ = fs::remove_file(path);
    }

    impl HandshakePolicy for CountingPipeline {}

    #[async_trait]
    impl PipelinePorts for CountingPipeline {
        async fn request(
            &self,
            _context: &ConnectionContext,
            _message: &mut Message,
        ) -> intercept_proxy_runtime::Result<Vec<FaultAction>> {
            self.requests.fetch_add(1, Ordering::SeqCst);
            Ok(Vec::new())
        }

        async fn response(
            &self,
            _context: &ConnectionContext,
            _message: &mut Message,
        ) -> intercept_proxy_runtime::Result<Vec<FaultAction>> {
            self.responses.fetch_add(1, Ordering::SeqCst);
            Ok(Vec::new())
        }
    }

    #[tokio::test]
    async fn forward_absolute_form_http_enters_shared_pipeline() {
        let upstream = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let upstream_address = upstream.local_addr().unwrap();
        let upstream_task = tokio::spawn(async move {
            let (mut stream, _) = upstream.accept().await.unwrap();
            let mut request = Vec::new();
            let mut buffer = [0_u8; 256];
            while !request.windows(4).any(|window| window == b"\r\n\r\n") {
                let read = stream.read(&mut buffer).await.unwrap();
                assert!(read > 0);
                request.extend_from_slice(&buffer[..read]);
            }
            assert!(request.starts_with(b"GET /through-pipeline HTTP/1.1"));
            stream
                .write_all(b"HTTP/1.1 200 OK\r\nContent-Length: 2\r\nConnection: close\r\n\r\nok")
                .await
                .unwrap();
        });
        let reservation = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let bind_address = reservation.local_addr().unwrap();
        drop(reservation);
        let listener = ProxyListener {
            id: ListenerId::new(),
            name: "forward".into(),
            enabled: false,
            bind_address: bind_address.ip().to_string(),
            port: bind_address.port(),
            ..ProxyListener::default()
        };
        let workspace = ProxyWorkspace {
            listeners: vec![listener.clone()],
            ..ProxyWorkspace::default()
        };
        workspace.validate().unwrap();
        let store = Arc::new(SqliteStore::in_memory().unwrap());
        store
            .insert_workspace(&WorkspaceRecord {
                id: workspace.id.as_uuid(),
                revision: workspace.revision.get(),
                value: serde_json::to_value(&workspace).unwrap(),
                updated_at: Utc::now(),
            })
            .unwrap();
        let pipeline = Arc::new(CountingPipeline::default());
        let runtime = ListenerRuntimeAdapter::new(store);
        runtime.set_pipeline_ports(pipeline.clone());
        runtime
            .start(workspace.clone(), listener.clone())
            .await
            .unwrap();

        let mut client = TcpStream::connect(bind_address).await.unwrap();
        client
            .write_all(
                format!(
                    "GET http://{upstream_address}/through-pipeline HTTP/1.1\r\nHost: {upstream_address}\r\nConnection: close\r\n\r\n"
                )
                .as_bytes(),
            )
            .await
            .unwrap();
        let mut response = Vec::new();
        tokio::time::timeout(Duration::from_secs(3), async {
            let mut buffer = [0_u8; 256];
            while !response.ends_with(b"\r\n\r\nok") {
                let read = client.read(&mut buffer).await.unwrap();
                assert!(read > 0, "response ended before its complete body");
                response.extend_from_slice(&buffer[..read]);
            }
        })
        .await
        .expect("forward response timeout");
        assert!(response.starts_with(b"HTTP/1.1 200"));
        assert_eq!(pipeline.requests.load(Ordering::SeqCst), 1);
        assert_eq!(pipeline.responses.load(Ordering::SeqCst), 1);

        upstream_task.await.unwrap();
        runtime.stop(listener.id).await.unwrap();
    }

    #[tokio::test]
    async fn fixed_server_listener_uses_selected_workspace_pipeline_and_preserves_body_bytes() {
        let upstream = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let upstream_address = upstream.local_addr().unwrap();
        let upstream_task = tokio::spawn(async move {
            let (mut stream, _) = upstream.accept().await.unwrap();
            let mut request = Vec::new();
            let mut buffer = [0_u8; 256];
            loop {
                let read = stream.read(&mut buffer).await.unwrap();
                assert!(read > 0, "HTTP request ended before its complete body");
                request.extend_from_slice(&buffer[..read]);
                if request
                    .windows(4)
                    .position(|window| window == b"\r\n\r\n")
                    .is_some_and(|head_end| request.len() >= head_end + 4 + 4)
                {
                    break;
                }
            }
            assert!(request.ends_with(b"\x00\x81\xff\x7f"));
            stream
                .write_all(b"HTTP/1.1 200 OK\r\nContent-Length: 4\r\n\r\n\xff\x00ok")
                .await
                .unwrap();
        });

        let reservation = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let bind_address = reservation.local_addr().unwrap();
        drop(reservation);
        let listener = ProxyListener {
            id: ListenerId::new(),
            name: "fixed server".into(),
            enabled: false,
            bind_address: bind_address.ip().to_string(),
            port: bind_address.port(),
            fixed_server: Some(FixedServerSettings {
                upstream_url: format!("http://{upstream_address}"),
                upstream_tls: UpstreamTlsSettings::default(),
            }),
            ..ProxyListener::default()
        };
        let workspace = ProxyWorkspace {
            id: intercept_proxy_domain::WorkspaceId::new(),
            name: "test".into(),
            revision: Revision::INITIAL,
            listeners: vec![listener.clone()],
            metadata_extractors: Vec::new(),
            response_assertions: Vec::new(),
            rules: Vec::new(),
            fault_presets: Vec::new(),
            certificate_references: Vec::new(),
            android_network_profiles: Vec::new(),
        };
        workspace.validate().unwrap();
        let store = Arc::new(SqliteStore::in_memory().unwrap());
        store
            .insert_workspace(&WorkspaceRecord {
                id: workspace.id.as_uuid(),
                revision: workspace.revision.get(),
                value: serde_json::to_value(&workspace).unwrap(),
                updated_at: Utc::now(),
            })
            .unwrap();
        let runtime = ListenerRuntimeAdapter::new(store);
        runtime.set_pipeline_ports(Arc::new(NoopPipelinePorts));
        let status = runtime
            .start(workspace.clone(), listener.clone())
            .await
            .unwrap();
        assert_eq!(status.state, ListenerRuntimeState::Running);

        let mut client = TcpStream::connect(bind_address).await.unwrap();
        client
            .write_all(
                b"POST /binary HTTP/1.1\r\nHost: preserved.test\r\nContent-Length: 4\r\n\r\n\x00\x81\xff\x7f",
            )
            .await
            .unwrap();
        let mut response = Vec::new();
        client.read_to_end(&mut response).await.unwrap();
        assert!(response.starts_with(b"HTTP/1.1 200"));
        assert!(response.ends_with(b"\xff\x00ok"));
        upstream_task.await.unwrap();
        runtime.stop(listener.id).await.unwrap();
    }

    #[tokio::test]
    async fn fixed_server_connect_cannot_escape_to_request_authority() {
        let fixed_upstream = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let fixed_upstream_address = fixed_upstream.local_addr().unwrap();
        let fixed_upstream_task = tokio::spawn(async move {
            if let Ok(Ok((mut stream, _))) =
                tokio::time::timeout(Duration::from_secs(2), fixed_upstream.accept()).await
            {
                let mut request = [0_u8; 512];
                let _ = stream.read(&mut request).await;
                let _ = stream
                    .write_all(
                        b"HTTP/1.1 405 Method Not Allowed\r\nContent-Length: 0\r\nConnection: close\r\n\r\n",
                    )
                    .await;
            }
        });
        let forbidden_target = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let forbidden_address = forbidden_target.local_addr().unwrap();

        let reservation = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let bind_address = reservation.local_addr().unwrap();
        drop(reservation);
        let listener = ProxyListener {
            id: ListenerId::new(),
            name: "fixed CONNECT isolation".into(),
            bind_address: bind_address.ip().to_string(),
            port: bind_address.port(),
            fixed_server: Some(FixedServerSettings {
                upstream_url: format!("http://{fixed_upstream_address}"),
                upstream_tls: UpstreamTlsSettings::default(),
            }),
            ..ProxyListener::default()
        };
        let workspace = ProxyWorkspace {
            listeners: vec![listener.clone()],
            ..ProxyWorkspace::default()
        };
        let runtime = ListenerRuntimeAdapter::new(Arc::new(SqliteStore::in_memory().unwrap()));
        runtime.set_pipeline_ports(Arc::new(NoopPipelinePorts));
        runtime
            .start(workspace.clone(), listener.clone())
            .await
            .unwrap();

        let mut client = TcpStream::connect(bind_address).await.unwrap();
        client
            .write_all(
                format!(
                    "CONNECT {forbidden_address} HTTP/1.1\r\nHost: {forbidden_address}\r\n\r\n"
                )
                .as_bytes(),
            )
            .await
            .unwrap();
        let mut response = [0_u8; 256];
        let _ = tokio::time::timeout(Duration::from_secs(2), client.read(&mut response)).await;

        assert!(
            tokio::time::timeout(Duration::from_millis(300), forbidden_target.accept())
                .await
                .is_err(),
            "固定 Server 模式不得按 CONNECT authority 建立旁路隧道"
        );
        runtime.stop(listener.id).await.unwrap();
        fixed_upstream_task.await.unwrap();
    }

    #[tokio::test]
    async fn multiple_fixed_server_listeners_route_to_their_own_upstream_origins() {
        async fn upstream(response_body: &'static [u8]) -> (SocketAddr, JoinHandle<()>) {
            let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
            let address = listener.local_addr().unwrap();
            let task = tokio::spawn(async move {
                let (mut stream, _) = listener.accept().await.unwrap();
                let mut request = Vec::new();
                let mut buffer = [0_u8; 256];
                while !request.windows(4).any(|window| window == b"\r\n\r\n") {
                    let read = stream.read(&mut buffer).await.unwrap();
                    assert!(read > 0, "request headers ended unexpectedly");
                    request.extend_from_slice(&buffer[..read]);
                }
                let response = format!(
                    "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                    response_body.len()
                );
                stream.write_all(response.as_bytes()).await.unwrap();
                stream.write_all(response_body).await.unwrap();
            });
            (address, task)
        }

        async fn reserve_local_address() -> SocketAddr {
            let reservation = TcpListener::bind("127.0.0.1:0").await.unwrap();
            let address = reservation.local_addr().unwrap();
            drop(reservation);
            address
        }

        async fn request(address: SocketAddr) -> Vec<u8> {
            let mut client = TcpStream::connect(address).await.unwrap();
            client
                .write_all(b"GET / HTTP/1.1\r\nHost: local.test\r\nConnection: close\r\n\r\n")
                .await
                .unwrap();
            let mut response = Vec::new();
            client.read_to_end(&mut response).await.unwrap();
            response
        }

        let (transaction_upstream, transaction_task) = upstream(b"transaction-response").await;
        let (dll_upstream, dll_task) = upstream(b"dll-response").await;
        let transaction_bind = reserve_local_address().await;
        let dll_bind = reserve_local_address().await;
        let fixed = |name: &str, bind: SocketAddr, upstream: SocketAddr| ProxyListener {
            id: ListenerId::new(),
            name: name.into(),
            enabled: false,
            bind_address: bind.ip().to_string(),
            port: bind.port(),
            fixed_server: Some(FixedServerSettings {
                upstream_url: format!("http://{upstream}"),
                upstream_tls: UpstreamTlsSettings::default(),
            }),
            ..ProxyListener::default()
        };
        let transaction = fixed("Transaction", transaction_bind, transaction_upstream);
        let dll = fixed("DLL", dll_bind, dll_upstream);
        let workspace = ProxyWorkspace {
            id: intercept_proxy_domain::WorkspaceId::new(),
            name: "multiple mappings".into(),
            revision: Revision::INITIAL,
            listeners: vec![transaction.clone(), dll.clone()],
            metadata_extractors: Vec::new(),
            response_assertions: Vec::new(),
            rules: Vec::new(),
            fault_presets: Vec::new(),
            certificate_references: Vec::new(),
            android_network_profiles: Vec::new(),
        };
        workspace.validate().unwrap();
        let store = Arc::new(SqliteStore::in_memory().unwrap());
        store
            .insert_workspace(&WorkspaceRecord {
                id: workspace.id.as_uuid(),
                revision: workspace.revision.get(),
                value: serde_json::to_value(&workspace).unwrap(),
                updated_at: Utc::now(),
            })
            .unwrap();
        let runtime = ListenerRuntimeAdapter::new(store);
        runtime.set_pipeline_ports(Arc::new(NoopPipelinePorts));

        for listener in [transaction.clone(), dll.clone()] {
            runtime.start(workspace.clone(), listener).await.unwrap();
        }
        assert_eq!(runtime.statuses().await.unwrap().len(), 2);

        let transaction_response = request(transaction_bind).await;
        let dll_response = request(dll_bind).await;
        assert!(transaction_response.ends_with(b"transaction-response"));
        assert!(dll_response.ends_with(b"dll-response"));

        transaction_task.await.unwrap();
        dll_task.await.unwrap();
        runtime.stop(transaction.id).await.unwrap();
        runtime.stop(dll.id).await.unwrap();
    }

    #[tokio::test]
    async fn upstream_tls_probe_requires_a_fixed_https_server() {
        let runtime = ListenerRuntimeAdapter::new(Arc::new(SqliteStore::in_memory().unwrap()));
        let dynamic = ProxyListener::default();
        let dynamic_workspace = ProxyWorkspace {
            listeners: vec![dynamic.clone()],
            ..ProxyWorkspace::default()
        };
        let error = runtime
            .test_upstream_tls(dynamic_workspace, dynamic)
            .await
            .unwrap_err();
        assert_eq!(error.view_model.code, "FIXED_SERVER_NOT_CONFIGURED");

        let http = ProxyListener {
            fixed_server: Some(FixedServerSettings {
                upstream_url: "http://127.0.0.1:8080".into(),
                upstream_tls: UpstreamTlsSettings::default(),
            }),
            ..ProxyListener::default()
        };
        let http_workspace = ProxyWorkspace {
            listeners: vec![http.clone()],
            ..ProxyWorkspace::default()
        };
        let error = runtime
            .test_upstream_tls(http_workspace, http)
            .await
            .unwrap_err();
        assert_eq!(error.view_model.code, "UPSTREAM_TLS_NOT_ENABLED");
    }

    #[test]
    fn pkcs12_reference_requires_external_password_reference() {
        let error = identity_reference("pkcs12:/tmp/client.p12").unwrap_err();
        assert_eq!(error.view_model.code, "CERTIFICATE_NOT_READY");
        let (path, variable) =
            identity_reference("pkcs12:/tmp/client.p12?password_env=PROXY_TEST_PASSWORD").unwrap();
        assert_eq!(path, PathBuf::from("/tmp/client.p12"));
        assert_eq!(variable.as_deref(), Some("PROXY_TEST_PASSWORD"));
    }
}
