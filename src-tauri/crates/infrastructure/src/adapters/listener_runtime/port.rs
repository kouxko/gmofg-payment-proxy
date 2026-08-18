use std::sync::Arc;

use async_trait::async_trait;
use intercept_proxy_application::{
    AppError, AppResult, ListenerDataPlaneKind, ListenerId, ListenerRuntimePort,
    ListenerRuntimeState, ListenerStatusViewModel, ListenerUpstreamConnectionTestViewModel,
    ListenerUpstreamTlsEvidenceViewModel, ListenerUpstreamTlsTestViewModel, ProxyListener,
    ProxyWorkspace, SocketTransportMode as ApplicationSocketMode, UiTone,
};
use intercept_proxy_domain::{
    SocketRelaySecurity as DomainSocketSecurity, SocketTopology as DomainSocketTopology,
};
use intercept_proxy_runtime::{SocketRelayMetricsSnapshot, UpstreamScheme, UpstreamTransport};
use parking_lot::RwLock;
use tokio_util::sync::CancellationToken;

use super::{
    ListenerRuntimeAdapter, ListenerRuntimePlanBuilder, PreparedListenerRuntime, RunningListener,
    bind_tcp_listener, running_status, upstream_tls_test_error,
};

struct StatusSnapshot {
    listener_id: ListenerId,
    finished: bool,
    listen_address: String,
    fault_reason: Option<String>,
    socket_service: Option<Arc<intercept_proxy_runtime::SocketRelayService>>,
}

#[async_trait]
impl ListenerRuntimePort for ListenerRuntimeAdapter {
    async fn statuses(&self) -> AppResult<Vec<ListenerStatusViewModel>> {
        let snapshots = {
            let running = self.running.lock().await;
            running
                .iter()
                .map(|(id, handle)| StatusSnapshot {
                    listener_id: *id,
                    finished: handle.task.is_finished(),
                    listen_address: handle.listen_address.clone(),
                    fault_reason: handle.fault.read().clone(),
                    socket_service: handle.socket_service.clone(),
                })
                .collect::<Vec<_>>()
        };
        let mut statuses = Vec::with_capacity(snapshots.len());
        for snapshot in snapshots {
            let metrics = match &snapshot.socket_service {
                Some(service) => service.metrics().await,
                None => SocketRelayMetricsSnapshot::default(),
            };
            statuses.push(status_from_snapshot(snapshot, metrics));
        }
        Ok(statuses)
    }

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
        let runtime_epoch = self.runtime_epoch_for_start(workspace.id);
        let plan = ListenerRuntimePlanBuilder::new(self)
            .build(&workspace, &listener, runtime_epoch)
            .await?;
        let scripted_snapshot = plan.scripted_snapshot();
        if let Some(snapshot) = plan.scripted_snapshot()
            && matches!(snapshot.topology(), DomainSocketTopology::LocalResponder(_))
            && matches!(
                &plan,
                PreparedListenerRuntime::ScriptedSocket { service: None, .. }
            )
        {
            // T25 之后 LocalResponder 必须在 bind 前已经持有专用本地 service。继续保留这个
            // 防御门禁，避免未来计划重构时把“已校验但不可服务”的旧占位状态重新暴露为 Running。
            return Err(AppError::new(
                "LOCAL_RESPONDER_PLAN_INVALID",
                "LocalResponder 未能构造本地应答运行服务。",
            )
            .entity(listener_id.to_string()));
        }
        let listen_address = plan.bind_addr().to_string();
        let tcp_listener = bind_tcp_listener(plan.bind_addr(), listener_id).await?;
        let cancellation = CancellationToken::new();
        let task_cancellation = cancellation.clone();
        let fault = Arc::new(RwLock::new(None));
        let task_fault = Arc::clone(&fault);
        let socket_service = match &plan {
            PreparedListenerRuntime::Socket { service, .. }
            | PreparedListenerRuntime::ScriptedSocket {
                service: Some(service),
                ..
            } => Some(Arc::clone(service)),
            _ => None,
        };
        let task = tokio::spawn(async move {
            let result = serve_prepared_listener(
                plan,
                tcp_listener,
                listener_id,
                runtime_epoch,
                task_cancellation,
            )
            .await;
            if let Err(error) = result
                && !is_orderly_stop(error.code)
            {
                *task_fault.write() = Some(error.message);
            }
        });
        let mut running = self.running.lock().await;
        if running.contains_key(&listener_id) {
            cancellation.cancel();
            task.abort();
            return Err(
                AppError::new("LISTENER_ALREADY_RUNNING", "Listener 已在运行。")
                    .entity(listener_id.to_string()),
            );
        }
        running.insert(
            listener_id,
            RunningListener {
                cancellation,
                task,
                listen_address: listen_address.clone(),
                fault,
                workspace,
                socket_service,
                scripted_snapshot,
            },
        );
        Ok(running_status(listener_id, listen_address))
    }

    async fn replace_socket_rules(
        &self,
        workspace: ProxyWorkspace,
        listener_id: ListenerId,
    ) -> AppResult<()> {
        let snapshot = self
            .running
            .lock()
            .await
            .get(&listener_id)
            .and_then(|running| running.scripted_snapshot.clone());
        let Some(snapshot) = snapshot else {
            return Ok(());
        };
        let listener = workspace
            .listeners
            .iter()
            .find(|listener| listener.id == listener_id)
            .ok_or_else(|| {
                AppError::new("LISTENER_NOT_FOUND", "入口配置不存在。")
                    .entity(listener_id.to_string())
            })?;
        snapshot.replace_document_rules(&workspace, listener)?;

        for running in self
            .running
            .lock()
            .await
            .values_mut()
            .filter(|running| running.workspace.id == workspace.id)
        {
            running.workspace = workspace.clone();
        }
        Ok(())
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
        ensure_upstream_tls_enabled(&listener)?;
        let result = self.test_upstream_connection(workspace, listener).await?;
        let tls = result.tls.ok_or_else(|| {
            AppError::new("UPSTREAM_TLS_NOT_ENABLED", "该入口的上游连接没有启用 TLS。")
                .entity(result.listener_id.to_string())
        })?;
        Ok(ListenerUpstreamTlsTestViewModel {
            listener_id: result.listener_id,
            upstream_origin: result.upstream_origin,
            resolved_address: result.resolved_address,
            tls_version: tls.tls_version,
            cipher_suite: tls.cipher_suite,
            peer_subject: tls.peer_subject,
            peer_sha256_fingerprint: tls.peer_sha256_fingerprint,
            hostname_verification_enabled: tls.hostname_verification_enabled,
            client_identity_configured: tls.client_identity_configured,
            elapsed_millis: result.elapsed_millis,
            message: "上游 Server TLS 握手成功。".into(),
            ui_tone: UiTone::Positive,
        })
    }

    async fn test_upstream_connection(
        &self,
        workspace: ProxyWorkspace,
        listener: ProxyListener,
    ) -> AppResult<ListenerUpstreamConnectionTestViewModel> {
        workspace.validate().map_err(AppError::from)?;
        let runtime_epoch = uuid::Uuid::new_v4();
        let plan = ListenerRuntimePlanBuilder::new(self)
            .build_probe(&workspace, &listener, runtime_epoch)
            .await?;
        match plan {
            PreparedListenerRuntime::HttpFixed { service, .. } => {
                let result = service
                    .test_upstream_connection()
                    .await
                    .map_err(|error| upstream_tls_test_error(listener.id, &error))?;
                Ok(http_probe_view(&listener, result))
            }
            PreparedListenerRuntime::Socket { service, .. } => {
                let result = service
                    .test_upstream_connection()
                    .await
                    .map_err(|error| upstream_tls_test_error(listener.id, &error))?;
                socket_probe_view(&listener, result)
            }
            PreparedListenerRuntime::ScriptedSocket { .. } => Err(AppError::new(
                "LISTENER_CONNECTION_TEST_UNSUPPORTED",
                "Scripted Socket 运行计划不能直接作为上游探测服务。",
            )
            .entity(listener.id.to_string())),
            PreparedListenerRuntime::HttpForward { .. } => Err(AppError::new(
                "FIXED_SERVER_NOT_CONFIGURED",
                "该代理监听未配置固定 Server，没有上游连接可测试。",
            )
            .entity(listener.id.to_string())),
        }
    }
}

async fn serve_prepared_listener(
    plan: PreparedListenerRuntime,
    tcp_listener: tokio::net::TcpListener,
    listener_id: ListenerId,
    runtime_epoch: uuid::Uuid,
    cancellation: CancellationToken,
) -> Result<(), intercept_proxy_runtime::ProxyError> {
    match plan {
        PreparedListenerRuntime::HttpForward { service, .. } => {
            service.serve_listener(tcp_listener, cancellation).await
        }
        PreparedListenerRuntime::HttpFixed { service, .. } => {
            service
                .serve_listener_with_epoch(tcp_listener, runtime_epoch, cancellation)
                .await
        }
        PreparedListenerRuntime::Socket { service, .. }
        | PreparedListenerRuntime::ScriptedSocket {
            service: Some(service),
            ..
        } => {
            service
                .serve_listener_with_context(
                    tcp_listener,
                    intercept_proxy_runtime::SocketRelayRunContext {
                        listener_id: listener_id.to_string(),
                        workspace_runtime_epoch: runtime_epoch,
                        listener_run_epoch: uuid::Uuid::new_v4(),
                    },
                    cancellation,
                )
                .await
        }
        PreparedListenerRuntime::ScriptedSocket { service: None, .. } => {
            Err(intercept_proxy_runtime::ProxyError::new(
                intercept_proxy_runtime::ErrorCode::Internal,
                "scripted socket plan reached serve without a runtime service",
            ))
        }
    }
}

fn status_from_snapshot(
    snapshot: StatusSnapshot,
    metrics: SocketRelayMetricsSnapshot,
) -> ListenerStatusViewModel {
    let faulted = snapshot.finished || snapshot.fault_reason.is_some();
    ListenerStatusViewModel {
        listener_id: snapshot.listener_id,
        state: if faulted {
            ListenerRuntimeState::Faulted
        } else {
            ListenerRuntimeState::Running
        },
        state_text: if faulted { "故障" } else { "运行中" }.into(),
        ui_tone: if faulted {
            UiTone::Danger
        } else {
            UiTone::Positive
        },
        listen_address: snapshot.listen_address,
        fault_reason: snapshot
            .fault_reason
            .or_else(|| faulted.then(|| "Listener 任务已意外结束。".into())),
        can_start: false,
        can_stop: true,
        active_connections: u32::try_from(metrics.active_connections).unwrap_or(u32::MAX),
        client_to_server_bytes: metrics.client_to_server_bytes,
        server_to_client_bytes: metrics.server_to_client_bytes,
        retained_diagnostic_evictions: metrics.retained_diagnostic_evictions,
    }
}

fn http_probe_view(
    listener: &ProxyListener,
    result: intercept_proxy_runtime::UpstreamConnectionTestResult,
) -> ListenerUpstreamConnectionTestViewModel {
    let tls = result.tls.map(tls_evidence_view);
    let scheme = match result.scheme {
        UpstreamScheme::Http => "http",
        UpstreamScheme::Https => "https",
    };
    let transport = match result.transport {
        UpstreamTransport::Tcp => "tcp",
        UpstreamTransport::Tls => "tls",
    };
    let origin = listener
        .http()
        .and_then(|http| http.fixed_server.as_ref())
        .map_or_else(String::new, |fixed| fixed.upstream_url.clone());
    ListenerUpstreamConnectionTestViewModel {
        listener_id: listener.id,
        data_plane: ListenerDataPlaneKind::Http,
        upstream_origin: origin,
        resolved_address: result.resolved_address.to_string(),
        scheme: scheme.into(),
        transport: transport.into(),
        tls,
        socket_transport_mode: None,
        elapsed_millis: result.elapsed_millis,
        message: format!("上游 Server {transport} 连接成功。"),
        ui_tone: UiTone::Positive,
    }
}

fn socket_probe_view(
    listener: &ProxyListener,
    result: intercept_proxy_runtime::SocketUpstreamConnectionTestResult,
) -> AppResult<ListenerUpstreamConnectionTestViewModel> {
    let Some(settings) = listener.socket() else {
        return Err(AppError::new(
            "LISTENER_DATA_PLANE_MISMATCH",
            "Socket 上游测试结果与 Listener 数据面不匹配。",
        )
        .entity(listener.id.to_string()));
    };
    let DomainSocketTopology::Relay(relay) = &settings.topology else {
        return Err(AppError::new(
            "LISTENER_UPSTREAM_NOT_APPLICABLE",
            "本地应答没有可测试的 Server 上游。",
        )
        .entity(listener.id.to_string()));
    };
    let transport = match result.transport {
        intercept_proxy_runtime::SocketUpstreamTransport::Tcp => "tcp",
        intercept_proxy_runtime::SocketUpstreamTransport::Tls => "tls",
    };
    Ok(ListenerUpstreamConnectionTestViewModel {
        listener_id: listener.id,
        data_plane: ListenerDataPlaneKind::Socket,
        upstream_origin: format!("{}:{}", relay.upstream.host, relay.upstream.port),
        resolved_address: result.resolved_address.to_string(),
        scheme: "socket".into(),
        transport: transport.into(),
        tls: result.tls.map(socket_tls_evidence_view),
        socket_transport_mode: Some(socket_mode(&relay.security)),
        elapsed_millis: result.elapsed_millis,
        message: format!("上游 Socket {transport} 连接成功。"),
        ui_tone: UiTone::Positive,
    })
}

fn tls_evidence_view(
    evidence: intercept_proxy_runtime::UpstreamTlsHandshakeResult,
) -> ListenerUpstreamTlsEvidenceViewModel {
    ListenerUpstreamTlsEvidenceViewModel {
        tls_version: evidence.tls_version,
        cipher_suite: evidence.cipher_suite,
        peer_subject: evidence.peer_subject,
        peer_sha256_fingerprint: evidence.peer_sha256_fingerprint,
        hostname_verification_enabled: evidence.hostname_verification_enabled,
        client_identity_configured: evidence.client_identity_configured,
    }
}

fn socket_tls_evidence_view(
    evidence: intercept_proxy_runtime::SocketTlsEvidence,
) -> ListenerUpstreamTlsEvidenceViewModel {
    ListenerUpstreamTlsEvidenceViewModel {
        tls_version: evidence.tls_version,
        cipher_suite: evidence.cipher_suite,
        peer_subject: evidence.peer_subject,
        peer_sha256_fingerprint: evidence.peer_sha256_fingerprint,
        hostname_verification_enabled: evidence.hostname_verification_enabled,
        client_identity_configured: evidence.client_identity_configured,
    }
}

fn socket_mode(security: &DomainSocketSecurity) -> ApplicationSocketMode {
    match security {
        DomainSocketSecurity::Transparent => ApplicationSocketMode::Transparent,
        DomainSocketSecurity::TcpToTls { .. } => ApplicationSocketMode::TcpToTls,
        DomainSocketSecurity::TlsToTcp { .. } => ApplicationSocketMode::TlsToTcp,
        DomainSocketSecurity::TlsToTls { .. } => ApplicationSocketMode::TlsToTls,
    }
}

fn ensure_upstream_tls_enabled(listener: &ProxyListener) -> AppResult<()> {
    let enabled = match &listener.data_plane {
        intercept_proxy_domain::ListenerDataPlane::Http(http) => http
            .fixed_server
            .as_ref()
            .ok_or_else(|| {
                AppError::new(
                    "FIXED_SERVER_NOT_CONFIGURED",
                    "该代理监听未配置固定 Server，没有上游 TLS 可测试。",
                )
                .entity(listener.id.to_string())
            })?
            .upstream_url
            .starts_with("https://"),
        intercept_proxy_domain::ListenerDataPlane::Socket(socket) => match &socket.topology {
            DomainSocketTopology::Relay(relay) => matches!(
                relay.security,
                DomainSocketSecurity::TcpToTls { .. } | DomainSocketSecurity::TlsToTls { .. }
            ),
            DomainSocketTopology::LocalResponder(_) => {
                return Err(AppError::new(
                    "LISTENER_UPSTREAM_NOT_APPLICABLE",
                    "本地应答没有可测试的 Server 上游 TLS。",
                )
                .entity(listener.id.to_string()));
            }
        },
    };
    if enabled {
        Ok(())
    } else {
        Err(
            AppError::new("UPSTREAM_TLS_NOT_ENABLED", "该入口的上游连接没有启用 TLS。")
                .entity(listener.id.to_string()),
        )
    }
}

fn is_orderly_stop(code: &'static str) -> bool {
    matches!(
        code,
        "PROXY_STOPPED" | "BREAKPOINT_PROXY_STOPPED" | "SOCKET_RELAY_CANCELLED"
    )
}

#[cfg(test)]
mod status_tests {
    use super::*;

    #[test]
    fn socket_diagnostic_drop_count_is_exposed_in_listener_status() {
        let listener_id = ListenerId::new();
        let status = status_from_snapshot(
            StatusSnapshot {
                listener_id,
                finished: false,
                listen_address: "127.0.0.1:1234".into(),
                fault_reason: None,
                socket_service: None,
            },
            SocketRelayMetricsSnapshot {
                retained_diagnostic_evictions: 7,
                ..SocketRelayMetricsSnapshot::default()
            },
        );
        assert_eq!(status.listener_id, listener_id);
        assert_eq!(status.retained_diagnostic_evictions, 7);
    }
}
