use super::*;

#[test]
fn typed_ipc_conditions_and_actions_round_trip_without_changing_domain_values() {
    let conditions = vec![
        MatchCondition::Field {
            field: MatchField::JsonPath("$.amount".into()),
            operator: MatchOperator::Regex(r"^\d+$".into()),
        },
        MatchCondition::NthHit(3),
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
