use super::*;
use intercept_proxy_domain::{
    Document, DocumentAction, DocumentCondition, DocumentValue, JsonPointer, ListenerId,
    ProtocolDirection, ProtocolDocumentRuleDefinition, ProtocolDocumentRuleId,
    ProtocolDocumentRuleProgram, ProtocolRuleStage, SocketLocalResponderTopology,
};
use intercept_proxy_package_contract::{
    CanonicalBase64, DecodeParams, DisplayParams, EncodeParams, FrameParams, FrameResult,
    PackageManifest,
};
use serde_json::json;
use uuid::Uuid;

#[derive(Debug)]
struct DisconnectedRpc;

#[async_trait]
impl ExternalPackageRpc for DisconnectedRpc {
    async fn frame(
        &self,
        _direction: ProtocolDirection,
        _request: FrameParams,
    ) -> Result<FrameResult, PackageTransportError> {
        Err(PackageTransportError::Disconnected)
    }

    async fn decode(
        &self,
        _direction: ProtocolDirection,
        _request: DecodeParams,
    ) -> Result<Document, PackageTransportError> {
        Err(PackageTransportError::Disconnected)
    }

    async fn encode(
        &self,
        _direction: ProtocolDirection,
        _request: EncodeParams,
    ) -> Result<String, PackageTransportError> {
        Err(PackageTransportError::Disconnected)
    }

    async fn display(
        &self,
        _direction: ProtocolDirection,
        _request: DisplayParams,
    ) -> Result<String, PackageTransportError> {
        Err(PackageTransportError::Disconnected)
    }
}

#[tokio::test]
async fn rpc_contract_frame_preserves_connection_failure() {
    let rpc = DisconnectedRpc;
    assert!(matches!(
        rpc.frame(
            ProtocolDirection::Upstream,
            FrameParams {
                buffer: CanonicalBase64::from_bytes(b"frame")
            }
        )
        .await,
        Err(PackageTransportError::Disconnected)
    ));
}

#[tokio::test]
async fn rpc_contract_decode_preserves_connection_failure() {
    let rpc = DisconnectedRpc;
    assert!(matches!(
        rpc.decode(
            ProtocolDirection::Upstream,
            DecodeParams {
                input: CanonicalBase64::from_bytes(b"frame").as_str().to_owned()
            }
        )
        .await,
        Err(PackageTransportError::Disconnected)
    ));
}

#[tokio::test]
async fn rpc_contract_encode_preserves_connection_failure() {
    let rpc = DisconnectedRpc;
    assert!(matches!(
        rpc.encode(
            ProtocolDirection::Upstream,
            EncodeParams {
                original_input: CanonicalBase64::from_bytes(b"frame").as_str().to_owned(),
                document: serde_json::from_value(json!({})).unwrap(),
            }
        )
        .await,
        Err(PackageTransportError::Disconnected)
    ));
}

#[tokio::test]
async fn rpc_contract_display_preserves_connection_failure() {
    let rpc = DisconnectedRpc;
    assert!(matches!(
        rpc.display(
            ProtocolDirection::Upstream,
            DisplayParams {
                document: serde_json::from_value(json!({})).unwrap(),
            }
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
    let rule = ProtocolDocumentRuleDefinition::new_named_for_stage(
        ProtocolDocumentRuleId::new(),
        "relay rule".to_owned(),
        true,
        10,
        1,
        listener.id,
        package,
        ProtocolRuleStage::ProxyToUpstream,
        vec![DocumentCondition::Equals {
            field: JsonPointer::property("request"),
            value: DocumentValue::String("original".to_owned()),
        }],
        vec![DocumentAction::SetField {
            field: JsonPointer::property("request"),
            value: DocumentValue::String("updated".to_owned()),
        }],
    )
    .unwrap();
    let mut workspace = ProxyWorkspace::default();
    workspace
        .replace_document_runtime_rules(vec![rule])
        .unwrap();
    let snapshot = ExternalSocketRuntimeSnapshot::new(
        ExternalSocketPackageBinding::new(registration.clone(), Arc::new(DisconnectedRpc)),
        empty_rules(&registration, listener.id),
        SocketTopology::default(),
    );
    let adapter = crate::adapters::listener_runtime::tests::test_listener_runtime(Arc::new(
        crate::SqliteStore::in_memory().unwrap(),
    ));

    snapshot
        .replace_document_rules(&adapter, &workspace, &listener)
        .await
        .unwrap();

    assert_eq!(
        snapshot
            .rules
            .program(ProtocolRuleStage::ProxyToUpstream)
            .rules()
            .len(),
        1
    );
}

#[tokio::test]
async fn replace_document_rules_rejects_relay_only_stage_for_local_responder() {
    let registration = registration();
    let listener = listener();
    let package = registration.package().identity().clone();
    let rule = ProtocolDocumentRuleDefinition::new_named_for_stage(
        ProtocolDocumentRuleId::new(),
        "invalid local stage".to_owned(),
        true,
        10,
        1,
        listener.id,
        package,
        ProtocolRuleStage::ProxyToUpstream,
        vec![DocumentCondition::Equals {
            field: JsonPointer::property("request"),
            value: DocumentValue::String("original".to_owned()),
        }],
        vec![DocumentAction::SetField {
            field: JsonPointer::property("request"),
            value: DocumentValue::String("updated".to_owned()),
        }],
    )
    .unwrap();
    let mut workspace = ProxyWorkspace::default();
    workspace
        .replace_document_runtime_rules(vec![rule])
        .unwrap();
    let snapshot = ExternalSocketRuntimeSnapshot::new(
        ExternalSocketPackageBinding::new(registration.clone(), Arc::new(DisconnectedRpc)),
        empty_rules(&registration, listener.id),
        SocketTopology::LocalResponder(SocketLocalResponderTopology::default()),
    );
    let adapter = crate::adapters::listener_runtime::tests::test_listener_runtime(Arc::new(
        crate::SqliteStore::in_memory().unwrap(),
    ));

    let error = snapshot
        .replace_document_rules(&adapter, &workspace, &listener)
        .await
        .unwrap_err();

    assert_eq!(error.view_model.code, "PROTOCOL_RULE_DIRECTION_INVALID");
    assert!(
        snapshot
            .rules
            .program(ProtocolRuleStage::ProxyToUpstream)
            .rules()
            .is_empty()
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
    let mut replacement =
        Box::pin(snapshot.replace_document_rules(&adapter, &workspace, &listener));
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
            .program(ProtocolRuleStage::ProxyToUpstream)
            .rules()
            .is_empty()
    );
}

#[test]
fn stale_rule_compile_generation_cannot_overwrite_newer_rules() {
    let registration = registration();
    let listener = listener();
    let workspace = relay_rule_workspace(&registration, &listener);
    let snapshot = ExternalSocketRuntimeSnapshot::new(
        ExternalSocketPackageBinding::new(registration.clone(), Arc::new(DisconnectedRpc)),
        empty_rules(&registration, listener.id),
        SocketTopology::default(),
    );
    let stale = snapshot.rule_generation.fetch_add(1, Ordering::AcqRel) + 1;
    let current = snapshot.rule_generation.fetch_add(1, Ordering::AcqRel) + 1;
    let newer = scripted_snapshot::compile_document_rules(
        &workspace,
        &listener,
        &registration.package().identity(),
        registration.document().upstream().schema().unwrap(),
        registration.document().downstream().schema().unwrap(),
        &SocketTopology::default(),
    )
    .unwrap();
    let older = empty_rules(&registration, listener.id);

    snapshot.publish_document_rules(current, &newer);
    snapshot.publish_document_rules(stale, &older);

    assert_eq!(
        snapshot
            .rules
            .program(ProtocolRuleStage::ProxyToUpstream)
            .rules()
            .len(),
        1
    );
}

fn relay_rule_workspace(
    registration: &PackageManifest,
    listener: &ProxyListener,
) -> ProxyWorkspace {
    let rule = ProtocolDocumentRuleDefinition::new_named_for_stage(
        ProtocolDocumentRuleId::new(),
        "relay rule".to_owned(),
        true,
        10,
        1,
        listener.id,
        registration.package().identity().clone(),
        ProtocolRuleStage::ProxyToUpstream,
        vec![DocumentCondition::Equals {
            field: JsonPointer::property("request"),
            value: DocumentValue::String("original".to_owned()),
        }],
        vec![DocumentAction::SetField {
            field: JsonPointer::property("request"),
            value: DocumentValue::String("updated".to_owned()),
        }],
    )
    .unwrap();
    let mut workspace = ProxyWorkspace::default();
    workspace
        .replace_document_runtime_rules(vec![rule])
        .unwrap();
    workspace
}

fn listener() -> ProxyListener {
    ProxyListener {
        id: ListenerId::from_uuid(Uuid::from_u128(42)),
        ..ProxyListener::default()
    }
}

fn empty_rules(
    registration: &PackageManifest,
    listener_id: ListenerId,
) -> ProtocolDocumentRuleConnectionFactory {
    let package = registration.package().identity().clone();
    let upstream = registration.document().upstream().schema().unwrap().clone();
    let downstream = registration
        .document()
        .downstream()
        .schema()
        .unwrap()
        .clone();
    let program = |stage, schema| {
        Arc::new(
            ProtocolDocumentRuleProgram::new_for_stage(
                listener_id,
                package.clone(),
                schema,
                stage,
                Vec::new(),
            )
            .unwrap(),
        )
    };
    ProtocolDocumentRuleConnectionFactory::new(
        program(ProtocolRuleStage::AppToProxy, upstream.clone()),
        program(ProtocolRuleStage::ProxyToUpstream, upstream),
        program(ProtocolRuleStage::UpstreamToProxy, downstream.clone()),
        program(ProtocolRuleStage::ProxyToApp, downstream),
    )
    .unwrap()
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
