use std::sync::Arc;

use super::{
    ListenerRuntimeAdapter, ListenerRuntimePlanBuilder, PreparedListenerRuntime,
    upstream_tls_test_error,
};
use crate::adapters::external_package_server::ExternalPackageListenerRuntime;
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

mod status;

use status::{http_probe_view, socket_probe_view, status_from_snapshot};

struct StatusSnapshot {
    listener_id: ListenerId,
    runtime_epoch: uuid::Uuid,
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
                    runtime_epoch: handle.runtime_epoch,
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
        let _environment_apply_gate = self
            .environment_apply_resource_gates
            .acquire(
                super::super::environment_configuration_lease::EnvironmentApplyLeaseResourceKey::Listener(
                    listener_id.as_uuid(),
                ),
            )
            .await;
        workspace.validate().map_err(AppError::from)?;
        let runtime_epoch = self.reserve_start(workspace.id, listener_id).await?;
        self.finish_start_owned(workspace, listener, runtime_epoch)
            .await
    }

    async fn replace_rule_definitions(
        &self,
        workspace: ProxyWorkspace,
        listener_id: ListenerId,
    ) -> AppResult<()> {
        let _environment_apply_gate = self
            .environment_apply_resource_gates
            .acquire(
                super::super::environment_configuration_lease::EnvironmentApplyLeaseResourceKey::Listener(
                    listener_id.as_uuid(),
                ),
            )
            .await;
        let (socket_snapshot, external_socket_snapshot, http_snapshot) = self
            .running
            .lock()
            .await
            .get(&listener_id)
            .map_or((None, None, None), |running| {
                (
                    running.scripted_snapshot.clone(),
                    running.external_socket_snapshot.clone(),
                    running.http_protocol_snapshot.clone(),
                )
            });
        let listener = workspace
            .listeners
            .iter()
            .find(|listener| listener.id == listener_id)
            .ok_or_else(|| {
                AppError::new("LISTENER_NOT_FOUND", "入口配置不存在。")
                    .entity(listener_id.to_string())
            })?;
        if let Some(snapshot) = socket_snapshot {
            snapshot
                .replace_document_rules(self, &workspace, listener)
                .await?;
        }
        if let Some(snapshot) = external_socket_snapshot {
            snapshot
                .replace_document_rules(self, &workspace, listener)
                .await?;
        }
        if let Some(snapshot) = http_snapshot {
            snapshot
                .replace_document_rules(self, &workspace, listener)
                .await?;
        }

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
        let environment_apply_gate = self
            .environment_apply_resource_gates
            .acquire(
                super::super::environment_configuration_lease::EnvironmentApplyLeaseResourceKey::Listener(
                    listener_id.as_uuid(),
                ),
            )
            .await;
        let handle = {
            let mut running = self.running.lock().await;
            let handle = running.remove(&listener_id).ok_or_else(|| {
                AppError::new("LISTENER_NOT_RUNNING", "Listener 当前未运行。")
                    .entity(listener_id.to_string())
            })?;
            self.stopping.write().insert(
                handle.run_token,
                super::StoppingListener {
                    runtime_epoch: handle.runtime_epoch,
                },
            );
            let active_epoch_owned = running
                .values()
                .any(|candidate| candidate.runtime_epoch == handle.runtime_epoch)
                || self
                    .pending_starts
                    .read()
                    .values()
                    .any(|candidate| candidate.runtime_epoch == handle.runtime_epoch);
            if !active_epoch_owned {
                self.retire_runtime_epoch(handle.workspace.id, handle.runtime_epoch);
            }
            handle
        };
        self.finish_stopping_owned(listener_id, handle, environment_apply_gate)
            .await
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
            PreparedListenerRuntime::ExternalScriptedSocket { .. }
            | PreparedListenerRuntime::ScriptedSocket { .. } => Err(AppError::new(
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

#[async_trait]
impl ExternalPackageListenerRuntime for ListenerRuntimeAdapter {
    async fn current_run_token(&self, listener_id: ListenerId) -> Option<uuid::Uuid> {
        self.running
            .lock()
            .await
            .get(&listener_id)
            .map(|running| running.run_token)
    }

    async fn stop_if_run_token(
        &self,
        listener_id: ListenerId,
        expected_run_token: uuid::Uuid,
    ) -> AppResult<Option<ListenerStatusViewModel>> {
        let environment_apply_gate = self
            .environment_apply_resource_gates
            .acquire(
                super::super::environment_configuration_lease::EnvironmentApplyLeaseResourceKey::Listener(
                    listener_id.as_uuid(),
                ),
            )
            .await;
        let removed = {
            let mut running = self.running.lock().await;
            match running.get(&listener_id) {
                Some(current) if current.run_token == expected_run_token => {
                    let handle = running
                        .remove(&listener_id)
                        .expect("listener existence was checked while holding the running lock");
                    self.stopping.write().insert(
                        handle.run_token,
                        super::StoppingListener {
                            runtime_epoch: handle.runtime_epoch,
                        },
                    );
                    let active_epoch_owned = running
                        .values()
                        .any(|candidate| candidate.runtime_epoch == handle.runtime_epoch)
                        || self
                            .pending_starts
                            .read()
                            .values()
                            .any(|candidate| candidate.runtime_epoch == handle.runtime_epoch);
                    if !active_epoch_owned {
                        self.retire_runtime_epoch(handle.workspace.id, handle.runtime_epoch);
                    }
                    Some(handle)
                }
                Some(_) | None => None,
            }
        };
        let Some(handle) = removed else {
            return Ok(None);
        };
        Ok(Some(
            self.finish_stopping_owned(listener_id, handle, environment_apply_gate)
                .await?,
        ))
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
