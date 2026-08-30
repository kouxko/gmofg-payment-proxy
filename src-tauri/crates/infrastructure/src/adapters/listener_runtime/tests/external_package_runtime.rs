//! 外部协议包从真实 WebSocket peer 到真实 Socket Listener 的端到端验收。
//!
//! 测试只从公开运行入口启动 `ExternalPackageServer` 与 `ListenerRuntimeAdapter`，并使用
//! localhost TCP client/upstream 验证线路结果；不直接调用 external processor。

use intercept_proxy_application::{ExternalPackageApplicationPort, ListenerRuntimePort};
use intercept_proxy_domain::{
    Condition, ConditionTree, DocumentValue, JsonPointer, ProtocolDocumentOperation,
    ProtocolDocumentPredicate, ProtocolDocumentRuleDefinition, ProtocolDocumentRuleId,
    ProtocolRuleStage, RuleContent,
};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};
use tokio::time::timeout;

use external_package_runtime_support::*;

#[tokio::test]
async fn external_relay_handles_fragmentation_across_sequential_interactions() {
    let mut harness = ExternalRuntimeHarness::start().await;
    let upstream = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let upstream_address = upstream.local_addr().unwrap();
    let listener_address = reserve_address().await;
    let listener = external_relay_listener(listener_address, upstream_address, &harness.package);
    let workspace = external_workspace(listener.clone(), Vec::new());

    harness.start_listener(workspace, listener.clone()).await;
    let upstream_task = tokio::spawn(async move {
        let (mut stream, _) = upstream.accept().await.unwrap();
        let mut first = [0_u8; 3];
        stream.read_exact(&mut first).await.unwrap();
        assert_eq!(first, [3, b'a', b'b']);
        stream.write_all(&[3, b'x', b'y']).await.unwrap();
        let mut second = [0_u8; 4];
        stream.read_exact(&mut second).await.unwrap();
        assert_eq!(second, [4, b'c', b'd', b'e']);
        stream.write_all(&[4, b'z', b'1', b'2']).await.unwrap();
        stream.shutdown().await.unwrap();
    });

    let mut client = TcpStream::connect(listener_address).await.unwrap();
    client.write_all(&[3]).await.unwrap();
    harness.peer().wait_for_need_more().await;
    client.write_all(b"ab").await.unwrap();
    let mut first_response = [0_u8; 3];
    client.read_exact(&mut first_response).await.unwrap();
    assert_eq!(first_response, [3, b'x', b'y']);
    client.write_all(&[4, b'c', b'd', b'e']).await.unwrap();
    client.shutdown().await.unwrap();
    let mut response = Vec::new();
    timeout(TEST_TIMEOUT, client.read_to_end(&mut response))
        .await
        .expect("relay response deadline")
        .unwrap();

    assert_eq!(response, [4, b'z', b'1', b'2']);
    upstream_task.await.unwrap();
    assert_eq!(harness.peer().registration_count(), 1);
    harness.stop_listener(listener.id).await;
    harness.shutdown().await;
}

#[tokio::test]
async fn external_local_server_echoes_one_payload_through_both_direction_hooks() {
    let mut harness = ExternalRuntimeHarness::start().await;
    let listener_address = reserve_address().await;
    let listener = external_local_listener(listener_address, &harness.package);
    let workspace = external_workspace(listener.clone(), Vec::new());
    harness.start_listener(workspace, listener.clone()).await;

    let mut client = TcpStream::connect(listener_address).await.unwrap();
    client.write_all(&[3, b'a', b'b']).await.unwrap();
    client.shutdown().await.unwrap();
    let mut response = Vec::new();
    timeout(TEST_TIMEOUT, client.read_to_end(&mut response))
        .await
        .expect("LocalResponder response deadline")
        .unwrap();

    assert_eq!(response, [3, b'a', b'b']);
    harness.stop_listener(listener.id).await;
    harness.shutdown().await;
}

#[tokio::test]
async fn external_peer_disconnect_marks_package_offline_and_stops_exact_listener() {
    let mut harness = ExternalRuntimeHarness::start().await;
    let listener_address = reserve_address().await;
    let listener = external_local_listener(listener_address, &harness.package);
    let workspace = external_workspace(listener.clone(), Vec::new());
    harness.start_listener(workspace, listener.clone()).await;

    harness.disconnect_peer().await;
    harness.wait_until_offline().await;
    harness.wait_until_listener_stopped(listener.id).await;

    assert!(
        harness
            .registry
            .get(&harness.package)
            .await
            .unwrap()
            .is_some_and(|version| version.source.external_online() == Some(false))
    );
    assert!(harness.runtime.statuses().await.unwrap().is_empty());
    TcpListener::bind(listener_address)
        .await
        .expect("disconnect cleanup must release the exact listener port");
    harness.shutdown().await;
}

#[tokio::test]
async fn oversized_external_frame_boundary_closes_only_the_business_connection() {
    let mut harness = ExternalRuntimeHarness::start().await;
    let listener_address = reserve_address().await;
    let listener = external_local_listener(listener_address, &harness.package);
    let workspace = external_workspace(listener.clone(), Vec::new());
    harness.start_listener(workspace, listener.clone()).await;

    harness.peer().return_oversized_frame_boundary_once();
    let mut malformed = TcpStream::connect(listener_address).await.unwrap();
    malformed.write_all(&[3, b'a', b'b']).await.unwrap();
    malformed.shutdown().await.unwrap();
    let mut malformed_response = Vec::new();
    timeout(TEST_TIMEOUT, malformed.read_to_end(&mut malformed_response))
        .await
        .expect("malformed business connection closes")
        .unwrap();
    assert!(malformed_response.is_empty());
    assert!(
        harness
            .registry
            .get(&harness.package)
            .await
            .unwrap()
            .is_some_and(|version| version.source.external_online() == Some(true))
    );
    assert!(
        harness
            .runtime
            .statuses()
            .await
            .unwrap()
            .iter()
            .any(|status| status.listener_id == listener.id)
    );

    let mut healthy = TcpStream::connect(listener_address).await.unwrap();
    healthy.write_all(&[3, b'x', b'y']).await.unwrap();
    healthy.shutdown().await.unwrap();
    let mut healthy_response = Vec::new();
    timeout(TEST_TIMEOUT, healthy.read_to_end(&mut healthy_response))
        .await
        .expect("next business connection completes")
        .unwrap();
    assert_eq!(healthy_response, [3, b'x', b'y']);

    harness.stop_listener(listener.id).await;
    harness.shutdown().await;
}

#[tokio::test]
async fn production_socket_pipeline_rolls_back_failure_and_commits_each_write_stage_once() {
    let mut harness = ExternalRuntimeHarness::start().await;
    let upstream = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let upstream_address = upstream.local_addr().unwrap();
    let listener_address = reserve_address().await;
    let listener = external_relay_listener(listener_address, upstream_address, &harness.package);
    let mut workspace = external_workspace(
        listener.clone(),
        two_stage_one_shot_rules(&listener, &harness.package),
    );
    configure_nth_one_shot_chain(&mut workspace);
    assert_authoritative_write_stages(&workspace);
    let workspace_id = workspace.id;
    let initial_revision = workspace.revision.get();
    harness.start_listener(workspace, listener.clone()).await;
    let (upstream_seen_tx, upstream_seen_rx) = tokio::sync::oneshot::channel();
    let (release_echo_tx, release_echo_rx) = tokio::sync::oneshot::channel();
    let upstream_task = tokio::spawn(async move {
        let (mut first, _) = upstream.accept().await.unwrap();
        let mut first_request = [0_u8; 3];
        first.read_exact(&mut first_request).await.unwrap();
        assert_eq!(first_request, [3, b'a', b'b']);
        first.write_all(&first_request).await.unwrap();
        first.shutdown().await.unwrap();

        let (mut committed, _) = upstream.accept().await.unwrap();
        let mut committed_request = [0_u8; 3];
        committed.read_exact(&mut committed_request).await.unwrap();
        assert_eq!(committed_request, [3, b'x', b'b']);
        upstream_seen_tx.send(()).unwrap();
        release_echo_rx.await.unwrap();
        committed.write_all(&committed_request).await.unwrap();
        committed.shutdown().await.unwrap();
    });

    let first_miss = socket_roundtrip(listener_address, [3, b'a', b'b']).await;
    assert_eq!(first_miss, [3, b'a', b'b']);
    assert!(harness.peer().encode_methods().is_empty());
    let after_first_miss = harness.workspace(workspace_id);
    assert_eq!(after_first_miss.revision.get(), initial_revision + 1);
    assert!(after_first_miss.rule_definitions.iter().all(|rule| {
        rule.enabled() && rule.lifecycle().hit_count == 0 && rule.lifecycle().last_hit_at.is_none()
    }));

    harness.peer().fail_encode_once();
    let rejected_response = socket_roundtrip(listener_address, [3, b'a', b'b']).await;
    assert!(rejected_response.is_empty());
    assert_eq!(harness.peer().encode_methods(), ["hooks.upstream.encode"]);
    let rolled_back = harness.workspace(workspace_id);
    assert_eq!(rolled_back.revision.get(), initial_revision + 1);
    assert!(rolled_back.rule_definitions.iter().all(|rule| {
        rule.enabled() && rule.lifecycle().hit_count == 0 && rule.lifecycle().last_hit_at.is_none()
    }));

    let committed_task = tokio::spawn(socket_roundtrip(listener_address, [3, b'a', b'b']));
    timeout(TEST_TIMEOUT, upstream_seen_rx)
        .await
        .expect("upstream committed stage observation deadline")
        .unwrap();
    let after_upstream_commit = harness.workspace(workspace_id);
    assert_eq!(after_upstream_commit.revision.get(), initial_revision + 2);
    let upstream_rule = after_upstream_commit
        .rule_definitions
        .iter()
        .find(|rule| rule.stage() == intercept_proxy_domain::RuleStage::ProxyToUpstream)
        .unwrap();
    assert!(!upstream_rule.enabled());
    assert_eq!(upstream_rule.lifecycle().hit_count, 1);
    let downstream_rule = after_upstream_commit
        .rule_definitions
        .iter()
        .find(|rule| rule.stage() == intercept_proxy_domain::RuleStage::ProxyToApp)
        .unwrap();
    assert!(downstream_rule.enabled());
    assert_eq!(downstream_rule.lifecycle().hit_count, 0);
    release_echo_tx.send(()).unwrap();
    let committed_response = committed_task.await.unwrap();
    assert_eq!(committed_response, [3, b'x', b'y']);
    assert_eq!(
        harness.peer().encode_methods(),
        [
            "hooks.upstream.encode",
            "hooks.upstream.encode",
            "hooks.downstream.encode",
        ]
    );
    let committed_workspace = harness.workspace(workspace_id);
    assert_eq!(committed_workspace.revision.get(), initial_revision + 3);
    assert!(committed_workspace.rule_definitions.iter().all(|rule| {
        !rule.enabled() && rule.lifecycle().hit_count == 1 && rule.lifecycle().last_hit_at.is_some()
    }));

    upstream_task.await.unwrap();
    harness.stop_listener(listener.id).await;
    harness.shutdown().await;
}

fn configure_nth_one_shot_chain(workspace: &mut intercept_proxy_domain::ProxyWorkspace) {
    for (index, definition) in workspace.rule_definitions.iter_mut().enumerate() {
        let mut draft = definition.to_draft();
        draft.one_shot = true;
        if index == 0 {
            let RuleContent::Socket(content) = &mut draft.content else {
                panic!("production Socket fixture must stay Socket-owned");
            };
            content.condition = ConditionTree::All(vec![
                ConditionTree::Leaf(Condition::NthHit { count: 2 }),
                content.condition.clone(),
            ]);
        }
        definition.update(definition.revision(), draft).unwrap();
    }
}

fn assert_authoritative_write_stages(workspace: &intercept_proxy_domain::ProxyWorkspace) {
    assert_eq!(
        workspace
            .rule_definitions
            .iter()
            .map(intercept_proxy_domain::RuleDefinition::stage)
            .collect::<Vec<_>>(),
        [
            intercept_proxy_domain::RuleStage::ProxyToUpstream,
            intercept_proxy_domain::RuleStage::ProxyToApp,
        ]
    );
}

async fn socket_roundtrip(listener: std::net::SocketAddr, request: [u8; 3]) -> Vec<u8> {
    let mut client = TcpStream::connect(listener).await.unwrap();
    client.write_all(&request).await.unwrap();
    client.shutdown().await.unwrap();
    let mut response = Vec::new();
    timeout(TEST_TIMEOUT, client.read_to_end(&mut response))
        .await
        .expect("Socket transaction response deadline")
        .unwrap();
    response
}

fn two_stage_one_shot_rules(
    listener: &intercept_proxy_domain::ProxyListener,
    package: &intercept_proxy_domain::ProtocolPackageRef,
) -> Vec<ProtocolDocumentRuleDefinition> {
    [
        (
            ProtocolRuleStage::ProxyToUpstream,
            10,
            1,
            vec![ProtocolDocumentPredicate::Equals {
                field: JsonPointer::parse("/payload/1").unwrap(),
                value: DocumentValue::integer(i64::from(b'a')).unwrap(),
            }],
            JsonPointer::parse("/payload/1").unwrap(),
            b'x',
        ),
        (
            ProtocolRuleStage::ProxyToApp,
            20,
            2,
            vec![ProtocolDocumentPredicate::Equals {
                field: JsonPointer::parse("/payload/1").unwrap(),
                value: DocumentValue::integer(i64::from(b'x')).unwrap(),
            }],
            JsonPointer::parse("/payload/2").unwrap(),
            b'y',
        ),
    ]
    .into_iter()
    .map(|(stage, priority, order, conditions, field, value)| {
        ProtocolDocumentRuleDefinition::new_named_for_stage(
            ProtocolDocumentRuleId::new(),
            format!("production {stage:?}"),
            true,
            priority,
            order,
            listener.id,
            package.clone(),
            stage,
            conditions,
            vec![ProtocolDocumentOperation::SetField {
                field,
                value: DocumentValue::integer(i64::from(value)).unwrap(),
            }],
        )
        .unwrap()
    })
    .collect()
}

#[path = "external_package_runtime/support.rs"]
mod external_package_runtime_support;
