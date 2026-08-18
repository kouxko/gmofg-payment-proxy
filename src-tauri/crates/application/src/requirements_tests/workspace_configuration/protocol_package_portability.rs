use super::*;

pub(super) fn package(id: &str, version: &str) -> ProtocolPackageRef {
    ProtocolPackageRef {
        id: ProtocolPackageId::new(id).unwrap(),
        version: ProtocolPackageVersion::new(version).unwrap(),
    }
}

pub(super) fn portable_package(
    package: ProtocolPackageRef,
    enabled: bool,
) -> PortableApplicationProtocolPackage {
    PortableApplicationProtocolPackage {
        package,
        files: vec![PortableProtocolPackageFile {
            path: "manifest.toml".into(),
            contents_base64: "bWFuaWZlc3Q=".into(),
        }],
        enabled,
    }
}

pub(super) fn description(package: ProtocolPackageRef) -> ProtocolPackageDescriptionViewModel {
    ProtocolPackageDescriptionViewModel {
        package,
        capabilities: ProtocolPackageCapabilitiesViewModel {
            upstream: ProtocolPackageDirectionCapabilitiesViewModel {
                frame: true,
                decode: true,
                encode: true,
            },
            downstream: ProtocolPackageDirectionCapabilitiesViewModel {
                frame: true,
                decode: true,
                encode: true,
            },
            display: true,
        },
        schema: ProtocolPackageSchemaViewModel {
            id: "portable-message".into(),
            version: 7,
            title: "Portable Message".into(),
            fields: [
                ("text", ProtocolPackageSchemaFieldTypeViewModel::String),
                ("amount", ProtocolPackageSchemaFieldTypeViewModel::Int),
                ("approved", ProtocolPackageSchemaFieldTypeViewModel::Bool),
                ("raw", ProtocolPackageSchemaFieldTypeViewModel::Blob),
            ]
            .into_iter()
            .map(|(name, field_type)| ProtocolPackageSchemaFieldViewModel {
                name: name.into(),
                label: name.into(),
                field_type,
            })
            .collect(),
        },
    }
}

pub(super) fn scripted_workspace(
    package: ProtocolPackageRef,
    local_responder: bool,
) -> ProxyWorkspace {
    let mut workspace = ProxyWorkspace::default();
    let listener = &mut workspace.listeners[0];
    let topology = if local_responder {
        SocketTopology::LocalResponder(SocketLocalResponderTopology {
            downstream_security: SocketDownstreamSecurity::Tcp,
        })
    } else {
        SocketTopology::Relay(SocketRelayTopology {
            upstream: SocketEndpoint {
                host: "127.0.0.1".into(),
                port: 9_001,
            },
            security: SocketRelaySecurity::Transparent,
        })
    };
    listener.data_plane = ListenerDataPlane::Socket(SocketRelaySettings {
        topology,
        maximum_connections: 8,
        processing: SocketPayloadProcessing::Scripted(ScriptedSocketProcessing {
            package: package.clone(),
            upstream: DirectionProcessingOptions {
                decode_enabled: true,
                encode_enabled: !local_responder,
            },
            downstream: DirectionProcessingOptions {
                decode_enabled: !local_responder,
                encode_enabled: true,
            },
        }),
    });
    let direction = if local_responder {
        SocketDirection::Downstream
    } else {
        SocketDirection::Upstream
    };
    workspace.socket_rules.push(
        SocketDocumentRuleDefinition::new(
            SocketDocumentRuleId::new(),
            true,
            -10,
            41,
            listener.id,
            package,
            7,
            direction,
            vec![
                equals("text", DocumentValue::String("sale".into())),
                equals("amount", DocumentValue::Int(1234)),
                equals("approved", DocumentValue::Bool(true)),
                equals("raw", DocumentValue::Blob(vec![0, 1, 2, 255])),
            ],
            vec![
                DocumentAction::RecordMatch,
                set("text", DocumentValue::String("reply".into())),
                set("amount", DocumentValue::Int(4321)),
                set("approved", DocumentValue::Bool(false)),
                set("raw", DocumentValue::Blob(vec![9, 8, 7])),
            ],
        )
        .unwrap(),
    );
    workspace.socket_rule_created_order_high_water = 41;
    workspace.validate().unwrap();
    workspace
}

fn equals(name: &str, value: DocumentValue) -> DocumentCondition {
    DocumentCondition::Equals {
        field: DocumentFieldName::new(name).unwrap(),
        value,
    }
}

fn set(name: &str, value: DocumentValue) -> DocumentAction {
    DocumentAction::SetField {
        field: DocumentFieldName::new(name).unwrap(),
        value,
    }
}

fn app_fixture(
    workspaces: Arc<InMemoryWorkspaceStore>,
    documents: Arc<InMemoryWorkspaceDocumentStore>,
    configuration_store: Arc<dyn ApplicationConfigurationStorePort>,
) -> (
    Application,
    Arc<FakePorts>,
    Arc<FakeProtocolPackagePortability>,
) {
    let ports = Arc::new(FakePorts::default());
    let (application, portability) = application_with_workspace_configuration_and_packages(
        ports.clone(),
        workspaces,
        documents,
        configuration_store,
    );
    (application, ports, portability)
}

#[test]
fn portable_binding_validator_rejects_wrong_identity_schema_and_capability() {
    let expected_package = package("validator", "1.0.0");
    let workspace = scripted_workspace(expected_package.clone(), false);

    let wrong_identity = description(package("other", "1.0.0"));
    assert_eq!(
        validate_portable_protocol_bindings(
            std::slice::from_ref(&workspace),
            std::slice::from_ref(&expected_package),
            &[wrong_identity],
        )
        .unwrap_err()
        .view_model
        .code,
        "PORTABLE_PROTOCOL_PACKAGE_INVALID"
    );

    let mut wrong_schema = description(expected_package.clone());
    wrong_schema.schema.fields[1].field_type = ProtocolPackageSchemaFieldTypeViewModel::String;
    assert_eq!(
        validate_portable_protocol_bindings(
            std::slice::from_ref(&workspace),
            std::slice::from_ref(&expected_package),
            &[wrong_schema],
        )
        .unwrap_err()
        .view_model
        .code,
        "RULE_INVALID"
    );

    let mut missing_capability = description(expected_package.clone());
    missing_capability.capabilities.upstream.encode = false;
    assert_eq!(
        validate_portable_protocol_bindings(
            std::slice::from_ref(&workspace),
            std::slice::from_ref(&expected_package),
            &[missing_capability],
        )
        .unwrap_err()
        .view_model
        .code,
        "PROTOCOL_PACKAGE_CAPABILITY_MISMATCH"
    );
}

#[tokio::test]
async fn workspace_v4_round_trip_embeds_exact_package_and_preserves_typed_rules() {
    for local_responder in [false, true] {
        let workspaces = Arc::new(InMemoryWorkspaceStore::new_empty());
        let documents = Arc::new(InMemoryWorkspaceDocumentStore::default());
        let package = package("portable", "1.2.3");
        let source = workspaces
            .import_workspace(scripted_workspace(package.clone(), local_responder))
            .await
            .unwrap();
        workspaces.select(source.id).await.unwrap();
        let (application, _, portability) = app_fixture(
            workspaces.clone(),
            documents.clone(),
            Arc::new(UnavailableApplicationConfigurationStore),
        );
        portability.register(
            portable_package(package.clone(), true),
            description(package.clone()),
        );

        application.workspace_export(source.id).await.unwrap();
        let (_, bytes) = documents.take_last_export().unwrap();
        let json: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
        assert!(json["protocol_packages"][0].get("enabled").is_none());
        let exported = parse_workspace_document(&bytes).unwrap();
        assert_eq!(exported.protocol_packages.len(), 1);
        assert_eq!(exported.protocol_packages[0].package, package);
        assert_eq!(exported.workspace.socket_rules, source.socket_rules);

        documents.set_next_import(bytes);
        application.workspace_import().await.unwrap();
        assert_eq!(portability.preflight_calls.load(Ordering::SeqCst), 1);
        assert_eq!(portability.commit_calls.load(Ordering::SeqCst), 1);
        let imported = workspaces
            .list()
            .await
            .unwrap()
            .into_iter()
            .find(|summary| summary.id != source.id)
            .map(|summary| workspaces.get(summary.id))
            .unwrap()
            .await
            .unwrap();
        assert_eq!(
            imported.socket_rules[0].rule_id(),
            source.socket_rules[0].rule_id()
        );
        assert_eq!(imported.socket_rules[0].created_order(), 41);
        assert_ne!(
            imported.socket_rules[0].listener_id(),
            source.listeners[0].id
        );
        assert_eq!(
            imported.socket_rules[0].listener_id(),
            imported.listeners[0].id
        );
    }
}

#[tokio::test]
async fn full_v4_export_keeps_entire_registry_enabled_state_and_shared_reference() {
    let workspaces = Arc::new(InMemoryWorkspaceStore::new_empty());
    let documents = Arc::new(InMemoryWorkspaceDocumentStore::default());
    let first = package("shared", "1.0.0");
    let extra = package("unused", "2.0.0");
    let source = workspaces
        .import_workspace(scripted_workspace(first.clone(), false))
        .await
        .unwrap();
    workspaces.select(source.id).await.unwrap();
    let (application, _, portability) = app_fixture(
        workspaces,
        documents.clone(),
        Arc::new(UnavailableApplicationConfigurationStore),
    );
    portability.register(
        portable_package(first.clone(), false),
        description(first.clone()),
    );
    portability.register(
        portable_package(extra.clone(), true),
        description(extra.clone()),
    );

    application
        .application_configuration_export()
        .await
        .unwrap();
    let (_, bytes) = documents.take_last_export().unwrap();
    let exported = parse_application_configuration(&bytes).unwrap();
    assert_eq!(exported.protocol_packages.len(), 2);
    assert!(
        exported
            .protocol_packages
            .iter()
            .any(|item| item.package == first && !item.enabled)
    );
    assert!(
        exported
            .protocol_packages
            .iter()
            .any(|item| item.package == extra && item.enabled)
    );
}

#[tokio::test]
async fn full_v4_import_preflights_once_and_atomically_replaces_shared_package_bundle() {
    let workspaces = Arc::new(InMemoryWorkspaceStore::default());
    let documents = Arc::new(InMemoryWorkspaceDocumentStore::default());
    let configuration_store = Arc::new(RecordingConfigurationStore::default());
    let package = package("shared", "3.0.0");
    let first = scripted_workspace(package.clone(), false);
    let mut second = first.clone();
    remap_workspace_identity(&mut second).unwrap();
    let portable = portable_package(package.clone(), false);
    let document = ApplicationConfigurationDocument {
        format_version: APPLICATION_CONFIGURATION_FORMAT_VERSION,
        selected_workspace_id: first.id,
        workspaces: vec![first, second],
        settings: PortableSettings::from(&SettingsDraft::default()),
        certificate_materials: Vec::new(),
        protocol_packages: vec![portable.clone()],
    };
    documents.set_next_import(serialize_application_configuration(&document).unwrap());
    let (application, _, portability) =
        app_fixture(workspaces, documents, configuration_store.clone());
    portability.register(portable, description(package));

    application
        .application_configuration_import()
        .await
        .unwrap();
    assert_eq!(portability.preflight_calls.load(Ordering::SeqCst), 1);
    assert_eq!(portability.replace_calls.load(Ordering::SeqCst), 1);
    let stored = configuration_store.document.lock().clone().unwrap();
    assert_eq!(stored.workspaces.len(), 2);
    assert_eq!(stored.protocol_packages.len(), 1);
    assert!(!stored.protocol_packages[0].enabled);
}

#[tokio::test]
async fn legacy_v3_requires_pure_fresh_local_preflight_and_preserves_registry() {
    let workspaces = Arc::new(InMemoryWorkspaceStore::new_empty());
    let documents = Arc::new(InMemoryWorkspaceDocumentStore::default());
    let legacy_package = package("legacy", "1.0.0");
    let mut source = scripted_workspace(legacy_package.clone(), false);
    source.socket_rules.clear();
    source.socket_rule_created_order_high_water = 0;
    let mut value = serde_json::to_value(WorkspaceDocument {
        format_version: WORKSPACE_DOCUMENT_FORMAT_VERSION,
        workspace: source,
        certificate_materials: Vec::new(),
        protocol_packages: vec![PortableProtocolPackage {
            package: legacy_package.clone(),
            files: portable_package(legacy_package.clone(), true).files,
        }],
    })
    .unwrap();
    value["format_version"] = serde_json::json!(WORKSPACE_DOCUMENT_V3_FORMAT_VERSION);
    value.as_object_mut().unwrap().remove("protocol_packages");
    let workspace = value["workspace"].as_object_mut().unwrap();
    workspace.insert("metadata_extractors".into(), serde_json::json!([]));
    workspace.remove("socket_rules");
    workspace.remove("socket_rule_created_order_high_water");
    documents.set_next_import(serde_json::to_vec(&value).unwrap());
    let (application, _, portability) = app_fixture(
        workspaces,
        documents,
        Arc::new(UnavailableApplicationConfigurationStore),
    );
    portability.register(
        portable_package(legacy_package.clone(), false),
        description(legacy_package),
    );

    application.workspace_import().await.unwrap();
    assert_eq!(
        portability.installed_preflight_calls.load(Ordering::SeqCst),
        1
    );
    assert_eq!(portability.preflight_calls.load(Ordering::SeqCst), 0);
    assert_eq!(portability.commit_calls.load(Ordering::SeqCst), 0);
    assert_eq!(portability.legacy_commit_calls.load(Ordering::SeqCst), 1);
}

#[tokio::test]
async fn syntax_topology_and_rule_schema_failures_make_zero_atomic_writes() {
    let package = package("invalid", "1.0.0");
    let workspace = scripted_workspace(package.clone(), false);
    let portable = portable_package(package.clone(), true);

    assert_eq!(
        workspace_import_failure(
            None,
            package.clone(),
            workspace.clone(),
            portable.clone(),
            true,
            false,
        )
        .await,
        "SCRIPT_SYNTAX_INVALID"
    );
    assert_eq!(
        workspace_import_failure(
            None,
            package.clone(),
            workspace.clone(),
            portable.clone(),
            false,
            true,
        )
        .await,
        "RULE_INVALID"
    );

    let valid = WorkspaceDocument {
        format_version: WORKSPACE_DOCUMENT_FORMAT_VERSION,
        workspace,
        certificate_materials: Vec::new(),
        protocol_packages: vec![PortableProtocolPackage {
            package: portable.package.clone(),
            files: portable.files.clone(),
        }],
    };
    let mut forged: serde_json::Value =
        serde_json::from_slice(&serialize_workspace_document(&valid).unwrap()).unwrap();
    forged["workspace"]["listeners"][0]["data_plane"]["settings"]["topology"]["kind"] =
        serde_json::json!("local_responder");
    assert_eq!(
        workspace_import_failure(
            Some(serde_json::to_vec(&forged).unwrap()),
            package,
            valid.workspace,
            portable,
            false,
            false,
        )
        .await,
        "IMPORT_FAILED"
    );
}

async fn workspace_import_failure(
    bytes: Option<Vec<u8>>,
    package: ProtocolPackageRef,
    workspace: ProxyWorkspace,
    portable: PortableApplicationProtocolPackage,
    syntax_failure: bool,
    wrong_schema: bool,
) -> String {
    let workspaces = Arc::new(InMemoryWorkspaceStore::new_empty());
    let documents = Arc::new(InMemoryWorkspaceDocumentStore::default());
    let bytes = bytes.unwrap_or_else(|| {
        serialize_workspace_document(&WorkspaceDocument {
            format_version: WORKSPACE_DOCUMENT_FORMAT_VERSION,
            workspace,
            certificate_materials: Vec::new(),
            protocol_packages: vec![PortableProtocolPackage {
                package: portable.package.clone(),
                files: portable.files.clone(),
            }],
        })
        .unwrap()
    });
    documents.set_next_import(bytes);
    let (application, _, portability) = app_fixture(
        workspaces,
        documents,
        Arc::new(UnavailableApplicationConfigurationStore),
    );
    let mut package_description = description(package);
    if wrong_schema {
        package_description.schema.fields[1].field_type =
            ProtocolPackageSchemaFieldTypeViewModel::String;
    }
    portability.register(portable, package_description);
    portability
        .fail_preflight
        .store(syntax_failure, Ordering::SeqCst);
    let error = application.workspace_import().await.unwrap_err();
    assert_eq!(portability.commit_calls.load(Ordering::SeqCst), 0);
    error.view_model.code
}
