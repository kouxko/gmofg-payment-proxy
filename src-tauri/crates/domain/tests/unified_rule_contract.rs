use intercept_proxy_domain::{
    Condition, ConditionTree, DocumentMutation, DocumentPredicate, DropResponseMode, HttpAction,
    HttpDocumentRuleContent, HttpRuleContent, JsonPointer, ListenerId, ProtocolPackageId,
    ProtocolPackageRef, ProxyWorkspace, RuleContent, RuleDefinition, RuleDefinitionDraft,
    RuleStage, SocketRuleContent, StringOperator, StringPredicate, TerminalAction, UnifiedAction,
};

fn package() -> ProtocolPackageRef {
    ProtocolPackageRef {
        id: ProtocolPackageId::new("iso8583").expect("package id"),
        version: "1.0.0".parse().expect("version"),
    }
}

fn http_condition() -> ConditionTree {
    ConditionTree::Leaf(Condition::NthHit { count: 1 })
}

fn document_condition() -> ConditionTree {
    ConditionTree::Leaf(Condition::Document {
        path: JsonPointer::parse("/value").expect("pointer"),
        predicate: DocumentPredicate::String(StringPredicate {
            operator: StringOperator::Equal,
            value: "x".into(),
        }),
    })
}

fn document_action() -> UnifiedAction {
    UnifiedAction::Document(DocumentMutation::Set {
        path: JsonPointer::parse("/value").expect("pointer"),
        value: intercept_proxy_domain::DocumentValue::String("y".into()),
    })
}

#[test]
fn unified_rule_serializes_one_tagged_content_variant() {
    let rule = RuleDefinition::create(
        RuleDefinitionDraft {
            name: "HTTP 联合规则".into(),
            enabled: true,
            priority: 5,
            listener_id: ListenerId::new(),
            stage: RuleStage::ProxyToUpstream,
            one_shot: false,
            content: RuleContent::Http(HttpRuleContent {
                description: String::new(),
                condition: http_condition(),
                actions: vec![UnifiedAction::Http(HttpAction::Delay { milliseconds: 1 })],
                document: None,
            }),
        },
        7,
    )
    .expect("valid rule");

    let json = serde_json::to_value(&rule).expect("serialize");
    assert_eq!(json["stage"], "proxy_to_upstream");
    assert_eq!(json["content"]["type"], "http");
    assert!(json.get("protocol_rule").is_none());
}

#[test]
fn proxy_http_stages_accept_joint_http_and_document_work() {
    for stage in [RuleStage::ProxyToUpstream, RuleStage::ProxyToApp] {
        RuleDefinition::create(
            RuleDefinitionDraft {
                name: "Joint HTTP".into(),
                enabled: true,
                priority: 5,
                listener_id: ListenerId::new(),
                stage,
                one_shot: false,
                content: RuleContent::Http(HttpRuleContent {
                    description: String::new(),
                    condition: ConditionTree::All(vec![http_condition(), document_condition()]),
                    actions: vec![
                        UnifiedAction::Http(HttpAction::Delay { milliseconds: 1 }),
                        document_action(),
                    ],
                    document: Some(HttpDocumentRuleContent { package: package() }),
                }),
            },
            1,
        )
        .expect("proxy boundary stages expose joint HTTP capability");
    }
}

#[test]
fn http_without_document_binding_rejects_recursive_document_conditions_and_actions_on_save() {
    let nested_document_condition = ConditionTree::All(vec![ConditionTree::Any(vec![
        http_condition(),
        document_condition(),
    ])]);
    let draft = |condition, actions| RuleDefinitionDraft {
        name: "HTTP without Document".into(),
        enabled: true,
        priority: 5,
        listener_id: ListenerId::new(),
        stage: RuleStage::ProxyToUpstream,
        one_shot: false,
        content: RuleContent::Http(HttpRuleContent {
            description: String::new(),
            condition,
            actions,
            document: None,
        }),
    };

    assert!(
        RuleDefinition::create(
            draft(
                nested_document_condition,
                vec![UnifiedAction::Http(HttpAction::Delay { milliseconds: 1 })],
            ),
            1,
        )
        .is_err()
    );
    assert!(RuleDefinition::create(draft(http_condition(), vec![document_action()]), 1).is_err());

    let listener_id = ListenerId::new();
    let mut rule = RuleDefinition::create(
        RuleDefinitionDraft {
            listener_id,
            ..draft(
                http_condition(),
                vec![UnifiedAction::Http(HttpAction::Delay { milliseconds: 1 })],
            )
        },
        1,
    )
    .expect("valid HTTP-only rule");
    assert!(
        rule.update(
            rule.revision(),
            RuleDefinitionDraft {
                listener_id,
                ..draft(document_condition(), vec![document_action()])
            },
        )
        .is_err()
    );
}

#[test]
fn socket_save_rejects_every_terminal_variant_until_socket_capabilities_define_one() {
    let terminals = vec![
        TerminalAction::RejectTlsHandshake,
        TerminalAction::DisconnectBeforeUpstream,
        TerminalAction::UpstreamConnectTimeout { milliseconds: 1 },
        TerminalAction::UpstreamWriteTimeout { milliseconds: 1 },
        TerminalAction::UpstreamReadTimeout { milliseconds: 1 },
        TerminalAction::DropUpstreamResponse {
            mode: DropResponseMode::ReadCompleteResponse,
        },
        TerminalAction::MockResponse {
            status: 200,
            headers: Vec::new(),
            body_bytes: Vec::new(),
        },
        TerminalAction::InvalidJson {
            body_bytes: b"{".to_vec(),
        },
        TerminalAction::IncorrectContentLength { delta: 1 },
        TerminalAction::TruncateResponse { bytes: 0 },
        TerminalAction::DisconnectDuringUpstreamWrite { after_bytes: 0 },
        TerminalAction::DisconnectDuringDownstreamWrite { after_bytes: 0 },
    ];

    for terminal in terminals {
        let result = RuleDefinition::create(
            RuleDefinitionDraft {
                name: "Socket terminal".into(),
                enabled: true,
                priority: 0,
                listener_id: ListenerId::new(),
                stage: RuleStage::ProxyToUpstream,
                one_shot: false,
                content: RuleContent::Socket(SocketRuleContent {
                    package: package(),
                    condition: document_condition(),
                    actions: vec![UnifiedAction::Terminal(terminal.clone())],
                }),
            },
            1,
        );
        assert!(
            result.is_err(),
            "Socket accepted terminal variant {terminal:?}"
        );
    }
}

#[test]
fn listener_and_content_kind_are_immutable_after_creation() {
    let listener_id = ListenerId::new();
    let mut rule = RuleDefinition::create(
        RuleDefinitionDraft {
            name: "Socket".into(),
            enabled: true,
            priority: 0,
            listener_id,
            stage: RuleStage::ProxyToUpstream,
            one_shot: false,
            content: RuleContent::Socket(SocketRuleContent {
                package: package(),
                condition: document_condition(),
                actions: vec![document_action()],
            }),
        },
        1,
    )
    .expect("valid rule");

    let error = rule
        .update(
            rule.revision(),
            RuleDefinitionDraft {
                name: "changed".into(),
                enabled: true,
                priority: 0,
                listener_id: ListenerId::new(),
                stage: RuleStage::ProxyToUpstream,
                one_shot: false,
                content: RuleContent::Http(HttpRuleContent {
                    description: String::new(),
                    condition: http_condition(),
                    actions: vec![UnifiedAction::Http(HttpAction::Delay { milliseconds: 1 })],
                    document: None,
                }),
            },
        )
        .expect_err("immutable binding must fail closed");
    assert_eq!(error.code.as_str(), "RULE_INVALID");
    assert_eq!(rule.listener_id(), listener_id);
    assert!(matches!(rule.content(), RuleContent::Socket(_)));
}

#[test]
fn socket_content_rejects_tls_stage_and_unknown_fields() {
    let json = serde_json::json!({
        "name": "Socket",
        "enabled": true,
        "priority": 0,
        "listener_id": ListenerId::new(),
        "stage": "tls_handshake",
        "content": {
            "type": "socket",
            "value": {
                "package": package(),
                "schema_version": 1,
                "conditions": [],
                "actions": [{"type": "record_match"}],
                "http_status": 500
            }
        }
    });
    assert!(serde_json::from_value::<RuleDefinitionDraft>(json).is_err());
}

#[test]
fn workspace_persists_exactly_one_unified_rule_collection() {
    let workspace = ProxyWorkspace::default();
    let json = serde_json::to_value(workspace).expect("serialize workspace");
    assert!(json["rule_definitions"].is_array());
    assert_eq!(json["rule_created_order_high_water"], 0);
    assert!(json.get("rules").is_none());
    assert!(json.get("protocol_rules").is_none());
    assert!(json.get("protocol_rule_created_order_high_water").is_none());
}

#[test]
fn workspace_rejects_legacy_split_rule_collections() {
    let mut json = serde_json::to_value(ProxyWorkspace::default()).expect("serialize workspace");
    json.as_object_mut()
        .expect("workspace object")
        .insert("protocol_rules".into(), serde_json::json!([]));
    assert!(serde_json::from_value::<ProxyWorkspace>(json).is_err());
}
