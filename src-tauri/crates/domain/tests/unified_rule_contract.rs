use intercept_proxy_domain::{
    Condition, DocumentMutation, DocumentPredicate, DropResponseMode, HttpAction, HttpRuleContent,
    JsonPointer, ListenerId, ProtocolPackageId, ProtocolPackageRef, ProxyWorkspace, RuleContent,
    RuleDefinition, RuleDefinitionDraft, RuleStage, SocketRuleContent, StringOperator,
    StringPredicate, TerminalAction, UnifiedAction,
};

fn package() -> ProtocolPackageRef {
    ProtocolPackageRef {
        id: ProtocolPackageId::new("iso8583").expect("package id"),
        version: "1.0.0".parse().expect("version"),
    }
}

fn http_condition() -> Condition {
    Condition::Http {
        field: intercept_proxy_domain::MatchField::Method,
        operator: intercept_proxy_domain::MatchOperator::Equals("GET".into()),
    }
}

fn document_condition() -> Condition {
    Condition::Document {
        path: JsonPointer::parse("/value").expect("pointer"),
        predicate: DocumentPredicate::String(StringPredicate {
            operator: StringOperator::Equal,
            value: "x".into(),
        }),
    }
}

fn document_action() -> UnifiedAction {
    UnifiedAction::Document(DocumentMutation::Set {
        path: JsonPointer::parse("/value").expect("pointer"),
        value: intercept_proxy_domain::DocumentValue::String("y".into()),
    })
}

#[test]
fn rule_save_requires_exactly_one_condition_and_one_action() {
    let json = serde_json::json!({
        "description": "",
        "conditions": [document_condition()],
        "actions": [{"source": "record_match"}]
    });
    assert!(serde_json::from_value::<HttpRuleContent>(json).is_err());

    let content = HttpRuleContent {
        description: String::new(),
        condition: document_condition(),
        action: UnifiedAction::RecordMatch,
    };
    let serialized = serde_json::to_value(content).expect("singular content wire");
    assert!(serialized.get("condition").is_some());
    assert!(serialized.get("action").is_some());
    assert!(serialized.get("conditions").is_none());
    assert!(serialized.get("actions").is_none());
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
            content: RuleContent::Http(HttpRuleContent {
                description: String::new(),
                condition: http_condition(),
                action: UnifiedAction::Http(HttpAction::Delay { milliseconds: 1 }),
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
        for (condition, action) in [
            (http_condition(), document_action()),
            (
                document_condition(),
                UnifiedAction::Http(HttpAction::Delay { milliseconds: 1 }),
            ),
        ] {
            RuleDefinition::create(
                RuleDefinitionDraft {
                    name: "Joint HTTP".into(),
                    enabled: true,
                    priority: 5,
                    listener_id: ListenerId::new(),
                    stage,
                    content: RuleContent::Http(HttpRuleContent {
                        description: String::new(),
                        condition,
                        action,
                    }),
                },
                1,
            )
            .expect("proxy boundary stages expose joint HTTP capability");
        }
    }
}

#[test]
fn http_document_conditions_and_actions_do_not_require_a_duplicate_package_binding() {
    let draft = |condition, action| RuleDefinitionDraft {
        name: "HTTP without Document".into(),
        enabled: true,
        priority: 5,
        listener_id: ListenerId::new(),
        stage: RuleStage::ProxyToUpstream,
        content: RuleContent::Http(HttpRuleContent {
            description: String::new(),
            condition,
            action,
        }),
    };

    RuleDefinition::create(
        draft(
            document_condition(),
            UnifiedAction::Http(HttpAction::Delay { milliseconds: 1 }),
        ),
        1,
    )
    .expect("Document conditions derive their body owner from the Listener");
    RuleDefinition::create(draft(http_condition(), document_action()), 1)
        .expect("Document actions derive their body owner from the Listener");

    let listener_id = ListenerId::new();
    let mut rule = RuleDefinition::create(
        RuleDefinitionDraft {
            listener_id,
            ..draft(
                http_condition(),
                UnifiedAction::Http(HttpAction::Delay { milliseconds: 1 }),
            )
        },
        1,
    )
    .expect("valid HTTP-only rule");
    rule.update(
        rule.revision(),
        RuleDefinitionDraft {
            listener_id,
            ..draft(document_condition(), document_action())
        },
    )
    .expect("HTTP rules may add schema-free Body Document work");
}

#[test]
fn socket_save_rejects_every_terminal_variant_until_socket_capabilities_define_one() {
    let terminals = vec![
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
                content: RuleContent::Socket(SocketRuleContent {
                    package: package(),
                    condition: document_condition(),
                    action: UnifiedAction::Terminal(terminal.clone()),
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
            content: RuleContent::Socket(SocketRuleContent {
                package: package(),
                condition: document_condition(),
                action: document_action(),
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
                content: RuleContent::Http(HttpRuleContent {
                    description: String::new(),
                    condition: http_condition(),
                    action: UnifiedAction::Http(HttpAction::Delay { milliseconds: 1 }),
                }),
            },
        )
        .expect_err("immutable binding must fail closed");
    assert_eq!(error.code.as_str(), "RULE_INVALID");
    assert_eq!(rule.listener_id(), listener_id);
    assert!(matches!(rule.content(), RuleContent::Socket(_)));
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
