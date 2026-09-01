use super::*;
use crate::adapters::{PackageTransportError, ProtocolPackageRuntime};
use async_trait::async_trait;
use intercept_proxy_domain::{
    Condition, Document, DocumentMutation, DocumentPredicate, DocumentValue, JsonPointer,
    ListenerId, ProtocolDirection, RuleContent, RuleDefinition, RuleDefinitionDraft, RuleStage,
    SocketRuleContent, StringOperator, StringPredicate, UnifiedAction,
};

fn socket_rule(
    listener_id: ListenerId,
    package: intercept_proxy_domain::ProtocolPackageRef,
) -> RuleDefinition {
    RuleDefinition::create(
        RuleDefinitionDraft {
            name: "relay rule".to_owned(),
            enabled: true,
            priority: 10,
            listener_id,
            stage: RuleStage::ProxyToUpstream,
            content: RuleContent::Socket(SocketRuleContent {
                package,
                condition: Condition::Document {
                    path: JsonPointer::property("request"),
                    predicate: DocumentPredicate::String(StringPredicate {
                        operator: StringOperator::Equal,
                        value: "original".to_owned(),
                    }),
                },
                action: UnifiedAction::Document(DocumentMutation::Set {
                    path: JsonPointer::property("request"),
                    value: DocumentValue::String("updated".to_owned()),
                }),
            }),
        },
        1,
    )
    .unwrap()
}
use intercept_proxy_package_contract::{FrameResult, PackageManifest};
use serde_json::json;
use uuid::Uuid;

#[derive(Debug)]
struct DisconnectedRpc;

#[async_trait]
impl ProtocolPackageRuntime for DisconnectedRpc {
    async fn frame(
        &self,
        _direction: ProtocolDirection,
        _buffer: Vec<u8>,
    ) -> Result<FrameResult, PackageTransportError> {
        Err(PackageTransportError::Disconnected)
    }

    async fn decode_socket(
        &self,
        _direction: ProtocolDirection,
        _input: Vec<u8>,
    ) -> Result<Document, PackageTransportError> {
        Err(PackageTransportError::Disconnected)
    }

    async fn encode_socket(
        &self,
        _direction: ProtocolDirection,
        _original_input: Vec<u8>,
        _document: Document,
    ) -> Result<Vec<u8>, PackageTransportError> {
        Err(PackageTransportError::Disconnected)
    }

    async fn display(
        &self,
        _direction: ProtocolDirection,
        _document: Document,
    ) -> Result<String, PackageTransportError> {
        Err(PackageTransportError::Disconnected)
    }
}

#[tokio::test]
async fn runtime_contract_frame_preserves_connection_failure() {
    let runtime = DisconnectedRpc;
    assert!(matches!(
        runtime
            .frame(ProtocolDirection::Upstream, b"frame".to_vec())
            .await,
        Err(PackageTransportError::Disconnected)
    ));
}

#[tokio::test]
async fn runtime_contract_decode_preserves_connection_failure() {
    let runtime = DisconnectedRpc;
    assert!(matches!(
        runtime
            .decode_socket(ProtocolDirection::Upstream, b"frame".to_vec())
            .await,
        Err(PackageTransportError::Disconnected)
    ));
}

#[tokio::test]
async fn runtime_contract_encode_preserves_connection_failure() {
    let runtime = DisconnectedRpc;
    assert!(matches!(
        runtime
            .encode_socket(
                ProtocolDirection::Upstream,
                b"frame".to_vec(),
                serde_json::from_value(json!({})).unwrap(),
            )
            .await,
        Err(PackageTransportError::Disconnected)
    ));
}

#[tokio::test]
async fn runtime_contract_display_preserves_connection_failure() {
    let runtime = DisconnectedRpc;
    assert!(matches!(
        runtime
            .display(
                ProtocolDirection::Upstream,
                serde_json::from_value(json!({})).unwrap(),
            )
            .await,
        Err(PackageTransportError::Disconnected)
    ));
}

#[test]
fn binding_preserves_registration_limits_and_safe_debug_identity() {
    let registration = registration();
    let package = registration.package().identity().clone();
    let binding =
        ExternalSocketPackageBinding::with_limits(registration, Arc::new(DisconnectedRpc), 4096);

    assert_eq!(binding.registration().package().identity(), package);
    assert_eq!(binding.max_frame_bytes(), 4096);
    assert_eq!(
        format!("{binding:?}"),
        "ExternalSocketPackageBinding { package: ProtocolPackageRef { id: ProtocolPackageId(\"contract-test\"), version: ProtocolPackageVersion(\"1.0.0\") }, .. }"
    );
}

#[test]
fn snapshot_debug_includes_binding_and_topology_without_rule_state() {
    let registration = registration();
    let listener = listener();
    let snapshot = ExternalSocketRuntimeSnapshot::new(
        ExternalSocketPackageBinding::new(registration.clone(), Arc::new(DisconnectedRpc)),
        empty_rules(&registration, listener.id),
        SocketTopology::default(),
    );

    let debug = format!("{snapshot:?}");

    assert!(debug.contains("ExternalSocketRuntimeSnapshot"));
    assert!(debug.contains("contract-test"));
    assert!(debug.contains("topology: Relay"));
    assert!(!debug.contains("rules"));
}

#[tokio::test]
async fn replace_document_rules_installs_new_rules_for_the_running_snapshot() {
    let registration = registration();
    let listener = listener();
    let package = registration.package().identity().clone();
    let rule = socket_rule(listener.id, package);
    let workspace = ProxyWorkspace {
        rule_created_order_high_water: rule.created_order(),
        rule_definitions: vec![rule],
        ..ProxyWorkspace::default()
    };
    let snapshot = ExternalSocketRuntimeSnapshot::new(
        ExternalSocketPackageBinding::new(registration.clone(), Arc::new(DisconnectedRpc)),
        empty_rules(&registration, listener.id),
        SocketTopology::default(),
    );
    let adapter = crate::adapters::listener_runtime::tests::test_listener_runtime(Arc::new(
        crate::SqliteStore::in_memory().unwrap(),
    ));

    let replacement = snapshot
        .compile_replacement(&adapter, &workspace, &listener)
        .await
        .unwrap();
    snapshot.publish_replacement(&replacement);

    assert_eq!(
        snapshot
            .rules
            .direction_programs(ProtocolDirection::Upstream)[0]
            .rules()
            .len(),
        1
    );
}

#[tokio::test(flavor = "current_thread")]
async fn queued_rule_compile_cancellation_keeps_runtime_responsive_and_rules_unchanged() {
    use std::{future::Future, pin::Pin, task::Poll};

    let registration = registration();
    let listener = listener();
    let workspace = relay_rule_workspace(&registration, &listener);
    let snapshot = ExternalSocketRuntimeSnapshot::new(
        ExternalSocketPackageBinding::new(registration.clone(), Arc::new(DisconnectedRpc)),
        empty_rules(&registration, listener.id),
        SocketTopology::default(),
    );
    let adapter = crate::adapters::listener_runtime::tests::test_listener_runtime(Arc::new(
        crate::SqliteStore::in_memory().unwrap(),
    ));
    let capacity = adapter.document_rule_compiler.occupy_all().await;
    let mut replacement = Box::pin(snapshot.compile_replacement(&adapter, &workspace, &listener));
    let first_poll =
        std::future::poll_fn(|context| Poll::Ready(Pin::new(&mut replacement).poll(context))).await;
    assert!(matches!(first_poll, Poll::Pending));
    let (progress_tx, progress_rx) = tokio::sync::oneshot::channel();
    tokio::spawn(async move { progress_tx.send(()).unwrap() });
    progress_rx.await.unwrap();
    drop(replacement);
    drop(capacity);

    assert!(
        snapshot
            .rules
            .direction_programs(ProtocolDirection::Upstream)[0]
            .rules()
            .is_empty()
    );
}

#[test]
fn compiled_rule_replacement_is_published_as_one_snapshot() {
    let registration = registration();
    let listener = listener();
    let workspace = relay_rule_workspace(&registration, &listener);
    let snapshot = ExternalSocketRuntimeSnapshot::new(
        ExternalSocketPackageBinding::new(registration.clone(), Arc::new(DisconnectedRpc)),
        empty_rules(&registration, listener.id),
        SocketTopology::default(),
    );
    let newer = document_rules::compile_document_rules(
        &workspace,
        &listener,
        &registration.package().identity(),
        registration.document().upstream().schema(),
        registration.document().downstream().schema(),
        &SocketTopology::default(),
    )
    .unwrap();

    snapshot.publish_replacement(&newer);

    assert_eq!(
        snapshot
            .rules
            .direction_programs(ProtocolDirection::Upstream)[0]
            .rules()
            .len(),
        1
    );
}

fn relay_rule_workspace(
    registration: &PackageManifest,
    listener: &ProxyListener,
) -> ProxyWorkspace {
    let rule = socket_rule(listener.id, registration.package().identity().clone());
    ProxyWorkspace {
        rule_created_order_high_water: rule.created_order(),
        rule_definitions: vec![rule],
        ..ProxyWorkspace::default()
    }
}

fn listener() -> ProxyListener {
    ProxyListener {
        id: ListenerId::from_uuid(Uuid::from_u128(42)),
        ..ProxyListener::default()
    }
}

fn empty_rules(registration: &PackageManifest, listener_id: ListenerId) -> DocumentProgramFactory {
    let package = registration.package().identity().clone();
    DocumentProgramFactory::new(
        listener_id,
        package,
        Arc::new(intercept_proxy_domain::UnifiedRuleProgram::new(Vec::new()).unwrap()),
        Arc::new(intercept_proxy_domain::UnifiedRuleProgram::new(Vec::new()).unwrap()),
    )
}

fn registration() -> PackageManifest {
    serde_json::from_value(json!({
        "api": 1,
        "kind": "socket",
        "package": {
            "id": "contract-test",
            "name": "Contract test",
            "version": "1.0.0",
            "description": "test"
        },
        "document": {
            "upstream": {
                "schema": {
                    "type": "object",
                    "title": "Upstream",
                    "properties": {
                        "request": {"type": "string", "title": "Request"}
                    }
                }
            },
            "downstream": {
                "schema": {
                    "type": "object",
                    "title": "Downstream",
                    "properties": {
                        "response": {"type": "string", "title": "Response"}
                    }
                }
            }
        }
    }))
    .unwrap()
}
