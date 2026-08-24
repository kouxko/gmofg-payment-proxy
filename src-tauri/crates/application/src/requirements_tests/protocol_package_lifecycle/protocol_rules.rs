use super::*;

mod concurrency;
mod lifecycle;
mod listener_modes;
mod topology;

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
    direction: ProtocolDirection,
    priority: i32,
) -> ProtocolRuleSaveInput {
    ProtocolRuleSaveInput {
        rule_id: None,
        expected_revision: None,
        name: "测试规则".into(),
        enabled: true,
        priority,
        listener_id,
        package,
        schema_version: match direction {
            ProtocolDirection::Upstream => 1,
            ProtocolDirection::Downstream => 2,
        },
        stage: match direction {
            ProtocolDirection::Upstream => ProtocolRuleStage::ProxyToUpstream,
            ProtocolDirection::Downstream => ProtocolRuleStage::ProxyToApp,
        },
        conditions: Vec::new(),
        actions: vec![DocumentAction::RecordMatch],
    }
}

fn description_with_blob(package: ProtocolPackageRef) -> ProtocolPackageDescriptionViewModel {
    let mut value = description(package);
    value.capabilities.downstream.encode = true;
    value
        .upstream_schema
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

#[tokio::test]
async fn http_and_socket_entries_reject_cross_kind_rule_capabilities_and_save() {
    for entry_kind in [
        ProtocolPackageKindViewModel::Http,
        ProtocolPackageKindViewModel::Socket,
    ] {
        let (application, services, workspaces, _) = fixture();
        let package = pkg("kind-mismatch", "1.0.0");
        let listener_id = match entry_kind {
            ProtocolPackageKindViewModel::Http => {
                configure_http(&services, &workspaces, &package).await
            }
            ProtocolPackageKindViewModel::Socket => {
                configure_relay(&services, &workspaces, &package).await
            }
        };
        let mut wrong_description = description_with_blob(package.clone());
        wrong_description.kind = match entry_kind {
            ProtocolPackageKindViewModel::Http => ProtocolPackageKindViewModel::Socket,
            ProtocolPackageKindViewModel::Socket => ProtocolPackageKindViewModel::Http,
        };
        services.set_description(package.clone(), wrong_description);

        let capabilities_error = application
            .protocol_rule_capabilities(listener_id, ProtocolRuleStage::AppToProxy)
            .await
            .unwrap_err();
        assert_eq!(
            error_code(&capabilities_error),
            "PROTOCOL_PACKAGE_KIND_MISMATCH"
        );

        let save_error = application
            .protocol_rule_save(input(listener_id, package, ProtocolDirection::Upstream, 0))
            .await
            .unwrap_err();
        assert_eq!(error_code(&save_error), "PROTOCOL_PACKAGE_KIND_MISMATCH");
    }
}

#[tokio::test]
async fn capability_catalog_is_schema_typed_and_has_no_http_surface() {
    let (application, services, workspaces, _) = fixture();
    let package = pkg("iso-8583", "1.0.0");
    let listener_id = configure_relay(&services, &workspaces, &package).await;

    let upstream = application
        .protocol_rule_capabilities(listener_id, ProtocolRuleStage::ProxyToUpstream)
        .await
        .unwrap();
    assert_eq!(upstream.package, package);
    assert_eq!(upstream.schema_version, 1);
    assert_eq!(upstream.fields.len(), 4);
    assert!(upstream.fields.iter().all(|item| {
        item.operators == [ProtocolRuleFieldOperatorCapability::Equals]
            && item.actions
                == [
                    ProtocolRuleFieldActionCapability::SetField,
                    ProtocolRuleFieldActionCapability::ClearField,
                ]
    }));
    assert_eq!(
        upstream.common_actions,
        [
            ProtocolRuleCommonActionCapability::RecordMatch,
            ProtocolRuleCommonActionCapability::ClearDocument,
        ]
    );

    let downstream = application
        .protocol_rule_capabilities(listener_id, ProtocolRuleStage::ProxyToApp)
        .await
        .unwrap();
    assert!(downstream.fields.iter().all(|item| {
        item.actions
            == [
                ProtocolRuleFieldActionCapability::SetField,
                ProtocolRuleFieldActionCapability::ClearField,
            ]
    }));
    assert_eq!(
        downstream.common_actions,
        [
            ProtocolRuleCommonActionCapability::RecordMatch,
            ProtocolRuleCommonActionCapability::ClearDocument,
        ]
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
        ProtocolDirection::Upstream,
        0,
    ))
    .unwrap();
    forged["http_status"] = serde_json::json!(500);
    assert!(serde_json::from_value::<ProtocolRuleSaveInput>(forged).is_err());
}

#[tokio::test]
async fn save_validates_all_four_schema_value_types_and_exact_bindings() {
    let (application, services, workspaces, _) = fixture();
    let package = pkg("iso-8583", "1.0.0");
    let listener_id = configure_relay(&services, &workspaces, &package).await;
    let mut valid = input(listener_id, package.clone(), ProtocolDirection::Upstream, 0);
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
    application.protocol_rule_save(valid).await.unwrap();

    let mut wrong_type = input(listener_id, package.clone(), ProtocolDirection::Upstream, 1);
    wrong_type.conditions = vec![equals("amount", DocumentValue::String("1000".into()))];
    assert_eq!(
        error_code(
            &application
                .protocol_rule_save(wrong_type)
                .await
                .unwrap_err()
        ),
        "RULE_INVALID"
    );

    let mut wrong_package = input(
        listener_id,
        pkg("iso-8583", "2.0.0"),
        ProtocolDirection::Upstream,
        1,
    );
    assert_eq!(
        error_code(
            &application
                .protocol_rule_save(wrong_package.clone())
                .await
                .unwrap_err()
        ),
        "PROTOCOL_RULE_PACKAGE_MISMATCH"
    );
    wrong_package.package = package.clone();
    wrong_package.schema_version = 2;
    assert_eq!(
        error_code(
            &application
                .protocol_rule_save(wrong_package)
                .await
                .unwrap_err()
        ),
        "PROTOCOL_RULE_SCHEMA_MISMATCH"
    );
}

#[tokio::test]
async fn update_preserves_binding_rejects_stale_revision_and_never_queries_forged_pkg() {
    let (application, services, workspaces, _) = fixture();
    let package = pkg("iso-8583", "1.0.0");
    let listener_id = configure_relay(&services, &workspaces, &package).await;
    let created = application
        .protocol_rule_save(input(
            listener_id,
            package.clone(),
            ProtocolDirection::Upstream,
            0,
        ))
        .await
        .unwrap();
    let describe_calls = services.describe_calls.load(Ordering::SeqCst);

    let mut forged = input(
        ListenerId::new(),
        pkg("other", "9.0.0"),
        ProtocolDirection::Downstream,
        1,
    );
    forged.rule_id = Some(created.rule_id());
    forged.expected_revision = Some(created.revision().get());
    assert_eq!(
        error_code(&application.protocol_rule_save(forged).await.unwrap_err()),
        "PROTOCOL_RULE_BINDING_IMMUTABLE"
    );
    assert_eq!(
        services.describe_calls.load(Ordering::SeqCst),
        describe_calls
    );

    let mut stale = input(listener_id, package, ProtocolDirection::Upstream, 2);
    stale.rule_id = Some(created.rule_id());
    stale.expected_revision = Some(0);
    assert_eq!(
        error_code(&application.protocol_rule_save(stale).await.unwrap_err()),
        "REVISION_CONFLICT"
    );
    assert_eq!(
        services.describe_calls.load(Ordering::SeqCst),
        describe_calls
    );
}

#[tokio::test]
async fn save_rejects_missing_entry_and_incomplete_update_identity() {
    let (application, _, _, _) = fixture();
    let package = pkg("iso-8583", "1.0.0");
    let missing_listener = ListenerId::new();

    let missing_entry = application
        .protocol_rule_save(input(
            missing_listener,
            package.clone(),
            ProtocolDirection::Upstream,
            0,
        ))
        .await
        .unwrap_err();
    assert_eq!(
        error_code(&missing_entry),
        "PROTOCOL_RULE_LISTENER_NOT_FOUND"
    );

    let mut incomplete = input(missing_listener, package, ProtocolDirection::Upstream, 0);
    incomplete.rule_id = Some(ProtocolDocumentRuleId::new());
    let incomplete_update = application
        .protocol_rule_save(incomplete)
        .await
        .unwrap_err();
    assert_eq!(
        error_code(&incomplete_update),
        "PROTOCOL_RULE_REVISION_REQUIRED"
    );
}

#[tokio::test]
async fn create_update_toggle_delete_keep_monotonic_order_revision_and_stable_sort() {
    let (application, services, workspaces, _) = fixture();
    let package = pkg("iso-8583", "1.0.0");
    let listener_id = configure_relay(&services, &workspaces, &package).await;
    let first = application
        .protocol_rule_save(input(
            listener_id,
            package.clone(),
            ProtocolDirection::Upstream,
            10,
        ))
        .await
        .unwrap();
    let second = application
        .protocol_rule_save(input(
            listener_id,
            package.clone(),
            ProtocolDirection::Upstream,
            0,
        ))
        .await
        .unwrap();
    let third = application
        .protocol_rule_save(input(
            listener_id,
            package.clone(),
            ProtocolDirection::Upstream,
            10,
        ))
        .await
        .unwrap();
    assert!(first.created_order() < second.created_order());
    assert!(second.created_order() < third.created_order());
    assert_eq!(
        application
            .protocol_rule_list()
            .await
            .unwrap()
            .iter()
            .map(ProtocolDocumentRuleDefinition::rule_id)
            .collect::<Vec<_>>(),
        [second.rule_id(), first.rule_id(), third.rule_id()]
    );

    application
        .protocol_rule_delete(third.rule_id(), third.revision().get(), true)
        .await
        .unwrap();
    let replacement = application
        .protocol_rule_save(input(
            listener_id,
            package.clone(),
            ProtocolDirection::Upstream,
            10,
        ))
        .await
        .unwrap();
    assert!(replacement.created_order() > third.created_order());

    let mut update = input(listener_id, package, ProtocolDirection::Upstream, -1);
    update.rule_id = Some(first.rule_id());
    update.expected_revision = Some(first.revision().get());
    let updated = application.protocol_rule_save(update).await.unwrap();
    assert_eq!(updated.created_order(), first.created_order());
    assert_eq!(updated.revision().get(), first.revision().get() + 1);
    let toggled = application
        .protocol_rule_toggle(updated.rule_id(), updated.revision().get(), false)
        .await
        .unwrap();
    assert!(!toggled.enabled());
    assert_eq!(toggled.revision().get(), updated.revision().get() + 1);
    assert_eq!(
        error_code(
            &application
                .protocol_rule_delete(toggled.rule_id(), updated.revision().get(), true)
                .await
                .unwrap_err()
        ),
        "REVISION_CONFLICT"
    );
    application
        .protocol_rule_delete(toggled.rule_id(), toggled.revision().get(), true)
        .await
        .unwrap();
    assert_eq!(application.protocol_rule_list().await.unwrap().len(), 2);
}
