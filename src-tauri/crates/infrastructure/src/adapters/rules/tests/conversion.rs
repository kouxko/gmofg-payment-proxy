use super::*;
use intercept_proxy_domain::Condition;

#[test]
fn typed_ipc_conditions_and_actions_round_trip_without_changing_domain_values() {
    let conditions = vec![
        Condition::Http {
            condition: MatchCondition::Field {
                field: MatchField::JsonPath("$.amount".into()),
                operator: MatchOperator::Regex(r"^\d+$".into()),
            },
        },
        Condition::NthHit { count: 3 },
    ];
    let actions = vec![
        RuleAction::SetJsonField {
            path: "$.approved".into(),
            value: serde_json::json!({"ok": true, "code": 0}),
        },
        RuleAction::ReplaceBodyText("本文".into()),
        RuleAction::SetHeader {
            name: "x-test".into(),
            value: "yes".into(),
        },
        RuleAction::Delay { milliseconds: 25 },
        RuleAction::Pause,
        RuleAction::CustomHttpStatus { status: 503 },
        RuleAction::Terminal(TerminalAction::MockResponse {
            status: 200,
            headers: vec![("content-type".into(), "application/json".into())],
            body_bytes: vec![0x82, 0xa0],
        }),
    ];

    assert_eq!(
        conditions,
        conditions
            .iter()
            .map(condition_to_app)
            .map(|condition| condition_to_domain(&condition))
            .collect::<Vec<_>>()
    );
    assert_eq!(
        actions,
        actions
            .iter()
            .map(action_to_app)
            .collect::<Result<Vec<_>, _>>()
            .expect("app actions")
            .iter()
            .map(action_to_domain)
            .collect::<Result<Vec<_>, _>>()
            .expect("domain actions")
    );
}

fn delta_fixture() -> (RuleRuntimeSnapshot, RuleLifecycleDelta) {
    let rule =
        Rule::create(to_domain_draft(&request_delay_draft("delta", false), 1).expect("draft"))
            .expect("rule");
    let snapshot = RuleRuntimeSnapshot::new(vec![rule.clone()]);
    (
        snapshot,
        RuleLifecycleDelta {
            rule_id: rule.id,
            expected_revision: rule.revision,
            hit_count_increment: 1,
            last_hit_at: Some(Utc::now()),
            disable_one_shot: false,
            nth_counter_advance: None,
        },
    )
}

#[test]
fn runtime_delta_rejects_decrease_instead_of_saturating_it() {
    let (snapshot, _) = delta_fixture();
    let mut evaluated = snapshot.rules.clone();
    evaluated[0].hit_count = 1;
    let advanced = RuleRuntimeSnapshot::new(evaluated.clone());
    evaluated[0].hit_count = 0;
    assert_eq!(
        super::super::conversion::runtime_deltas(&advanced, &evaluated, &[])
            .expect_err("decrease")
            .view_model
            .code,
        "RULE_INVALID"
    );
}

#[test]
fn repository_conversion_rejects_zero_duplicate_oversized_and_wrong_id_deltas() {
    let (snapshot, valid) = delta_fixture();
    let mut zero = valid.clone();
    zero.hit_count_increment = 0;
    zero.last_hit_at = None;
    assert!(super::super::conversion::apply_runtime_deltas(&snapshot, &[zero]).is_err());
    assert!(
        super::super::conversion::apply_runtime_deltas(&snapshot, &[valid.clone(), valid.clone()])
            .is_err()
    );
    let mut oversized = valid.clone();
    oversized.hit_count_increment = 2;
    assert!(super::super::conversion::apply_runtime_deltas(&snapshot, &[oversized]).is_err());
    let mut wrong = valid;
    wrong.rule_id = RuleId::new();
    assert!(super::super::conversion::apply_runtime_deltas(&snapshot, &[wrong]).is_err());
}

#[test]
fn repository_conversion_rejects_nth_only_one_shot_disable_without_partial_write() {
    let mut draft = request_delay_draft("one-shot", true);
    draft.one_shot = true;
    let rule = Rule::create(to_domain_draft(&draft, 1).expect("draft")).expect("rule");
    let snapshot = RuleRuntimeSnapshot::new(vec![rule.clone()]);
    let crafted = RuleLifecycleDelta {
        rule_id: rule.id,
        expected_revision: rule.revision,
        hit_count_increment: 0,
        last_hit_at: None,
        disable_one_shot: true,
        nth_counter_advance: Some(intercept_proxy_domain::NthCounterAdvance {
            rule_id: rule.id,
            terminal: intercept_proxy_domain::TerminalIdentity {
                source_ip: "10.0.0.2".into(),
                certificate_sha256: "AA:BB".into(),
            },
            expected_attempts: 0,
            increment: 1,
        }),
    };

    let error = super::super::conversion::apply_runtime_deltas(&snapshot, &[crafted])
        .expect_err("Nth-only delta cannot disable one-shot");
    assert_eq!(error.view_model.code, "RULE_INVALID");
    assert!(snapshot.rules[0].enabled);
    assert_eq!(snapshot.rules[0].revision, rule.revision);
    assert_eq!(snapshot.rules[0].hit_count, 0);
}
