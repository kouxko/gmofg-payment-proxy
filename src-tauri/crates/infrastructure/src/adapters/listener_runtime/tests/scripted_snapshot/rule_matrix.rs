//! Scripted 启动快照的方向能力矩阵与冻结规则执行回归。

use intercept_proxy_domain::{
    DocumentFieldName, DocumentValue, ListenerDataPlane, SocketDocumentRuleDefinition,
};
use intercept_proxy_runtime::SocketConnectionIdentity;

use super::*;

#[tokio::test]
async fn relay_rule_capability_matrix_is_enforced_by_the_real_snapshot_builder() {
    let cases = [
        (false, true, false, Some("SOCKET_RULE_DECODE_REQUIRED")),
        (true, false, false, None),
        (true, false, true, Some("SOCKET_RULE_ENCODE_REQUIRED")),
        (true, true, true, None),
    ];

    for (decode_enabled, encode_enabled, modifies, expected_error) in cases {
        let store = Arc::new(SqliteStore::in_memory().unwrap());
        let repository = Arc::new(ProtocolPackageRepositoryAdapter::with_default_limits(
            Arc::clone(&store),
        ));
        install_enabled(&repository);
        let mut listener = relay_listener();
        set_upstream_options(&mut listener, decode_enabled, encode_enabled);
        let rule = direction_rule(&listener, SocketDirection::Upstream, 10, 1, modifies);
        let workspace = ProxyWorkspace {
            listeners: vec![listener.clone()],
            socket_rules: vec![rule],
            socket_rule_created_order_high_water: 1,
            ..ProxyWorkspace::default()
        };
        let runtime = ListenerRuntimeAdapter::new(store).with_protocol_packages(repository);
        let result = ListenerRuntimePlanBuilder::new(&runtime)
            .build(&workspace, &listener, Uuid::new_v4())
            .await;

        if let Some(code) = expected_error {
            assert_eq!(
                result
                    .err()
                    .expect("matrix case must be rejected")
                    .view_model
                    .code,
                code
            );
        } else {
            let snapshot = result.unwrap().scripted_snapshot().unwrap();
            assert_eq!(
                snapshot
                    .rule_program(SocketDirection::Upstream)
                    .rules()
                    .len(),
                1
            );
        }
    }
}

#[tokio::test]
async fn snapshot_partitions_both_directions_without_changing_stable_rule_order() {
    let store = Arc::new(SqliteStore::in_memory().unwrap());
    let repository = Arc::new(ProtocolPackageRepositoryAdapter::with_default_limits(
        Arc::clone(&store),
    ));
    install_enabled(&repository);
    let listener = relay_listener();
    let workspace = ProxyWorkspace {
        listeners: vec![listener.clone()],
        socket_rules: vec![
            direction_rule(&listener, SocketDirection::Downstream, 20, 2, false),
            direction_rule(&listener, SocketDirection::Upstream, 10, 4, false),
            direction_rule(&listener, SocketDirection::Downstream, 10, 3, false),
            direction_rule(&listener, SocketDirection::Upstream, 10, 1, false),
        ],
        socket_rule_created_order_high_water: 4,
        ..ProxyWorkspace::default()
    };
    let runtime = ListenerRuntimeAdapter::new(store).with_protocol_packages(repository);
    let plan = ListenerRuntimePlanBuilder::new(&runtime)
        .build(&workspace, &listener, Uuid::new_v4())
        .await
        .unwrap();
    let snapshot = plan.scripted_snapshot().unwrap();

    assert_eq!(
        orders(&snapshot, SocketDirection::Upstream),
        vec![(10, 1), (10, 4)]
    );
    assert_eq!(
        orders(&snapshot, SocketDirection::Downstream),
        vec![(10, 3), (20, 2)]
    );
}

#[tokio::test]
async fn local_decode_off_executes_static_response_from_the_frozen_snapshot_factory() {
    let store = Arc::new(SqliteStore::in_memory().unwrap());
    let repository = Arc::new(ProtocolPackageRepositoryAdapter::with_default_limits(
        Arc::clone(&store),
    ));
    install_enabled(&repository);
    let listener = scripted_listener(SocketTopology::LocalResponder(
        SocketLocalResponderTopology::default(),
    ));
    let mut workspace = ProxyWorkspace {
        listeners: vec![listener.clone()],
        socket_rules: vec![direction_rule(
            &listener,
            SocketDirection::Downstream,
            10,
            1,
            true,
        )],
        socket_rule_created_order_high_water: 1,
        ..ProxyWorkspace::default()
    };
    let runtime = ListenerRuntimeAdapter::new(store).with_protocol_packages(repository);
    let plan = ListenerRuntimePlanBuilder::new(&runtime)
        .build(&workspace, &listener, Uuid::new_v4())
        .await
        .unwrap();
    let snapshot = plan.scripted_snapshot().unwrap();

    // 快照建立后修改 Workspace，不得改变已冻结连接执行结果。
    workspace.socket_rules.clear();
    let connection = snapshot.rule_connections().connection(
        SocketConnectionIdentity {
            runtime_epoch: Uuid::from_u128(1),
            connection_id: Uuid::from_u128(2),
            peer_addr: "127.0.0.1:10000".parse().unwrap(),
        },
        SocketDirection::Downstream,
    );
    let result = connection.execute(connection.empty_document()).unwrap();

    assert_eq!(result.matched_rule_ids().len(), 1);
    assert_eq!(
        result.document().get("amount").unwrap(),
        &DocumentValue::Int(42)
    );
    assert!(
        snapshot
            .rule_program(SocketDirection::Upstream)
            .rules()
            .is_empty()
    );
}

fn relay_listener() -> ProxyListener {
    scripted_listener(SocketTopology::Relay(SocketRelayTopology {
        upstream: SocketEndpoint {
            host: "127.0.0.1".into(),
            port: 9_999,
        },
        security: SocketRelaySecurity::Transparent,
    }))
}

fn set_upstream_options(listener: &mut ProxyListener, decode: bool, encode: bool) {
    let ListenerDataPlane::Socket(socket) = &mut listener.data_plane else {
        panic!("test listener must use the Socket data plane");
    };
    let SocketPayloadProcessing::Scripted(scripted) = &mut socket.processing else {
        panic!("test listener must use Scripted processing");
    };
    scripted.upstream = DirectionProcessingOptions {
        decode_enabled: decode,
        encode_enabled: encode,
    };
}

fn direction_rule(
    listener: &ProxyListener,
    direction: SocketDirection,
    priority: i32,
    created_order: u64,
    modifies: bool,
) -> SocketDocumentRuleDefinition {
    let actions = if modifies {
        vec![DocumentAction::SetField {
            field: DocumentFieldName::new("amount").unwrap(),
            value: DocumentValue::Int(42),
        }]
    } else {
        vec![DocumentAction::RecordMatch]
    };
    SocketDocumentRuleDefinition::new(
        SocketDocumentRuleId::new(),
        true,
        priority,
        created_order,
        listener.id,
        snapshot_package(),
        7,
        direction,
        Vec::new(),
        actions,
    )
    .unwrap()
}

fn orders(
    snapshot: &super::super::super::scripted_snapshot::ScriptedSocketRuntimeSnapshot,
    direction: SocketDirection,
) -> Vec<(i32, u64)> {
    snapshot
        .rule_program(direction)
        .rules()
        .iter()
        .map(|rule| (rule.priority(), rule.created_order()))
        .collect()
}
