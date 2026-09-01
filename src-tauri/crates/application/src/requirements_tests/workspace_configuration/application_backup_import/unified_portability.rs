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
                one_shot: false,
                content: RuleContent::Socket(SocketRuleContent {
                    package: package.clone(),
                    conditions: vec![
                        Condition::DocumentPattern {
                            path: DocumentMatchPath::parse("/raw/*").unwrap(),
                            predicate: DocumentPredicate::Number(NumberPredicate {
                                operator: NumberOperator::Equal,
                                value: DocumentNumber::new(7.0).unwrap(),
                            }),
                        },
                        Condition::NthHit { count: 2 },
                        Condition::Document {
                            path: JsonPointer::property("amount"),
                            predicate: DocumentPredicate::Number(NumberPredicate {
                                operator: NumberOperator::Equal,
                                value: DocumentNumber::new(1234.0).unwrap(),
                            }),
                        },
                    ],
                    actions: vec![
                        UnifiedAction::Document(DocumentMutation::Insert {
                            path: JsonPointer::property("raw"),
                            index: 0,
                            value: DocumentValue::integer(1).unwrap(),
                        }),
                        UnifiedAction::Document(DocumentMutation::Append {
                            path: JsonPointer::property("raw"),
                            value: DocumentValue::integer(2).unwrap(),
                        }),
                    ],
                }),
            },
            1,
        )
        .unwrap(),
    ];
    workspace.rule_created_order_high_water = 1;
    workspace.validate().unwrap();
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
