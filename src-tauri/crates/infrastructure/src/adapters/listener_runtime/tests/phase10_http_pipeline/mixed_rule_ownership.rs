use super::production_shape::{
    Phase10ActorRules, actor_pipeline, execute_nth_http_attempt, prepared_external_snapshot_for,
};
use super::*;

#[tokio::test]
async fn production_http_joint_leaves_ordinary_false_rule_to_actor_matching() {
    use intercept_proxy_domain::{Condition, ConditionTree, MatchField, MatchOperator};

    let listener = phase10_listener();
    let unified = set_string_rule(
        &listener,
        ProtocolDocumentRuleId::from_uuid(Uuid::from_u128(996)),
        ProtocolRuleStage::ProxyToUpstream,
        1,
        "value",
        Vec::new(),
        "new",
    );
    let mut workspace = workspace_with_http_rules(&listener, vec![unified]);
    workspace.rule_definitions.extend([
        ordinary_header_rule(
            &listener,
            "ordinary false",
            20,
            2,
            ConditionTree::Leaf(Condition::Http {
                field: MatchField::TerminalIp,
                operator: MatchOperator::Equals("192.0.2.1".into()),
            }),
            "x-ordinary-false",
        ),
        ordinary_header_rule(
            &listener,
            "ordinary true",
            30,
            3,
            ConditionTree::Leaf(Condition::Http {
                field: MatchField::TerminalIp,
                operator: MatchOperator::Equals("127.0.0.1".into()),
            }),
            "x-ordinary-true",
        ),
        ordinary_header_rule(
            &listener,
            "ordinary nth",
            40,
            4,
            ConditionTree::Leaf(Condition::NthHit { count: 2 }),
            "x-ordinary-nth",
        ),
    ]);
    workspace.rule_created_order_high_water = 4;
    let snapshot = prepared_external_snapshot_for(
        Arc::new(RecordingHttpRpc::new(false)),
        &workspace,
        &listener,
    )
    .await;
    let repository = Arc::new(Phase10ActorRules {
        snapshot: Mutex::new(RuleRuntimeSnapshot::with_collection_identity_and_order(
            Some(Uuid::from_u128(997)),
            7,
            workspace.http_runtime_rules().unwrap(),
            workspace.http_runtime_rule_execution_order(),
        )),
        commit_attempts: AtomicUsize::new(0),
    });
    let pipeline = actor_pipeline(&snapshot, Arc::clone(&repository));
    pipeline.runtime_started(Uuid::from_u128(994)).await;

    let first = execute_nth_http_attempt(&snapshot, &pipeline, 10).await;
    let second = execute_nth_http_attempt(&snapshot, &pipeline, 11).await;

    assert!(!has_header(&first, b"x-ordinary-false"));
    assert!(has_header(&first, b"x-ordinary-true"));
    assert!(!has_header(&first, b"x-ordinary-nth"));
    assert!(has_header(&second, b"x-ordinary-true"));
    assert!(has_header(&second, b"x-ordinary-nth"));
    let committed = repository.snapshot.lock().clone();
    let hit_count = |name| {
        committed
            .rules
            .iter()
            .find(|rule| rule.name == name)
            .unwrap()
            .hit_count
    };
    assert_eq!(hit_count("ordinary false"), 0);
    assert_eq!(hit_count("ordinary true"), 2);
    assert_eq!(hit_count("ordinary nth"), 1);
}

fn ordinary_header_rule(
    listener: &ProxyListener,
    name: &str,
    priority: i32,
    created_order: u64,
    condition: intercept_proxy_domain::ConditionTree,
    header: &str,
) -> RuleDefinition {
    use intercept_proxy_domain::{HttpAction, RuleStage, UnifiedAction};

    RuleDefinition::create(
        RuleDefinitionDraft {
            name: name.into(),
            enabled: true,
            priority,
            listener_id: listener.id,
            stage: RuleStage::ProxyToUpstream,
            one_shot: false,
            content: RuleContent::Http(HttpRuleContent {
                description: String::new(),
                condition,
                actions: vec![UnifiedAction::Http(HttpAction::SetHeader {
                    name: header.into(),
                    value: "executed".into(),
                })],
                document: None,
            }),
        },
        created_order,
    )
    .unwrap()
}

fn has_header(message: &Message, name: &[u8]) -> bool {
    message
        .headers
        .iter()
        .any(|header| header.name.eq_ignore_ascii_case(name))
}
