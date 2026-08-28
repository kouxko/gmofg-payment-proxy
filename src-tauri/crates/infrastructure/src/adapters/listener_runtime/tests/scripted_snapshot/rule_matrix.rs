//! Scripted 启动快照的方向能力矩阵与冻结规则执行回归。

use intercept_proxy_domain::{DocumentFieldName, DocumentValue, ProtocolDocumentRuleDefinition};
use intercept_proxy_runtime::SocketConnectionIdentity;

use super::*;

#[tokio::test]
async fn scripted_relay_always_builds_the_full_rule_chain() {
    let store = Arc::new(SqliteStore::in_memory().unwrap());
    let repository = Arc::new(ProtocolPackageRepositoryAdapter::with_default_limits(
        Arc::clone(&store),
    ));
    install_enabled(&repository);
    let listener = relay_listener();
    let rule = direction_rule(&listener, ProtocolDirection::Upstream, 10, 1, true);
    let mut workspace = ProxyWorkspace {
        listeners: vec![listener.clone()],
        rule_created_order_high_water: 1,
        ..ProxyWorkspace::default()
    };
    workspace
        .replace_document_runtime_rules(vec![rule])
        .unwrap();
    let runtime = test_listener_runtime_with_packages(store, repository);
    let snapshot = ListenerRuntimePlanBuilder::new(&runtime)
        .build(&workspace, &listener, Uuid::new_v4())
        .await
        .unwrap()
        .scripted_snapshot()
        .unwrap();

    assert_eq!(
        snapshot
            .rule_program(ProtocolRuleStage::ProxyToUpstream)
            .rules()
            .len(),
        1
    );
}

#[tokio::test]
async fn snapshot_partitions_both_directions_without_changing_stable_rule_order() {
    let store = Arc::new(SqliteStore::in_memory().unwrap());
    let repository = Arc::new(ProtocolPackageRepositoryAdapter::with_default_limits(
        Arc::clone(&store),
    ));
    install_enabled(&repository);
    let listener = relay_listener();
    let mut workspace = ProxyWorkspace {
        listeners: vec![listener.clone()],
        rule_created_order_high_water: 4,
        ..ProxyWorkspace::default()
    };
    workspace
        .replace_document_runtime_rules(vec![
            direction_rule(&listener, ProtocolDirection::Downstream, 20, 2, false),
            direction_rule(&listener, ProtocolDirection::Upstream, 10, 4, false),
            direction_rule(&listener, ProtocolDirection::Downstream, 10, 3, false),
            direction_rule(&listener, ProtocolDirection::Upstream, 10, 1, false),
        ])
        .unwrap();
    let runtime = test_listener_runtime_with_packages(store, repository);
    let plan = ListenerRuntimePlanBuilder::new(&runtime)
        .build(&workspace, &listener, Uuid::new_v4())
        .await
        .unwrap();
    let snapshot = plan.scripted_snapshot().unwrap();

    assert_eq!(
        orders(&snapshot, ProtocolDirection::Upstream),
        vec![(10, 1), (10, 4)]
    );
    assert_eq!(
        orders(&snapshot, ProtocolDirection::Downstream),
        vec![(10, 3), (20, 2)]
    );
}

#[tokio::test]
async fn local_response_executes_static_response_from_the_frozen_snapshot_factory() {
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
        rule_created_order_high_water: 1,
        ..ProxyWorkspace::default()
    };
    workspace
        .replace_document_runtime_rules(vec![direction_rule(
            &listener,
            ProtocolDirection::Downstream,
            10,
            1,
            true,
        )])
        .unwrap();
    let runtime = test_listener_runtime_with_packages(store, repository);
    let plan = ListenerRuntimePlanBuilder::new(&runtime)
        .build(&workspace, &listener, Uuid::new_v4())
        .await
        .unwrap();
    let snapshot = plan.scripted_snapshot().unwrap();

    // 快照建立后修改 Workspace，不得改变已冻结连接执行结果。
    workspace
        .replace_document_runtime_rules(Vec::new())
        .unwrap();
    let connection = snapshot.rule_connections().connection(
        SocketConnectionIdentity {
            runtime_epoch: Uuid::from_u128(1),
            connection_id: Uuid::from_u128(2),
            peer_addr: "127.0.0.1:10000".parse().unwrap(),
        },
        ProtocolRuleStage::ProxyToApp,
    );
    let result = connection.execute(connection.empty_document()).unwrap();

    assert_eq!(result.matched_rule_ids().len(), 1);
    assert_eq!(
        result.document().get("amount").unwrap(),
        &DocumentValue::Int(42)
    );
    assert!(
        snapshot
            .rule_program(ProtocolRuleStage::ProxyToUpstream)
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

fn direction_rule(
    listener: &ProxyListener,
    direction: ProtocolDirection,
    priority: i32,
    created_order: u64,
    modifies: bool,
) -> ProtocolDocumentRuleDefinition {
    let actions = if modifies {
        vec![DocumentAction::SetField {
            field: DocumentFieldName::new("amount").unwrap(),
            value: DocumentValue::Int(42),
        }]
    } else {
        vec![DocumentAction::RecordMatch]
    };
    ProtocolDocumentRuleDefinition::new(
        ProtocolDocumentRuleId::new(),
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
    direction: ProtocolDirection,
) -> Vec<(i32, u64)> {
    let stage = match direction {
        ProtocolDirection::Upstream => ProtocolRuleStage::ProxyToUpstream,
        ProtocolDirection::Downstream => ProtocolRuleStage::ProxyToApp,
    };
    snapshot
        .rule_program(stage)
        .rules()
        .iter()
        .map(|rule| (rule.priority(), rule.created_order()))
        .collect()
}
