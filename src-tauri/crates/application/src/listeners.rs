//! `ListenerRuntimePort` 的无网络内存实现，仅用于无 UI 用例测试。

use std::collections::BTreeMap;

use async_trait::async_trait;
use parking_lot::RwLock;

use crate::{
    AppError, AppResult, ListenerDataPlane, ListenerDataPlaneKind, ListenerId, ListenerRuntimePort,
    ListenerRuntimeState, ListenerStatusViewModel, ListenerUpstreamConnectionTestViewModel,
    ListenerUpstreamTlsEvidenceViewModel, ListenerUpstreamTlsTestViewModel, ProxyListener,
    ProxyWorkspace, SocketRelaySecurity, SocketTransportMode, UiTone,
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
        let id = listener.id;
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
            active_connections: 0,
            client_to_server_bytes: 0,
            server_to_client_bytes: 0,
            retained_diagnostic_evictions: 0,
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
            active_connections: 0,
            client_to_server_bytes: running.client_to_server_bytes,
            server_to_client_bytes: running.server_to_client_bytes,
            retained_diagnostic_evictions: running.retained_diagnostic_evictions,
        })
    }

    async fn test_upstream_tls(
        &self,
        _workspace: ProxyWorkspace,
        listener: ProxyListener,
    ) -> AppResult<ListenerUpstreamTlsTestViewModel> {
        let target = upstream_tls_target(&listener)?;
        Ok(ListenerUpstreamTlsTestViewModel {
            listener_id: listener.id,
            upstream_origin: target.address,
            resolved_address: "127.0.0.1:443".into(),
            tls_version: "TLS 1.2".into(),
            cipher_suite: "测试密码套件".into(),
            peer_subject: "CN=测试上游".into(),
            peer_sha256_fingerprint: "00:11:22".into(),
            hostname_verification_enabled: target.verify_hostname,
            client_identity_configured: target.client_identity_configured,
            elapsed_millis: 1,
            message: "上游 Server TLS 握手成功。".into(),
            ui_tone: UiTone::Positive,
        })
    }

    async fn test_upstream_connection(
        &self,
        _workspace: ProxyWorkspace,
        listener: ProxyListener,
    ) -> AppResult<ListenerUpstreamConnectionTestViewModel> {
        let target = connection_target(&listener)?;
        Ok(ListenerUpstreamConnectionTestViewModel {
            listener_id: listener.id,
            data_plane: target.data_plane,
            upstream_origin: target.address,
            resolved_address: if target.uses_tls {
                "127.0.0.1:443"
            } else {
                "127.0.0.1:80"
            }
            .into(),
            scheme: target.scheme,
            transport: if target.uses_tls { "tls" } else { "tcp" }.into(),
            tls: target
                .uses_tls
                .then(|| ListenerUpstreamTlsEvidenceViewModel {
                    tls_version: "TLS 1.2".into(),
                    cipher_suite: "测试密码套件".into(),
                    peer_subject: "CN=测试上游".into(),
                    peer_sha256_fingerprint: "00:11:22".into(),
                    hostname_verification_enabled: target.verify_hostname,
                    client_identity_configured: target.client_identity_configured,
                }),
            socket_transport_mode: target.socket_transport_mode,
            elapsed_millis: 1,
            message: "上游 Server 连接成功。".into(),
            ui_tone: UiTone::Positive,
        })
    }
}

struct TestTarget {
    address: String,
    scheme: String,
    data_plane: ListenerDataPlaneKind,
    uses_tls: bool,
    verify_hostname: bool,
    client_identity_configured: bool,
    socket_transport_mode: Option<SocketTransportMode>,
}

fn connection_target(listener: &ProxyListener) -> AppResult<TestTarget> {
    match &listener.data_plane {
        ListenerDataPlane::Http(settings) => {
            let fixed = settings.fixed_server.as_ref().ok_or_else(|| {
                unsupported_connection_test(listener.id, "该 HTTP 监听器未开启固定 Server。")
            })?;
            let uses_tls = fixed.upstream_url.starts_with("https://");
            Ok(TestTarget {
                address: fixed.upstream_url.clone(),
                scheme: if uses_tls { "https" } else { "http" }.into(),
                data_plane: ListenerDataPlaneKind::Http,
                uses_tls,
                verify_hostname: fixed.upstream_tls.verify_hostname,
                client_identity_configured: fixed.upstream_tls.client_identity.is_some(),
                socket_transport_mode: None,
            })
        }
        ListenerDataPlane::Socket(settings) => {
            let (mode, upstream_tls) = match &settings.security {
                SocketRelaySecurity::Transparent => (SocketTransportMode::Transparent, None),
                SocketRelaySecurity::TcpToTls { upstream_tls } => {
                    (SocketTransportMode::TcpToTls, Some(upstream_tls))
                }
                SocketRelaySecurity::TlsToTcp { .. } => (SocketTransportMode::TlsToTcp, None),
                SocketRelaySecurity::TlsToTls { upstream_tls, .. } => {
                    (SocketTransportMode::TlsToTls, Some(upstream_tls))
                }
            };
            Ok(TestTarget {
                address: format!("{}:{}", settings.upstream.host, settings.upstream.port),
                scheme: "socket".into(),
                data_plane: ListenerDataPlaneKind::Socket,
                uses_tls: upstream_tls.is_some(),
                verify_hostname: upstream_tls.is_some_and(|tls| tls.verify_hostname),
                client_identity_configured: upstream_tls
                    .is_some_and(|tls| tls.client_identity.is_some()),
                socket_transport_mode: Some(mode),
            })
        }
    }
}

fn upstream_tls_target(listener: &ProxyListener) -> AppResult<TestTarget> {
    let target = connection_target(listener)?;
    if target.uses_tls {
        Ok(target)
    } else {
        Err(
            AppError::new("UPSTREAM_TLS_NOT_ENABLED", "该入口的上游连接没有启用 TLS。")
                .entity(listener.id.to_string()),
        )
    }
}

fn unsupported_connection_test(listener_id: ListenerId, message: &str) -> AppError {
    AppError::new("LISTENER_CONNECTION_TEST_UNSUPPORTED", message).entity(listener_id.to_string())
}

#[cfg(test)]
mod tests {
    use crate::{
        ListenerDataPlane, SocketEndpoint, SocketPayloadProcessing, SocketRelaySecurity,
        SocketRelaySettings, SocketUpstreamTlsSettings,
    };

    use super::*;

    #[test]
    fn socket_connection_probe_reports_transport_without_inventing_tls() {
        let listener = ProxyListener {
            data_plane: ListenerDataPlane::Socket(SocketRelaySettings {
                upstream: SocketEndpoint {
                    host: "socket.example.test".into(),
                    port: 16_127,
                },
                security: SocketRelaySecurity::Transparent,
                maximum_connections: 500,
                processing: SocketPayloadProcessing::Direct,
            }),
            ..ProxyListener::default()
        };

        let target = connection_target(&listener).unwrap();
        assert_eq!(target.data_plane, ListenerDataPlaneKind::Socket);
        assert_eq!(target.address, "socket.example.test:16127");
        assert!(!target.uses_tls);
        assert_eq!(
            target.socket_transport_mode,
            Some(SocketTransportMode::Transparent)
        );
    }

    #[test]
    fn socket_connection_probe_reports_only_upstream_tls_evidence() {
        let listener = ProxyListener {
            data_plane: ListenerDataPlane::Socket(SocketRelaySettings {
                upstream: SocketEndpoint {
                    host: "socket.example.test".into(),
                    port: 443,
                },
                security: SocketRelaySecurity::TcpToTls {
                    upstream_tls: SocketUpstreamTlsSettings {
                        verify_hostname: false,
                        server_trust: None,
                        client_identity: None,
                    },
                },
                maximum_connections: 500,
                processing: SocketPayloadProcessing::Direct,
            }),
            ..ProxyListener::default()
        };

        let target = upstream_tls_target(&listener).unwrap();
        assert!(target.uses_tls);
        assert!(!target.verify_hostname);
        assert_eq!(
            target.socket_transport_mode,
            Some(SocketTransportMode::TcpToTls)
        );
    }
}
