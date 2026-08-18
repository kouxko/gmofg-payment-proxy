use super::*;

mod concurrency;
mod lifecycle;
mod listener_modes;

fn pkg(id: &str, version: &str) -> ProtocolPackageRef {
    super::package(id, version)
}

fn field(name: &str) -> DocumentFieldName {
    DocumentFieldName::new(name).unwrap()
}

fn equals(name: &str, value: DocumentValue) -> DocumentCondition {
    DocumentCondition::Equals {
        field: field(name),
        value,
    }
}

fn set(name: &str, value: DocumentValue) -> DocumentAction {
    DocumentAction::SetField {
        field: field(name),
        value,
    }
}

fn input(
    listener_id: ListenerId,
    package: ProtocolPackageRef,
    direction: SocketDirection,
    priority: i32,
) -> SocketRuleSaveInput {
    SocketRuleSaveInput {
        rule_id: None,
        expected_revision: None,
        name: "测试规则".into(),
        enabled: true,
        priority,
        listener_id,
        package,
        schema_version: 1,
        direction,
        conditions: Vec::new(),
        actions: vec![DocumentAction::RecordMatch],
    }
}

fn description_with_blob(package: ProtocolPackageRef) -> ProtocolPackageDescriptionViewModel {
    let mut value = description(package);
    value.capabilities.downstream.encode = true;
    value
        .schema
        .fields
        .push(ProtocolPackageSchemaFieldViewModel {
            name: "raw".into(),
            label: "Raw".into(),
            field_type: ProtocolPackageSchemaFieldTypeViewModel::Blob,
        });
    value
}

async fn configure_relay(
    services: &FakeProtocolPackageServices,
    workspaces: &InMemoryWorkspaceStore,
    package: &ProtocolPackageRef,
    upstream: DirectionProcessingOptions,
    downstream: DirectionProcessingOptions,
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
        processing: SocketPayloadProcessing::Scripted(ScriptedSocketProcessing {
            package: package.clone(),
            upstream,
            downstream,
        }),
    });
    let listener_id = listener.id;
    workspaces.save(workspace).await.unwrap();
    listener_id
}

#[tokio::test]
async fn capability_catalog_is_schema_typed_and_has_no_http_surface() {
    let (application, services, workspaces, _) = fixture();
    let package = pkg("iso-8583", "1.0.0");
    let listener_id = configure_relay(
        &services,
        &workspaces,
        &package,
        DirectionProcessingOptions {
            decode_enabled: true,
            encode_enabled: true,
        },
        DirectionProcessingOptions {
            decode_enabled: true,
            encode_enabled: false,
        },
    )
    .await;

    let upstream = application
        .socket_rule_capabilities(listener_id, SocketDirection::Upstream)
        .await
        .unwrap();
    assert_eq!(upstream.package, package);
    assert_eq!(upstream.schema_version, 1);
    assert_eq!(upstream.fields.len(), 4);
    assert!(upstream.fields.iter().all(|item| {
        item.operators == [SocketRuleFieldOperatorCapability::Equals]
            && item.actions == [SocketRuleFieldActionCapability::SetField]
    }));
    assert_eq!(
        upstream.common_actions,
        [
            SocketRuleCommonActionCapability::RecordMatch,
            SocketRuleCommonActionCapability::ClearDocument,
        ]
    );

    let downstream = application
        .socket_rule_capabilities(listener_id, SocketDirection::Downstream)
        .await
        .unwrap();
    assert!(downstream.fields.iter().all(|item| item.actions.is_empty()));
    assert_eq!(
        downstream.common_actions,
        [SocketRuleCommonActionCapability::RecordMatch]
    );

    let json = serde_json::to_value(&upstream).unwrap();
    let object = json.as_object().unwrap();
    for forbidden in [
        "method",
        "path",
        "query",
        "header",
        "cookie",
        "status",
        "json_path",
        "body",
    ] {
        assert!(!object.contains_key(forbidden));
        assert!(!json.to_string().contains(&format!("\"{forbidden}\"")));
    }

    let mut forged = serde_json::to_value(input(
        listener_id,
        pkg("iso-8583", "1.0.0"),
        SocketDirection::Upstream,
        0,
    ))
    .unwrap();
    forged["http_status"] = serde_json::json!(500);
    assert!(serde_json::from_value::<SocketRuleSaveInput>(forged).is_err());
}

#[tokio::test]
async fn save_validates_all_four_schema_value_types_and_exact_bindings() {
    let (application, services, workspaces, _) = fixture();
    let package = pkg("iso-8583", "1.0.0");
    let listener_id = configure_relay(
        &services,
        &workspaces,
        &package,
        DirectionProcessingOptions {
            decode_enabled: true,
            encode_enabled: true,
        },
        DirectionProcessingOptions::default(),
    )
    .await;
    let mut valid = input(listener_id, package.clone(), SocketDirection::Upstream, 0);
    valid.conditions = vec![
        equals("trace_id", DocumentValue::String("abc".into())),
        equals("amount", DocumentValue::Int(1000)),
        equals("approved", DocumentValue::Bool(true)),
        equals("raw", DocumentValue::Blob(vec![0, 255])),
    ];
    valid.actions = vec![
        DocumentAction::ClearDocument,
        set("trace_id", DocumentValue::String("next".into())),
        set("amount", DocumentValue::Int(2)),
        set("approved", DocumentValue::Bool(false)),
        set("raw", DocumentValue::Blob(vec![1, 2])),
    ];
    application.socket_rule_save(valid).await.unwrap();

    let mut wrong_type = input(listener_id, package.clone(), SocketDirection::Upstream, 1);
    wrong_type.conditions = vec![equals("amount", DocumentValue::String("1000".into()))];
    assert_eq!(
        error_code(&application.socket_rule_save(wrong_type).await.unwrap_err()),
        "RULE_INVALID"
    );

    let mut wrong_package = input(
        listener_id,
        pkg("iso-8583", "2.0.0"),
        SocketDirection::Upstream,
        1,
    );
    assert_eq!(
        error_code(
            &application
                .socket_rule_save(wrong_package.clone())
                .await
                .unwrap_err()
        ),
        "SOCKET_RULE_PACKAGE_MISMATCH"
    );
    wrong_package.package = package.clone();
    wrong_package.schema_version = 2;
    assert_eq!(
        error_code(
            &application
                .socket_rule_save(wrong_package)
                .await
                .unwrap_err()
        ),
        "SOCKET_RULE_SCHEMA_MISMATCH"
    );
}

#[tokio::test]
async fn update_preserves_binding_rejects_stale_revision_and_never_queries_forged_pkg() {
    let (application, services, workspaces, _) = fixture();
    let package = pkg("iso-8583", "1.0.0");
    let listener_id = configure_relay(
        &services,
        &workspaces,
        &package,
        DirectionProcessingOptions {
            decode_enabled: true,
            encode_enabled: true,
        },
        DirectionProcessingOptions::default(),
    )
    .await;
    let created = application
        .socket_rule_save(input(
            listener_id,
            package.clone(),
            SocketDirection::Upstream,
            0,
        ))
        .await
        .unwrap();
    let describe_calls = services.describe_calls.load(Ordering::SeqCst);

    let mut forged = input(
        ListenerId::new(),
        pkg("other", "9.0.0"),
        SocketDirection::Downstream,
        1,
    );
    forged.rule_id = Some(created.rule_id());
    forged.expected_revision = Some(created.revision().get());
    assert_eq!(
        error_code(&application.socket_rule_save(forged).await.unwrap_err()),
        "SOCKET_RULE_BINDING_IMMUTABLE"
    );
    assert_eq!(
        services.describe_calls.load(Ordering::SeqCst),
        describe_calls
    );

    let mut stale = input(listener_id, package, SocketDirection::Upstream, 2);
    stale.rule_id = Some(created.rule_id());
    stale.expected_revision = Some(0);
    assert_eq!(
        error_code(&application.socket_rule_save(stale).await.unwrap_err()),
        "REVISION_CONFLICT"
    );
    assert_eq!(
        services.describe_calls.load(Ordering::SeqCst),
        describe_calls
    );
}

#[tokio::test]
async fn create_update_toggle_delete_keep_monotonic_order_revision_and_stable_sort() {
    let (application, services, workspaces, _) = fixture();
    let package = pkg("iso-8583", "1.0.0");
    let listener_id = configure_relay(
        &services,
        &workspaces,
        &package,
        DirectionProcessingOptions {
            decode_enabled: true,
            encode_enabled: true,
        },
        DirectionProcessingOptions::default(),
    )
    .await;
    let first = application
        .socket_rule_save(input(
            listener_id,
            package.clone(),
            SocketDirection::Upstream,
            10,
        ))
        .await
        .unwrap();
    let second = application
        .socket_rule_save(input(
            listener_id,
            package.clone(),
            SocketDirection::Upstream,
            0,
        ))
        .await
        .unwrap();
    let third = application
        .socket_rule_save(input(
            listener_id,
            package.clone(),
            SocketDirection::Upstream,
            10,
        ))
        .await
        .unwrap();
    assert!(first.created_order() < second.created_order());
    assert!(second.created_order() < third.created_order());
    assert_eq!(
        application
            .socket_rule_list()
            .await
            .unwrap()
            .iter()
            .map(SocketDocumentRuleDefinition::rule_id)
            .collect::<Vec<_>>(),
        [second.rule_id(), first.rule_id(), third.rule_id()]
    );

    application
        .socket_rule_delete(third.rule_id(), third.revision().get(), true)
        .await
        .unwrap();
    let replacement = application
        .socket_rule_save(input(
            listener_id,
            package.clone(),
            SocketDirection::Upstream,
            10,
        ))
        .await
        .unwrap();
    assert!(replacement.created_order() > third.created_order());

    let mut update = input(listener_id, package, SocketDirection::Upstream, -1);
    update.rule_id = Some(first.rule_id());
    update.expected_revision = Some(first.revision().get());
    let updated = application.socket_rule_save(update).await.unwrap();
    assert_eq!(updated.created_order(), first.created_order());
    assert_eq!(updated.revision().get(), first.revision().get() + 1);
    let toggled = application
        .socket_rule_toggle(updated.rule_id(), updated.revision().get(), false)
        .await
        .unwrap();
    assert!(!toggled.enabled());
    assert_eq!(toggled.revision().get(), updated.revision().get() + 1);
    assert_eq!(
        error_code(
            &application
                .socket_rule_delete(toggled.rule_id(), updated.revision().get(), true)
                .await
                .unwrap_err()
        ),
        "REVISION_CONFLICT"
    );
    application
        .socket_rule_delete(toggled.rule_id(), toggled.revision().get(), true)
        .await
        .unwrap();
    assert_eq!(application.socket_rule_list().await.unwrap().len(), 2);
}

#[tokio::test]
async fn relay_and_local_responder_enforce_decode_encode_and_direction_matrix() {
    let (application, services, workspaces, _) = fixture();
    let package = pkg("iso-8583", "1.0.0");
    let listener_id = configure_relay(
        &services,
        &workspaces,
        &package,
        DirectionProcessingOptions::default(),
        DirectionProcessingOptions::default(),
    )
    .await;
    assert_eq!(
        error_code(
            &application
                .socket_rule_capabilities(listener_id, SocketDirection::Upstream)
                .await
                .unwrap_err()
        ),
        "SOCKET_RULE_DECODE_REQUIRED"
    );

    let selected = workspaces.list().await.unwrap().remove(0);
    let mut workspace = workspaces.get(selected.id).await.unwrap();
    let ListenerDataPlane::Socket(settings) = &mut workspace.listeners[0].data_plane else {
        unreachable!()
    };
    settings.topology = SocketTopology::LocalResponder(SocketLocalResponderTopology::default());
    if let SocketPayloadProcessing::Scripted(scripted) = &mut settings.processing {
        scripted.downstream = DirectionProcessingOptions {
            decode_enabled: false,
            encode_enabled: true,
        };
    }
    workspaces.save(workspace).await.unwrap();
    let mut static_response = input(listener_id, package.clone(), SocketDirection::Downstream, 0);
    static_response.conditions = vec![equals("trace_id", DocumentValue::String("x".into()))];
    static_response.actions = vec![set("trace_id", DocumentValue::String("00".into()))];
    application.socket_rule_save(static_response).await.unwrap();
    assert_eq!(
        error_code(
            &application
                .socket_rule_capabilities(listener_id, SocketDirection::Upstream)
                .await
                .unwrap_err()
        ),
        "SOCKET_RULE_DIRECTION_INVALID"
    );

    let selected = workspaces.list().await.unwrap().remove(0);
    let mut workspace = workspaces.get(selected.id).await.unwrap();
    let ListenerDataPlane::Socket(settings) = &mut workspace.listeners[0].data_plane else {
        unreachable!()
    };
    let SocketPayloadProcessing::Scripted(scripted) = &mut settings.processing else {
        unreachable!()
    };
    scripted.downstream.encode_enabled = false;
    workspace.socket_rules.clear();
    workspaces.save(workspace).await.unwrap();
    let mut modification = input(listener_id, package, SocketDirection::Downstream, 1);
    modification.actions = vec![DocumentAction::ClearDocument];
    assert_eq!(
        error_code(
            &application
                .socket_rule_save(modification)
                .await
                .unwrap_err()
        ),
        "SOCKET_RULE_ENCODE_REQUIRED"
    );
}
