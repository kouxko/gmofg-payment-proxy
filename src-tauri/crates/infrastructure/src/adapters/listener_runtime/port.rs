use async_trait::async_trait;

use super::{
    AppError, AppResult, Arc, CancellationToken, Duration, ForwardAuthenticationMode,
    ForwardMitmConfig, ForwardProxyAuthentication, ForwardProxyAuthenticator, ForwardProxyConfig,
    ForwardProxyService, ListenerId, ListenerRuntimeAdapter, ListenerRuntimePort,
    ListenerRuntimeState, ListenerStatusViewModel, ListenerUpstreamTlsTestViewModel, MessageLimits,
    NativeRootMitmConnector, NoAuthentication, ProxyListener, ProxyWorkspace, ReverseProxyConfig,
    ReverseProxyService, RunningListener, RuntimeChannelId, RwLock, UiTone, bind_tcp_listener,
    parse_bind_address, running_status, upstream_tls_test_error,
};

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
            allowed_client_cidrs: Vec::new(),
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
