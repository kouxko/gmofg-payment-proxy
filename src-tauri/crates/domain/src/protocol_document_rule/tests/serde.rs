use super::*;

#[test]
fn strict_round_trip_rejects_unknown_and_invalid_counter_fields() {
    let candidate = rule(
        1,
        ListenerId::new(),
        ProtocolDirection::Upstream,
        vec![condition("amount", DocumentValue::Int(1000))],
        vec![DocumentAction::RecordMatch],
    )
    .unwrap();
    let mut json = serde_json::to_value(&candidate).unwrap();
    assert_eq!(
        serde_json::from_value::<ProtocolDocumentRuleDefinition>(json.clone()).unwrap(),
        candidate
    );
    json["method"] = serde_json::json!("POST");
    assert!(serde_json::from_value::<ProtocolDocumentRuleDefinition>(json).is_err());

    let mut action = serde_json::to_value(&candidate).unwrap();
    action["actions"][0]["status"] = serde_json::json!(200);
    assert!(serde_json::from_value::<ProtocolDocumentRuleDefinition>(action).is_err());

    let valued = rule(
        2,
        ListenerId::new(),
        ProtocolDirection::Upstream,
        Vec::new(),
        vec![set("amount", DocumentValue::Int(1))],
    )
    .unwrap();
    let mut nested_value = serde_json::to_value(valued).unwrap();
    nested_value["actions"][0]["value"]["http_only"] = serde_json::json!(true);
    assert!(serde_json::from_value::<ProtocolDocumentRuleDefinition>(nested_value).is_err());

    for field in ["revision", "created_order"] {
        let mut invalid = serde_json::to_value(&candidate).unwrap();
        invalid[field] = serde_json::json!(0);
        assert!(serde_json::from_value::<ProtocolDocumentRuleDefinition>(invalid).is_err());

        let mut boundary = serde_json::to_value(&candidate).unwrap();
        boundary[field] = serde_json::json!(9_007_199_254_740_991_u64);
        assert!(serde_json::from_value::<ProtocolDocumentRuleDefinition>(boundary).is_ok());

        let mut overflow = serde_json::to_value(&candidate).unwrap();
        overflow[field] = serde_json::json!(9_007_199_254_740_992_u64);
        assert!(serde_json::from_value::<ProtocolDocumentRuleDefinition>(overflow).is_err());
    }

    assert!(
        ProtocolDocumentRuleDefinition::new(
            ProtocolDocumentRuleId::new(),
            true,
            0,
            0,
            ListenerId::new(),
            package("1.2.3"),
            7,
            ProtocolDirection::Upstream,
            Vec::new(),
            vec![DocumentAction::RecordMatch],
        )
        .is_err()
    );
}

#[test]
fn rule_serde_rejects_unsafe_integer_values_in_conditions_and_actions() {
    for (section, mut value) in [
        (
            "conditions",
            serde_json::to_value(
                rule(
                    3,
                    ListenerId::new(),
                    ProtocolDirection::Upstream,
                    vec![condition("amount", DocumentValue::Int(1))],
                    vec![DocumentAction::RecordMatch],
                )
                .unwrap(),
            )
            .unwrap(),
        ),
        (
            "actions",
            serde_json::to_value(
                rule(
                    4,
                    ListenerId::new(),
                    ProtocolDirection::Upstream,
                    Vec::new(),
                    vec![set("amount", DocumentValue::Int(1))],
                )
                .unwrap(),
            )
            .unwrap(),
        ),
    ] {
        for number in [-9_007_199_254_740_992_i64, 9_007_199_254_740_992_i64] {
            value[section][0]["value"]["value"] = serde_json::json!(number);
            assert!(
                serde_json::from_value::<ProtocolDocumentRuleDefinition>(value.clone()).is_err()
            );
        }
    }
}
