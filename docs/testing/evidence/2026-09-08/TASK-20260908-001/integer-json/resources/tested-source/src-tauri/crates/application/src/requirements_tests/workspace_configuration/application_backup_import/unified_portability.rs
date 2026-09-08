use super::*;

#[tokio::test]
async fn unified_flat_wildcard_insert_append_rules_pass_portable_binding_preflight() {
    use intercept_proxy_domain::{
        Condition, DocumentMatchPath, DocumentMutation, DocumentNumber, DocumentPredicate,
        DocumentValue, JsonPointer, NumberOperator, NumberPredicate, RuleContent, RuleDefinition,
        RuleDefinitionDraft, RuleStage, SocketRuleContent, UnifiedAction,
    };

    let (application, portability, _, _) = prepared_application();
    let package = protocol_package("unified-portable", "1.0.0");
    let mut workspace = scripted_workspace(package.clone(), false);
    let listener_id = workspace.listeners[0].id;
    workspace.rule_definitions = vec![
        RuleDefinition::create(
            RuleDefinitionDraft {
                name: "portable unified rule".into(),
                enabled: true,
                priority: 0,
                listener_id,
                stage: RuleStage::ProxyToUpstream,
                content: RuleContent::Socket(SocketRuleContent {
                    package: package.clone(),
                    condition: Condition::DocumentPattern {
                        path: DocumentMatchPath::parse("/raw/*").unwrap(),
                        predicate: DocumentPredicate::Number(NumberPredicate {
                            operator: NumberOperator::Equal,
                            value: DocumentNumber::new(7.0).unwrap(),
                        }),
                    },
                    action: UnifiedAction::Document(DocumentMutation::Insert {
                        path: JsonPointer::property("raw").into(),
                        index: 0,
                        value: DocumentValue::integer(1).unwrap(),
                    }),
                }),
            },
            1,
        )
        .unwrap(),
    ];
    workspace.rule_created_order_high_water = 1;
    workspace.validate().unwrap();

    let mut legacy_wire = serde_json::to_value(&workspace).unwrap();
    legacy_wire["rule_definitions"][0]["content"]["value"]["condition"]["predicate"]["value"]["value"] =
        serde_json::from_str("7.0").unwrap();
    legacy_wire["rule_definitions"][0]["content"]["value"]["action"]["value"]["value"] =
        serde_json::from_str("1.0").unwrap();
    let workspace: intercept_proxy_domain::ProxyWorkspace =
        serde_json::from_value(legacy_wire).expect("legacy floating-point rule values load");
    let saved_wire = serde_json::to_value(&workspace).expect("workspace configuration saves");
    assert_eq!(
        saved_wire.pointer("/rule_definitions/0/content/value/condition/predicate/value/value"),
        Some(&serde_json::json!(7))
    );
    assert_eq!(
        saved_wire.pointer("/rule_definitions/0/content/value/action/value/value"),
        Some(&serde_json::json!(1))
    );
    assert_eq!(
        saved_wire.pointer("/rule_definitions/0/content/value/package/version"),
        Some(&serde_json::json!("1.0.0"))
    );
    assert_eq!(
        serde_json::from_value::<intercept_proxy_domain::ProxyWorkspace>(saved_wire).unwrap(),
        workspace
    );

    let prepared = ApplicationBackupImportCandidate {
        selected_workspace_id: workspace.id,
        workspaces: vec![workspace],
        settings: PortableSettings::from(&SettingsDraft::default()),
        protocol_packages: vec![portable_protocol_package(package.clone(), true)],
        certificate_materials: Vec::new(),
    };
    portability.register(
        prepared.protocol_packages[0].clone(),
        protocol_package_description(package),
    );
    let source = FakeBackupPrepareSource::new(prepared);

    application
        .application_backup_import_prepare(&source, Vec::new())
        .await
        .expect("current unified rule shapes must pass portable binding preflight");
}
