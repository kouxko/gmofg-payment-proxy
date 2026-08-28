use super::*;
use intercept_proxy_domain::{HttpDocumentRuleContent, HttpRuleContent, SocketRuleContent};

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
            content: RuleContent::Socket(SocketRuleContent {
                package,
                schema_version: match stage.direction().unwrap() {
                    ProtocolDirection::Upstream => 1,
                    ProtocolDirection::Downstream => 2,
                },
                conditions: vec![equals("trace_id", DocumentValue::String("abc".into()))],
                actions: vec![set("amount", DocumentValue::Int(2))],
            }),
        },
    }
}

#[tokio::test]
async fn stopped_listener_rejects_invalid_unified_document_before_persistence() {
    for invalid_case in ["package", "schema", "field", "type", "stage"] {
        let (application, services, workspaces, runtime) = fixture();
        let package = pkg("unified-validation", "1.0.0");
        let listener_id = configure_relay(&services, &workspaces, &package).await;
        let selected = workspaces.list().await.unwrap().remove(0);
        let mut before = workspaces.get(selected.id).await.unwrap();
        assert!(runtime.statuses().await.unwrap().is_empty());

        let mut input =
            unified_socket_input(listener_id, package.clone(), RuleStage::ProxyToUpstream);
        let RuleContent::Socket(content) = &mut input.draft.content else {
            unreachable!()
        };
        match invalid_case {
            "package" => content.package = pkg("forged-package", "1.0.0"),
            "schema" => content.schema_version = 99,
            "field" => {
                content.conditions =
                    vec![equals("missing_field", DocumentValue::String("abc".into()))];
            }
            "type" => {
                content.actions = vec![set("amount", DocumentValue::String("wrong".into()))];
            }
            "stage" => {
                let ListenerDataPlane::Socket(settings) = &mut before.listeners[0].data_plane
                else {
                    unreachable!()
                };
                settings.topology =
                    SocketTopology::LocalResponder(SocketLocalResponderTopology::default());
                workspaces.save(before.clone()).await.unwrap();
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
async fn unified_document_save_enforces_direction_decode_and_encode_capabilities() {
    for (case, decode, encode, actions) in [
        ("decode", false, true, vec![DocumentAction::RecordMatch]),
        (
            "encode",
            true,
            false,
            vec![set("amount", DocumentValue::Int(2))],
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
        content.actions = actions;

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
                content: RuleContent::Http(HttpRuleContent {
                    description: "header + document".into(),
                    conditions: Vec::new(),
                    actions: vec![intercept_proxy_domain::RuleAction::Delay { milliseconds: 1 }],
                    document: Some(HttpDocumentRuleContent {
                        package: http_package,
                        schema_version: 1,
                        conditions: vec![equals("trace_id", DocumentValue::String("abc".into()))],
                        actions: vec![set("amount", DocumentValue::Int(2))],
                    }),
                    one_shot: false,
                    hit_count: 0,
                    last_hit_at: None,
                }),
            },
        })
        .await
        .unwrap();
    assert!(matches!(http.content(), RuleContent::Http(_)));
    assert!(runtime.statuses().await.unwrap().is_empty());
}

#[tokio::test]
async fn stopped_http_listener_rejects_ordinary_http_work_at_document_only_stages() {
    for (stage, conditions, actions) in [
        (
            RuleStage::AppToProxy,
            vec![intercept_proxy_domain::MatchCondition::NthHit(1)],
            Vec::new(),
        ),
        (
            RuleStage::AppToProxy,
            Vec::new(),
            vec![intercept_proxy_domain::RuleAction::Delay { milliseconds: 1 }],
        ),
        (
            RuleStage::UpstreamToProxy,
            vec![intercept_proxy_domain::MatchCondition::NthHit(1)],
            Vec::new(),
        ),
        (
            RuleStage::UpstreamToProxy,
            Vec::new(),
            vec![intercept_proxy_domain::RuleAction::Delay { milliseconds: 1 }],
        ),
    ] {
        let (application, services, workspaces, runtime) = fixture();
        let package = pkg("http-stage-gate", "1.0.0");
        let listener_id = configure_http(&services, &workspaces, &package).await;
        let selected = workspaces.list().await.unwrap().remove(0);
        let before = workspaces.get(selected.id).await.unwrap();

        let error = application
            .rule_definition_save(RuleDefinitionSaveInput {
                rule_id: None,
                expected_revision: None,
                draft: RuleDefinitionDraft {
                    name: "非法普通 HTTP 阶段".into(),
                    enabled: true,
                    priority: 10,
                    listener_id,
                    stage,
                    content: RuleContent::Http(HttpRuleContent {
                        description: String::new(),
                        conditions,
                        actions,
                        document: Some(HttpDocumentRuleContent {
                            package,
                            schema_version: match stage.direction().unwrap() {
                                ProtocolDirection::Upstream => 1,
                                ProtocolDirection::Downstream => 2,
                            },
                            conditions: Vec::new(),
                            actions: vec![DocumentAction::RecordMatch],
                        }),
                        one_shot: false,
                        hit_count: 0,
                        last_hit_at: None,
                    }),
                },
            })
            .await
            .unwrap_err();

        assert_eq!(error_code(&error), "RULE_INVALID");
        assert_eq!(workspaces.get(selected.id).await.unwrap(), before);
        assert!(runtime.statuses().await.unwrap().is_empty());
    }
}

#[tokio::test]
async fn stopped_http_listener_accepts_pure_document_and_exact_joint_stages() {
    for (stage, joint) in [
        (RuleStage::AppToProxy, false),
        (RuleStage::UpstreamToProxy, false),
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
                    content: RuleContent::Http(HttpRuleContent {
                        description: String::new(),
                        conditions: Vec::new(),
                        actions: joint
                            .then_some(intercept_proxy_domain::RuleAction::Delay {
                                milliseconds: 1,
                            })
                            .into_iter()
                            .collect(),
                        document: Some(HttpDocumentRuleContent {
                            package,
                            schema_version: match stage.direction().unwrap() {
                                ProtocolDirection::Upstream => 1,
                                ProtocolDirection::Downstream => 2,
                            },
                            conditions: Vec::new(),
                            actions: vec![DocumentAction::RecordMatch],
                        }),
                        one_shot: false,
                        hit_count: 0,
                        last_hit_at: None,
                    }),
                },
            })
            .await
            .unwrap();

        assert_eq!(saved.stage(), stage);
        assert!(runtime.statuses().await.unwrap().is_empty());
    }
}
