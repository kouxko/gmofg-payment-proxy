use super::*;
use intercept_proxy_domain::{HttpRuleContent, SocketRuleContent, UnifiedAction};

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
                condition: equals("trace_id", DocumentValue::String("abc".into())),
                action: set("amount", DocumentValue::integer(2).unwrap()),
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
                content.action = set("amount", DocumentValue::String("wrong".into()));
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
    content.condition = equals("missing_field", DocumentValue::String("abc".into()));
    content.action = set("extension_value", DocumentValue::Boolean(true));

    application
        .rule_definition_save(input)
        .await
        .expect("rule-owned paths need not appear in incomplete schema metadata");
    assert!(runtime.statuses().await.unwrap().is_empty());
}

#[tokio::test]
async fn unified_document_save_enforces_direction_decode_and_encode_capabilities() {
    for (case, decode, encode, action) in [
        ("decode", false, true, UnifiedAction::RecordMatch),
        (
            "encode",
            true,
            false,
            set("amount", DocumentValue::integer(2).unwrap()),
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
        content.action = action;

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
                    description: "document".into(),
                    condition: equals("trace_id", DocumentValue::String("abc".into())),
                    action: set(
                        "amount",
                        DocumentValue::integer(2).unwrap(),
                    ),
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
    for stage in [RuleStage::ProxyToUpstream, RuleStage::ProxyToApp] {
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
                        condition: equals("trace_id", DocumentValue::String("abc".into())),
                        action: set(
                            "amount",
                            DocumentValue::integer(2).unwrap(),
                        ),
                    }),
                },
            })
            .await
            .unwrap();

        assert_eq!(saved.stage(), stage);
        assert!(runtime.statuses().await.unwrap().is_empty());
    }
}
