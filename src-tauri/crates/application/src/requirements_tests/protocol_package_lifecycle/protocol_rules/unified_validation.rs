use super::*;
use intercept_proxy_domain::{
    ConditionTree, HttpDocumentRuleContent, HttpRuleContent, SocketRuleContent, UnifiedAction,
};

fn document_tree(conditions: Vec<ConditionTree>) -> ConditionTree {
    ConditionTree::All(conditions)
}

fn document_actions(actions: Vec<UnifiedAction>) -> Vec<UnifiedAction> {
    actions
}

fn http_tree(conditions: Vec<intercept_proxy_domain::Condition>) -> ConditionTree {
    ConditionTree::All(conditions.into_iter().map(ConditionTree::Leaf).collect())
}

fn http_actions(actions: Vec<intercept_proxy_domain::HttpAction>) -> Vec<UnifiedAction> {
    actions.into_iter().map(UnifiedAction::from).collect()
}

fn unified_socket_input(
    listener_id: ListenerId,
    package: ProtocolPackageRef,
    stage: RuleStage,
) -> RuleDefinitionSaveInput {
    RuleDefinitionSaveInput {
        rule_id: None,
        expected_revision: None,
        draft: RuleDefinitionDraft {
            name: "统一 Socket 规则".into(),
            enabled: true,
            priority: 10,
            listener_id,
            stage,
            one_shot: false,
            content: RuleContent::Socket(SocketRuleContent {
                package,
                condition: document_tree(vec![equals(
                    "trace_id",
                    DocumentValue::String("abc".into()),
                )]),
                actions: document_actions(vec![set("amount", DocumentValue::integer(2).unwrap())]),
            }),
        },
    }
}

#[tokio::test]
async fn stopped_listener_rejects_invalid_unified_document_before_persistence() {
    for invalid_case in ["package", "type"] {
        let (application, services, workspaces, runtime) = fixture();
        let package = pkg("unified-validation", "1.0.0");
        let listener_id = configure_relay(&services, &workspaces, &package).await;
        let selected = workspaces.list().await.unwrap().remove(0);
        assert!(runtime.statuses().await.unwrap().is_empty());

        let mut input =
            unified_socket_input(listener_id, package.clone(), RuleStage::ProxyToUpstream);
        let RuleContent::Socket(content) = &mut input.draft.content else {
            unreachable!()
        };
        match invalid_case {
            "package" => content.package = pkg("forged-package", "1.0.0"),
            "type" => {
                content.actions =
                    document_actions(vec![set("amount", DocumentValue::String("wrong".into()))]);
            }
            _ => unreachable!(),
        }
        let persisted_before = workspaces.get(selected.id).await.unwrap();

        let error = application.rule_definition_save(input).await.unwrap_err();

        assert_eq!(error_code(&error), "RULE_INVALID", "case={invalid_case}");
        assert_eq!(workspaces.get(selected.id).await.unwrap(), persisted_before);
        assert!(runtime.statuses().await.unwrap().is_empty());
    }
}

#[tokio::test]
async fn stopped_listener_accepts_rule_paths_missing_from_incomplete_schema_metadata() {
    let (application, services, workspaces, runtime) = fixture();
    let package = pkg("unified-validation", "1.0.0");
    let listener_id = configure_relay(&services, &workspaces, &package).await;
    let mut input = unified_socket_input(listener_id, package, RuleStage::ProxyToUpstream);
    let RuleContent::Socket(content) = &mut input.draft.content else {
        unreachable!()
    };
    content.condition = document_tree(vec![equals(
        "missing_field",
        DocumentValue::String("abc".into()),
    )]);
    content.actions = document_actions(vec![set("extension_value", DocumentValue::Boolean(true))]);

    application
        .rule_definition_save(input)
        .await
        .expect("rule-owned paths need not appear in incomplete schema metadata");
    assert!(runtime.statuses().await.unwrap().is_empty());
}

#[tokio::test]
async fn unified_document_save_enforces_direction_decode_and_encode_capabilities() {
    for (case, decode, encode, actions) in [
        ("decode", false, true, vec![UnifiedAction::RecordMatch]),
        (
            "encode",
            true,
            false,
            vec![set("amount", DocumentValue::integer(2).unwrap())],
        ),
    ] {
        let (application, services, workspaces, runtime) = fixture();
        let package = pkg("unified-capability", "1.0.0");
        let listener_id = configure_relay(&services, &workspaces, &package).await;
        let mut description = description_with_blob(package.clone());
        description.capabilities.upstream.decode = decode;
        description.capabilities.upstream.encode = encode;
        services.set_description(package.clone(), description);
        let selected = workspaces.list().await.unwrap().remove(0);
        let before = workspaces.get(selected.id).await.unwrap();
        let mut input = unified_socket_input(listener_id, package, RuleStage::ProxyToUpstream);
        let RuleContent::Socket(content) = &mut input.draft.content else {
            unreachable!()
        };
        content.actions = document_actions(actions);

        let error = application.rule_definition_save(input).await.unwrap_err();

        assert_eq!(error_code(&error), "RULE_INVALID", "case={case}");
        assert_eq!(workspaces.get(selected.id).await.unwrap(), before);
        assert!(runtime.statuses().await.unwrap().is_empty());
    }
}

#[tokio::test]
async fn stopped_listener_accepts_valid_unified_socket_and_joint_http_documents() {
    {
        let (application, services, workspaces, runtime) = fixture();
        let socket_package = pkg("unified-socket", "1.0.0");
        let socket_listener = configure_relay(&services, &workspaces, &socket_package).await;
        let socket = application
            .rule_definition_save(unified_socket_input(
                socket_listener,
                socket_package,
                RuleStage::ProxyToUpstream,
            ))
            .await
            .unwrap();
        assert!(matches!(socket.content(), RuleContent::Socket(_)));
        assert!(runtime.statuses().await.unwrap().is_empty());
    }

    let (application, services, workspaces, runtime) = fixture();
    let http_package = pkg("unified-http", "1.0.0");
    let http_listener = configure_http(&services, &workspaces, &http_package).await;
    let http = application
        .rule_definition_save(RuleDefinitionSaveInput {
            rule_id: None,
            expected_revision: None,
            draft: RuleDefinitionDraft {
                name: "联合 HTTP 规则".into(),
                enabled: true,
                priority: 10,
                listener_id: http_listener,
                stage: RuleStage::ProxyToUpstream,
                one_shot: false,
                content: RuleContent::Http(HttpRuleContent {
                    description: "header + document".into(),
                    condition: ConditionTree::All(vec![
                        http_tree(vec![intercept_proxy_domain::Condition::NthHit { count: 1 }]),
                        document_tree(vec![equals(
                            "trace_id",
                            DocumentValue::String("abc".into()),
                        )]),
                    ]),
                    actions: http_actions(vec![intercept_proxy_domain::HttpAction::Delay {
                        milliseconds: 1,
                    }])
                    .into_iter()
                    .chain(document_actions(vec![set(
                        "amount",
                        DocumentValue::integer(2).unwrap(),
                    )]))
                    .collect(),
                    document: Some(HttpDocumentRuleContent {
                        package: http_package,
                    }),
                }),
            },
        })
        .await
        .unwrap();
    assert!(matches!(http.content(), RuleContent::Http(_)));
    assert!(runtime.statuses().await.unwrap().is_empty());
}

#[tokio::test]
async fn stopped_http_listener_accepts_pure_document_and_exact_joint_stages() {
    for (stage, joint) in [
        (RuleStage::ProxyToUpstream, true),
        (RuleStage::ProxyToApp, true),
    ] {
        let (application, services, workspaces, runtime) = fixture();
        let package = pkg("http-valid-stage", "1.0.0");
        let listener_id = configure_http(&services, &workspaces, &package).await;
        let saved = application
            .rule_definition_save(RuleDefinitionSaveInput {
                rule_id: None,
                expected_revision: None,
                draft: RuleDefinitionDraft {
                    name: "合法 HTTP 阶段".into(),
                    enabled: true,
                    priority: 10,
                    listener_id,
                    stage,
                    one_shot: false,
                    content: RuleContent::Http(HttpRuleContent {
                        description: String::new(),
                        condition: document_tree(vec![equals(
                            "trace_id",
                            DocumentValue::String("abc".into()),
                        )]),
                        actions: http_actions(
                            joint
                                .then_some(intercept_proxy_domain::HttpAction::Delay {
                                    milliseconds: 1,
                                })
                                .into_iter()
                                .collect(),
                        )
                        .into_iter()
                        .chain([UnifiedAction::RecordMatch])
                        .collect(),
                        document: Some(HttpDocumentRuleContent { package }),
                    }),
                },
            })
            .await
            .unwrap();

        assert_eq!(saved.stage(), stage);
        assert!(runtime.statuses().await.unwrap().is_empty());
    }
}
