use intercept_proxy_domain::{
    DocumentAction, DocumentCondition, HttpDocumentRuleContent, HttpRuleContent, ListenerId,
    MatchCondition, ProtocolPackageId, ProtocolPackageRef, ProxyWorkspace, RuleAction, RuleContent,
    RuleDefinition, RuleDefinitionDraft, RuleStage, SocketRuleContent,
};

fn package() -> ProtocolPackageRef {
    ProtocolPackageRef {
        id: ProtocolPackageId::new("iso8583").expect("package id"),
        version: "1.0.0".parse().expect("version"),
    }
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
                conditions: Vec::<MatchCondition>::new(),
                actions: vec![RuleAction::Delay { milliseconds: 1 }],
                document: None,
                one_shot: false,
                hit_count: 0,
                last_hit_at: None,
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
fn application_boundary_http_stages_reject_ordinary_work_but_accept_pure_document() {
    for stage in [RuleStage::AppToProxy, RuleStage::UpstreamToProxy] {
        let draft = |actions| RuleDefinitionDraft {
            name: "HTTP stage contract".into(),
            enabled: true,
            priority: 5,
            listener_id: ListenerId::new(),
            stage,
            content: RuleContent::Http(HttpRuleContent {
                description: String::new(),
                conditions: Vec::new(),
                actions,
                document: Some(HttpDocumentRuleContent {
                    package: package(),
                    conditions: Vec::new(),
                    actions: vec![DocumentAction::RecordMatch],
                }),
                one_shot: false,
                hit_count: 0,
                last_hit_at: None,
            }),
        };
        let error = RuleDefinition::create(draft(vec![RuleAction::Delay { milliseconds: 1 }]), 1)
            .expect_err("ordinary HTTP work is unavailable at application boundary stages");
        assert_eq!(error.code.as_str(), "RULE_INVALID");
        RuleDefinition::create(draft(Vec::new()), 1).expect("pure Document remains valid");
    }
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
                content: RuleContent::Http(HttpRuleContent {
                    description: String::new(),
                    conditions: Vec::new(),
                    actions: vec![RuleAction::Delay { milliseconds: 1 }],
                    document: Some(HttpDocumentRuleContent {
                        package: package(),
                        conditions: Vec::new(),
                        actions: vec![DocumentAction::RecordMatch],
                    }),
                    one_shot: false,
                    hit_count: 0,
                    last_hit_at: None,
                }),
            },
            1,
        )
        .expect("proxy boundary stages expose joint HTTP capability");
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
            stage: RuleStage::AppToProxy,
            content: RuleContent::Socket(SocketRuleContent {
                package: package(),
                conditions: Vec::<DocumentCondition>::new(),
                actions: vec![DocumentAction::RecordMatch],
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
                stage: RuleStage::AppToProxy,
                content: RuleContent::Http(HttpRuleContent {
                    description: String::new(),
                    conditions: Vec::new(),
                    actions: Vec::new(),
                    document: None,
                    one_shot: false,
                    hit_count: 0,
                    last_hit_at: None,
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
