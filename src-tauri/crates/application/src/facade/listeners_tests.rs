use intercept_proxy_domain::{
    FixedServerSettings, HttpListenerSettings, ListenerDataPlane, UpstreamTlsSettings,
};

use crate::ListenerRuntimeState;

use super::*;

#[test]
fn copied_listener_preserves_fixed_server_and_stops_by_default() {
    let original_id = ListenerId::new();
    let source = ProxyListener {
        id: original_id,
        name: "Transaction".into(),
        enabled: true,
        bind_address: "0.0.0.0".into(),
        port: 16_627,
        data_plane: ListenerDataPlane::Http(HttpListenerSettings {
            fixed_server: Some(FixedServerSettings {
                upstream_url: "https://transaction.example.test:16627".into(),
                upstream_tls: UpstreamTlsSettings::default(),
            }),
            ..HttpListenerSettings::default()
        }),
        ..ProxyListener::default()
    };

    let copy = copy_listener_draft(source);
    assert_ne!(copy.id, original_id);
    assert_eq!(copy.name, "Transaction 副本");
    assert!(!copy.enabled);
    assert_eq!(copy.port, 16_627);
    assert_eq!(
        copy.http()
            .unwrap()
            .fixed_server
            .as_ref()
            .unwrap()
            .upstream_url,
        "https://transaction.example.test:16627"
    );
}

#[test]
fn overview_uses_workspace_as_the_only_listener_catalog() {
    let mut workspace = ProxyWorkspace::default();
    let forward_id = workspace.listeners[0].id;
    workspace.listeners.push(ProxyListener {
        id: ListenerId::new(),
        name: "API 固定上游".into(),
        enabled: false,
        bind_address: "127.0.0.1".into(),
        port: 9_001,
        data_plane: ListenerDataPlane::Http(HttpListenerSettings {
            fixed_server: Some(FixedServerSettings {
                upstream_url: "https://api.example.test:9443".into(),
                upstream_tls: UpstreamTlsSettings::default(),
            }),
            ..HttpListenerSettings::default()
        }),
        ..ProxyListener::default()
    });
    let overview = build_listener_overview(
        workspace,
        vec![ListenerStatusViewModel {
            listener_id: forward_id,
            state: ListenerRuntimeState::Running,
            state_text: "运行中".into(),
            ui_tone: UiTone::Positive,
            listen_address: "127.0.0.1:8080".into(),
            fault_reason: None,
            can_start: false,
            can_stop: true,
            active_connections: 1,
            client_to_server_bytes: 11,
            server_to_client_bytes: 22,
            retained_diagnostic_evictions: 0,
        }],
    );

    assert_eq!(overview.total_count, 2);
    assert_eq!(overview.active_count, 1);
    assert_eq!(overview.state_text, "部分入口运行中");
    assert_eq!(overview.rows[0].request_destination, "请求中的目标地址");
    assert!(!overview.rows[0].can_start);
    assert!(overview.rows[0].can_stop);
    assert_eq!(overview.rows[1].state_text, "已停止");
    assert!(overview.rows[1].can_start);
    assert!(!overview.rows[1].can_stop);
    assert_eq!(
        overview.rows[1].request_destination,
        "https://api.example.test:9443"
    );
}

#[test]
fn overview_preserves_faulted_listener_stop_capability() {
    let workspace = ProxyWorkspace::default();
    let listener_id = workspace.listeners[0].id;
    let overview = build_listener_overview(
        workspace,
        vec![ListenerStatusViewModel {
            listener_id,
            state: ListenerRuntimeState::Faulted,
            state_text: "故障".into(),
            ui_tone: UiTone::Danger,
            listen_address: "127.0.0.1:8080".into(),
            fault_reason: Some("Listener 任务已意外结束。".into()),
            can_start: false,
            can_stop: true,
            active_connections: 0,
            client_to_server_bytes: 0,
            server_to_client_bytes: 0,
            retained_diagnostic_evictions: 0,
        }],
    );

    assert_eq!(overview.faulted_count, 1);
    assert_eq!(overview.rows[0].state, ListenerRuntimeState::Faulted);
    assert!(!overview.rows[0].can_start);
    assert!(overview.rows[0].can_stop);
}
