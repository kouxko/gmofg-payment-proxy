//! Listener 草稿、证书引用与监控快照的纯领域辅助逻辑。

use std::collections::BTreeMap;

use crate::{
    AppError, AppResult, CertificateReference, ListenerDataPlane, ListenerId,
    ListenerMonitorRowViewModel, ListenerOverviewViewModel, ListenerRuntimePort,
    ListenerRuntimeState, ListenerStatusViewModel, MANAGED_LISTENER_CERTIFICATE_PREFIX,
    ProxyListener, ProxyWorkspace, SocketPayloadProcessing, SocketRelaySecurity, SocketTopology,
    UiTone,
};

/// 证书导入会先产生受基础设施管理的不可变引用，再由当前监听保存动作把引用并入
/// Workspace。已有同 ID 引用始终以持久化值为准，避免前端借保存监听修改证书来源。
pub(super) fn merge_new_certificate_references(
    current: &mut Vec<CertificateReference>,
    imported: Vec<CertificateReference>,
) {
    for reference in imported {
        if current.iter().all(|item| item.id != reference.id) {
            current.push(reference);
        }
    }
}

pub(super) async fn validate_new_certificate_references(
    certificates: &dyn crate::ListenerCertificateImportPort,
    current: &[CertificateReference],
    imported: &[CertificateReference],
) -> AppResult<()> {
    for reference in imported {
        if current.iter().any(|item| item.id == reference.id) {
            continue;
        }
        if !reference
            .reference
            .starts_with(MANAGED_LISTENER_CERTIFICATE_PREFIX)
        {
            return Err(AppError::new(
                "LISTENER_CERTIFICATE_REFERENCE_UNTRUSTED",
                concat!(
                    "监听证书必须通过应用内的原生导入功能创建，",
                    "不能保存文件路径或外部密码引用。"
                ),
            )
            .entity(reference.id.to_string()));
        }
        // `inspect` 同时证明受保护材料仍然存在且可以按其证书角色解析。普通保存命令
        // 不能仅凭前端提交的字符串把一个不存在的托管引用写入 Workspace。
        certificates.inspect(reference.clone()).await?;
    }
    Ok(())
}

pub(super) async fn ensure_listener_not_running(
    runtime: &dyn ListenerRuntimePort,
    listener_id: ListenerId,
) -> AppResult<()> {
    if runtime
        .statuses()
        .await?
        .iter()
        .any(|status| status.listener_id == listener_id)
    {
        return Err(AppError::new(
            "LISTENER_RUNTIME_ACTIVE",
            "Listener 正在运行；请停止后再保存或删除配置。",
        )
        .entity(listener_id.to_string()));
    }
    Ok(())
}

pub(super) fn build_listener_overview(
    workspace: ProxyWorkspace,
    statuses: Vec<ListenerStatusViewModel>,
) -> ListenerOverviewViewModel {
    let mut statuses = statuses
        .into_iter()
        .map(|status| (status.listener_id, status))
        .collect::<BTreeMap<_, _>>();
    let rows = workspace
        .listeners
        .iter()
        .map(|listener| {
            let id = listener.id;
            let (address, port) = listener.bind_endpoint();
            let status = statuses.remove(&id).unwrap_or(ListenerStatusViewModel {
                listener_id: id,
                state: ListenerRuntimeState::Stopped,
                state_text: "已停止".into(),
                ui_tone: UiTone::Neutral,
                listen_address: format!("{address}:{port}"),
                fault_reason: None,
                can_start: true,
                can_stop: false,
                active_connections: 0,
                client_to_server_bytes: 0,
                server_to_client_bytes: 0,
                retained_diagnostic_evictions: 0,
            });
            let (kind_text, request_destination) = listener_presentation(listener);
            ListenerMonitorRowViewModel {
                listener_id: id,
                name: listener.name.clone(),
                kind_text,
                listen_address: status.listen_address,
                request_destination,
                state: status.state,
                state_text: status.state_text,
                ui_tone: status.ui_tone,
                fault_reason: status.fault_reason,
                can_start: status.can_start,
                can_stop: status.can_stop,
                active_connections: status.active_connections,
                client_to_server_bytes: status.client_to_server_bytes,
                server_to_client_bytes: status.server_to_client_bytes,
            }
        })
        .collect::<Vec<_>>();
    let active_count = rows
        .iter()
        .filter(|row| {
            matches!(
                row.state,
                ListenerRuntimeState::Starting
                    | ListenerRuntimeState::Running
                    | ListenerRuntimeState::Stopping
            )
        })
        .count();
    let faulted_count = rows
        .iter()
        .filter(|row| row.state == ListenerRuntimeState::Faulted)
        .count();
    let total_count = rows.len();
    let (state_text, ui_tone) = if faulted_count > 0 {
        ("部分入口故障".into(), UiTone::Danger)
    } else if total_count == 0 {
        ("未配置入口".into(), UiTone::Neutral)
    } else if active_count == total_count {
        ("全部入口运行中".into(), UiTone::Positive)
    } else if active_count > 0 {
        ("部分入口运行中".into(), UiTone::Warning)
    } else {
        ("全部入口已停止".into(), UiTone::Neutral)
    };
    ListenerOverviewViewModel {
        workspace_id: workspace.id,
        workspace_name: workspace.name,
        state_text,
        ui_tone,
        total_count,
        active_count,
        faulted_count,
        rows,
    }
}

fn listener_presentation(listener: &ProxyListener) -> (String, String) {
    match &listener.data_plane {
        ListenerDataPlane::Http(settings) => settings.fixed_server.as_ref().map_or_else(
            || ("HTTP · 动态目标".to_owned(), "请求中的目标地址".to_owned()),
            |fixed| ("HTTP · 固定 Server".to_owned(), fixed.upstream_url.clone()),
        ),
        ListenerDataPlane::Socket(settings) => {
            let SocketTopology::Relay(relay) = &settings.topology else {
                return ("Socket · 本地应答".to_owned(), "本地回环服务".to_owned());
            };
            let processing = match &settings.processing {
                SocketPayloadProcessing::Direct => "透明转发",
                SocketPayloadProcessing::Scripted(_) => "按协议转发",
            };
            let transport = match &relay.security {
                SocketRelaySecurity::Transparent => None,
                SocketRelaySecurity::TcpToTls { .. } => Some("TCP → TLS"),
                SocketRelaySecurity::TlsToTcp { .. } => Some("TLS → TCP"),
                SocketRelaySecurity::TlsToTls { .. } => Some("TLS → TLS"),
            };
            (
                transport.map_or_else(
                    || format!("Socket · {processing}"),
                    |transport| format!("Socket · {processing} · {transport}"),
                ),
                format!("{}:{}", relay.upstream.host, relay.upstream.port),
            )
        }
    }
}

pub(super) fn copy_listener_draft(mut source: ProxyListener) -> ProxyListener {
    source.id = ListenerId::new();
    source.name = format!("{} 副本", source.name.trim());
    source.enabled = false;
    source
}

pub(super) fn find_listener(
    listeners: &[ProxyListener],
    listener_id: ListenerId,
) -> AppResult<ProxyListener> {
    listeners
        .iter()
        .find(|listener| listener.id == listener_id)
        .cloned()
        .ok_or_else(|| listener_not_found(listener_id))
}

pub(super) fn set_listener_enabled(
    listeners: &mut [ProxyListener],
    listener_id: ListenerId,
    enabled: bool,
) -> AppResult<()> {
    let listener = listeners
        .iter_mut()
        .find(|listener| listener.id == listener_id)
        .ok_or_else(|| listener_not_found(listener_id))?;
    listener.enabled = enabled;
    Ok(())
}

pub(super) fn listener_not_found(listener_id: ListenerId) -> AppError {
    AppError::new("LISTENER_NOT_FOUND", "Listener 不存在或已被删除。")
        .entity(listener_id.to_string())
}

#[cfg(test)]
mod presentation_tests {
    use crate::{
        ListenerDataPlane, ScriptedSocketProcessing, SocketEndpoint, SocketLocalResponderTopology,
        SocketRelaySettings, SocketUpstreamTlsSettings, builtin_iso8583_package_ref,
    };

    use super::*;

    #[test]
    fn scripted_relay_reports_processing_instead_of_transport_as_transparent() {
        let listener = ProxyListener {
            data_plane: ListenerDataPlane::Socket(SocketRelaySettings::relay(
                SocketEndpoint {
                    host: "127.0.0.1".into(),
                    port: 19_081,
                },
                SocketRelaySecurity::Transparent,
                20,
                SocketPayloadProcessing::Scripted(ScriptedSocketProcessing {
                    package: builtin_iso8583_package_ref(),
                }),
            )),
            ..ProxyListener::default()
        };

        assert_eq!(
            listener_presentation(&listener),
            ("Socket · 按协议转发".into(), "127.0.0.1:19081".into())
        );
    }

    #[test]
    fn relay_security_is_appended_without_hiding_processing_mode() {
        let listener = ProxyListener {
            data_plane: ListenerDataPlane::Socket(SocketRelaySettings::relay(
                SocketEndpoint {
                    host: "upstream.test".into(),
                    port: 443,
                },
                SocketRelaySecurity::TcpToTls {
                    upstream_tls: SocketUpstreamTlsSettings::default(),
                },
                20,
                SocketPayloadProcessing::Direct,
            )),
            ..ProxyListener::default()
        };

        assert_eq!(
            listener_presentation(&listener),
            (
                "Socket · 透明转发 · TCP → TLS".into(),
                "upstream.test:443".into()
            )
        );
    }

    #[test]
    fn local_responder_is_presented_as_the_supported_local_service() {
        let listener = ProxyListener {
            data_plane: ListenerDataPlane::Socket(SocketRelaySettings {
                topology: SocketTopology::LocalResponder(SocketLocalResponderTopology::default()),
                ..SocketRelaySettings::default()
            }),
            ..ProxyListener::default()
        };

        assert_eq!(
            listener_presentation(&listener),
            ("Socket · 本地应答".into(), "本地回环服务".into())
        );
    }
}
