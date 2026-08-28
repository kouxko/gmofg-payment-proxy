use super::*;
use crate::{ChannelId, ErrorCode, MessageStage, Revision, RuntimeEpoch, TerminalIdentity};
use chrono::Utc;
use serde_json::{Value, json};

fn draft(
    stage: MessageStage,
    conditions: Vec<MatchCondition>,
    actions: Vec<RuleAction>,
) -> RuleDraft {
    RuleDraft {
        expected_revision: None,
        name: "test".into(),
        description: String::new(),
        enabled: true,
        priority: 10,
        created_order: 1,
        channel: None,
        stage,
        conditions,
        actions,
        one_shot: false,
    }
}

fn context<'a>(
    epoch: RuntimeEpoch,
    terminal: &'a TerminalIdentity,
    json: Option<&'a Value>,
) -> MatchContext<'a> {
    MatchContext {
        runtime_epoch: epoch,
        channel: ChannelId::new("alpha").unwrap(),
        stage: MessageStage::Request,
        terminal,
        path_or_request_type: Some("/payment"),
        json_body: json,
    }
}

// RULE-003, RULE-005, RULE-008, RULE-010, ENGINE-006
#[test]
fn evaluates_priority_then_creation_order_and_stops_at_terminal_action() {
    let epoch = RuntimeEpoch::new();
    let mut first = Rule::create(draft(
        MessageStage::Request,
        Vec::new(),
        vec![RuleAction::Delay { milliseconds: 20 }],
    ))
    .unwrap();
    first.priority = 1;
    let mut terminal = Rule::create(draft(
        MessageStage::Request,
        Vec::new(),
        vec![RuleAction::Terminal(
            TerminalAction::DisconnectBeforeUpstream,
        )],
    ))
    .unwrap();
    terminal.priority = 2;
    let mut unreachable = Rule::create(draft(
        MessageStage::Request,
        Vec::new(),
        vec![RuleAction::Delay { milliseconds: 30 }],
    ))
    .unwrap();
    unreachable.priority = 3;
    let terminal_identity = TerminalIdentity {
        source_ip: "10.0.0.1".into(),
        certificate_sha256: "cert".into(),
    };
    let mut engine = RuleEngine::new(epoch, vec![unreachable, terminal, first]);
    let result = engine.evaluate(&context(epoch, &terminal_identity, None), Utc::now());
    assert_eq!(result.composed_actions.len(), 2);
    assert!(result.terminal_action.is_some());
    assert_eq!(result.traces.len(), 2);
}

#[test]
fn equal_priority_and_creation_order_use_rule_id_as_a_stable_tiebreaker() {
    let epoch = RuntimeEpoch::new();
    let first = Rule::create(draft(
        MessageStage::Request,
        Vec::new(),
        vec![RuleAction::Terminal(
            TerminalAction::DisconnectBeforeUpstream,
        )],
    ))
    .expect("first rule");
    let second = Rule::create(draft(
        MessageStage::Request,
        Vec::new(),
        vec![RuleAction::Terminal(
            TerminalAction::DisconnectBeforeUpstream,
        )],
    ))
    .expect("second rule");
    let expected = first.id.min(second.id);
    let terminal = TerminalIdentity {
        source_ip: "10.0.0.1".into(),
        certificate_sha256: "cert".into(),
    };

    for rules in [vec![first.clone(), second.clone()], vec![second, first]] {
        let mut engine = RuleEngine::new(epoch, rules);
        let evaluation = engine.evaluate(&context(epoch, &terminal, None), Utc::now());
        assert_eq!(evaluation.traces[0].rule_id, expected);
    }
}

#[test]
fn failed_joint_gate_does_not_consume_nth_hit_or_one_shot_state() {
    let epoch = RuntimeEpoch::new();
    let mut candidate = Rule::create(draft(
        MessageStage::Request,
        vec![MatchCondition::NthHit(1)],
        vec![RuleAction::Pause],
    ))
    .expect("joint rule");
    candidate.one_shot = true;
    let rule_id = candidate.id;
    let terminal = TerminalIdentity {
        source_ip: "10.0.0.1".into(),
        certificate_sha256: "cert".into(),
    };
    let mut engine = RuleEngine::new(epoch, vec![candidate]);

    let first = engine
        .evaluate_with_gate(
            &context(epoch, &terminal, None),
            Utc::now(),
            |_| Ok::<_, ()>(false),
        )
        .expect("gate mismatch is not an execution error");
    assert!(!first.traces[0].matched);
    assert_eq!(engine.rules()[0].hit_count, 0);
    assert!(engine.rules()[0].enabled);

    let second = engine
        .evaluate_with_gate(
            &context(epoch, &terminal, None),
            Utc::now(),
            |_| Ok::<_, ()>(true),
        )
        .expect("second evaluation");
    assert_eq!(second.traces[0].rule_id, rule_id);
    assert!(second.traces[0].matched);
    assert_eq!(engine.rules()[0].hit_count, 1);
    assert!(!engine.rules()[0].enabled);
}

#[test]
fn failed_joint_gate_commits_no_http_actions_or_hit_metadata() {
    let epoch = RuntimeEpoch::new();
    let mut candidate = Rule::create(draft(
        MessageStage::Request,
        Vec::new(),
        vec![RuleAction::Pause],
    ))
    .expect("joint rule");
    candidate.one_shot = true;
    let terminal = TerminalIdentity {
        source_ip: "10.0.0.1".into(),
        certificate_sha256: "cert".into(),
    };
    let mut engine = RuleEngine::new(epoch, vec![candidate]);

    let error = engine
        .evaluate_with_gate(
            &context(epoch, &terminal, None),
            Utc::now(),
            |_| Err::<bool, _>("document action failed"),
        )
        .expect_err("document failure must abort the joint evaluation");

    assert_eq!(error, "document action failed");
    assert_eq!(engine.rules()[0].hit_count, 0);
    assert!(engine.rules()[0].last_hit_at.is_none());
    assert!(engine.rules()[0].enabled);
}

// RULE-004, ENGINE-003, ENGINE-004, TEST-RULE
#[test]
fn matches_json_path_equals_contains_and_regex_without_panicking() {
    let epoch = RuntimeEpoch::new();
    let rule = Rule::create(draft(
        MessageStage::Request,
        vec![
            MatchCondition::Field {
                field: MatchField::JsonPath("$.payment.items[0].name".into()),
                operator: MatchOperator::Equals("商品A".into()),
            },
            MatchCondition::Field {
                field: MatchField::PathOrRequestType,
                operator: MatchOperator::Contains("pay".into()),
            },
            MatchCondition::Field {
                field: MatchField::TerminalIp,
                operator: MatchOperator::Regex(r"^10\.0\.".into()),
            },
        ],
        vec![RuleAction::Pause],
    ))
    .unwrap();
    let terminal = TerminalIdentity {
        source_ip: "10.0.0.8".into(),
        certificate_sha256: "cert".into(),
    };
    let body = json!({"payment":{"items":[{"name":"商品A"}]}});
    let mut engine = RuleEngine::new(epoch, vec![rule]);
    assert!(
        engine
            .evaluate(&context(epoch, &terminal, Some(&body)), Utc::now())
            .traces[0]
            .matched
    );
    let no_json = engine.evaluate(&context(epoch, &terminal, None), Utc::now());
    assert!(no_json.traces[0].reason.contains("JSON"));
}

#[test]
fn invalid_persisted_json_path_is_a_non_match_instead_of_a_panic() {
    let epoch = RuntimeEpoch::new();
    let mut rule = Rule::create(draft(
        MessageStage::Request,
        vec![MatchCondition::Field {
            field: MatchField::JsonPath("$.valid".into()),
            operator: MatchOperator::Equals("value".into()),
        }],
        vec![RuleAction::Pause],
    ))
    .expect("initial valid rule");
    rule.conditions = vec![MatchCondition::Field {
        field: MatchField::JsonPath("$.items[]".into()),
        operator: MatchOperator::Equals("value".into()),
    }];
    let terminal = TerminalIdentity {
        source_ip: "10.0.0.8".into(),
        certificate_sha256: "cert".into(),
    };
    let body = json!({"items": ["value"]});
    let mut engine = RuleEngine::new(epoch, vec![rule]);

    let evaluation = engine.evaluate(&context(epoch, &terminal, Some(&body)), Utc::now());

    assert!(!evaluation.traces[0].matched);
    assert!(evaluation.traces[0].reason.contains("未通过保存校验"));
}

// RULE-006, RULE-007, ENGINE-007
