use super::*;

mod listener_modes;
mod unified_validation;

fn pkg(id: &str, version: &str) -> ProtocolPackageRef {
    super::package(id, version)
}

fn field(name: &str) -> JsonPointer {
    JsonPointer::property(name)
}

fn equals(name: &str, value: DocumentValue) -> ConditionTree {
    ConditionTree::Leaf(Condition::Document {
        path: field(name),
        predicate: document_equals_predicate(value),
    })
}

fn set(name: &str, value: DocumentValue) -> UnifiedAction {
    UnifiedAction::Document(DocumentMutation::Set {
        path: field(name),
        value,
    })
}

fn document_equals_predicate(value: DocumentValue) -> DocumentPredicate {
    match value {
        DocumentValue::String(value) => DocumentPredicate::String(StringPredicate {
            operator: StringOperator::Equal,
            value,
        }),
        DocumentValue::Number(value) => DocumentPredicate::Number(NumberPredicate {
            operator: NumberOperator::Equal,
            value,
        }),
        DocumentValue::Boolean(value) => DocumentPredicate::Boolean(BooleanPredicate::Equal(value)),
        DocumentValue::Null(()) => DocumentPredicate::NullEqual,
        DocumentValue::Object(_) | DocumentValue::Array(_) => {
            panic!("fixture equality requires a scalar Document value")
        }
    }
}

fn description_with_blob(package: ProtocolPackageRef) -> ProtocolPackageDescriptionViewModel {
    let mut value = description(package);
    value.capabilities.downstream.encode = true;
    let intercept_proxy_domain::DocumentSchemaNode::Object { properties, .. } =
        &mut value.upstream_schema.as_mut().unwrap().root
    else {
        unreachable!()
    };
    properties.insert(
        "raw".into(),
        intercept_proxy_domain::DocumentSchemaNode::Array {
            title: Some("Raw".into()),
            items: Box::new(intercept_proxy_domain::DocumentSchemaNode::Number { title: None }),
        },
    );
    value
}

async fn configure_relay(
    services: &FakeProtocolPackageServices,
    workspaces: &InMemoryWorkspaceStore,
    package: &ProtocolPackageRef,
) -> ListenerId {
    services.insert(record(package.clone(), true));
    services.set_description(package.clone(), description_with_blob(package.clone()));
    let selected = workspaces
        .list()
        .await
        .unwrap()
        .into_iter()
        .find(|item| item.selected)
        .unwrap();
    let mut workspace = workspaces.get(selected.id).await.unwrap();
    let listener = &mut workspace.listeners[0];
    listener.data_plane = ListenerDataPlane::Socket(SocketRelaySettings {
        topology: SocketTopology::Relay(SocketRelayTopology {
            upstream: SocketEndpoint {
                host: "127.0.0.1".into(),
                port: 9000,
            },
            security: SocketRelaySecurity::Transparent,
        }),
        maximum_connections: 8,
        runtime_limits: intercept_proxy_domain::SocketRuntimeLimits::default(),
        processing: SocketPayloadProcessing::Scripted(ScriptedSocketProcessing {
            package: package.clone(),
        }),
    });
    let listener_id = listener.id;
    workspaces.save(workspace).await.unwrap();
    listener_id
}

async fn configure_http(
    services: &FakeProtocolPackageServices,
    workspaces: &InMemoryWorkspaceStore,
    package: &ProtocolPackageRef,
) -> ListenerId {
    let mut package_record = record(package.clone(), true);
    package_record.kind = ProtocolPackageKindViewModel::Http;
    services.insert(package_record);
    let mut package_description = description_with_blob(package.clone());
    package_description.kind = ProtocolPackageKindViewModel::Http;
    package_description.capabilities.upstream.frame = false;
    package_description.capabilities.downstream.frame = false;
    services.set_description(package.clone(), package_description);
    let selected = workspaces
        .list()
        .await
        .unwrap()
        .into_iter()
        .find(|item| item.selected)
        .unwrap();
    let mut workspace = workspaces.get(selected.id).await.unwrap();
    let listener = &mut workspace.listeners[0];
    listener.data_plane = ListenerDataPlane::Http(HttpListenerSettings {
        body_processing: HttpBodyProcessing::Protocol {
            package: package.clone(),
        },
        ..HttpListenerSettings::default()
    });
    let listener_id = listener.id;
    workspaces.save(workspace).await.unwrap();
    listener_id
}
