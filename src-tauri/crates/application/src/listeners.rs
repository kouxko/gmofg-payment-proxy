//! `ListenerRuntimePort` 的无网络内存实现，仅用于无 UI 用例测试。

use std::collections::BTreeMap;

use async_trait::async_trait;
use parking_lot::RwLock;

use crate::{
    AppError, AppResult, ListenerId, ListenerRuntimePort, ListenerRuntimeState,
    ListenerStatusViewModel, ListenerUpstreamTlsTestViewModel, ProxyListener, ProxyWorkspace,
    ReverseProxyListener, UiTone,
};

#[derive(Debug, Default)]
pub struct InMemoryListenerRuntime {
    statuses: RwLock<BTreeMap<ListenerId, ListenerStatusViewModel>>,
}

#[async_trait]
impl ListenerRuntimePort for InMemoryListenerRuntime {
    async fn statuses(&self) -> AppResult<Vec<ListenerStatusViewModel>> {
        Ok(self.statuses.read().values().cloned().collect())
    }

    async fn start(
        &self,
        workspace: ProxyWorkspace,
        listener: ProxyListener,
    ) -> AppResult<ListenerStatusViewModel> {
        let id = listener.id();
        if !workspace.listeners.iter().any(|item| item == &listener) {
            return Err(
                AppError::new("LISTENER_NOT_FOUND", "启动快照中不存在该代理入口。")
                    .entity(id.to_string()),
            );
        }
        if self.statuses.read().contains_key(&id) {
            return Err(
                AppError::new("LISTENER_ALREADY_RUNNING", "Listener 已在运行。")
                    .entity(id.to_string()),
            );
        }
        let (address, port) = listener.bind_endpoint();
        let status = ListenerStatusViewModel {
            listener_id: id,
            state: ListenerRuntimeState::Running,
            state_text: "运行中".into(),
            ui_tone: UiTone::Positive,
            listen_address: format!("{address}:{port}"),
            fault_reason: None,
            can_start: false,
            can_stop: true,
        };
        self.statuses.write().insert(id, status.clone());
        Ok(status)
    }

    async fn stop(&self, listener_id: ListenerId) -> AppResult<ListenerStatusViewModel> {
        let running = self.statuses.write().remove(&listener_id).ok_or_else(|| {
            AppError::new("LISTENER_NOT_RUNNING", "Listener 当前未运行。")
                .entity(listener_id.to_string())
        })?;
        Ok(ListenerStatusViewModel {
            listener_id,
            state: ListenerRuntimeState::Stopped,
            state_text: "已停止".into(),
            ui_tone: UiTone::Neutral,
            listen_address: running.listen_address,
            fault_reason: None,
            can_start: true,
            can_stop: false,
        })
    }

    async fn test_upstream_tls(
        &self,
        _workspace: ProxyWorkspace,
        listener: ReverseProxyListener,
    ) -> AppResult<ListenerUpstreamTlsTestViewModel> {
        if !listener.upstream_url.starts_with("https://") {
            return Err(AppError::new(
                "UPSTREAM_TLS_NOT_ENABLED",
                "该入口使用 HTTP 上游，没有 TLS 握手可测试。",
            )
            .entity(listener.id.to_string()));
        }
        Ok(ListenerUpstreamTlsTestViewModel {
            listener_id: listener.id,
            upstream_origin: listener.upstream_url,
            resolved_address: "127.0.0.1:443".into(),
            tls_version: "TLS 1.2".into(),
            cipher_suite: "测试密码套件".into(),
            peer_subject: "CN=测试上游".into(),
            peer_sha256_fingerprint: "00:11:22".into(),
            hostname_verification_enabled: listener.upstream_tls.verify_hostname,
            client_identity_configured: listener.upstream_tls.client_identity.is_some(),
            elapsed_millis: 1,
            message: "上游 Server TLS 握手成功。".into(),
            ui_tone: UiTone::Positive,
        })
    }
}
