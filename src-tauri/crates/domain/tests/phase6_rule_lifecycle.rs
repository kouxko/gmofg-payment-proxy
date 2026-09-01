use chrono::{TimeZone, Utc};
use intercept_proxy_domain::{
    Condition, Document, DocumentValue, ErrorCode, HttpRuleContent, ListenerId, NthCounterAdvance,
    Revision, RuleContent, RuleDefinition, RuleDefinitionDraft, RuleDefinitionRestoreSnapshot,
    RuleLifecycle, RuleLifecycleDelta, RuleStage, TerminalIdentity, validate_conditions,
};

#[test]
fn common_nth_hit_rejects_zero_at_the_domain_owner() {
    let error = validate_conditions(&[Condition::NthHit { count: 0 }]).expect_err("zero nth-hit");
    assert_eq!(error.code, ErrorCode::RuleInvalid);
}

#[test]
fn nth_only_delta_is_valid_for_the_shared_commit_contract_and_does_not_mutate_definition() {
    let baseline = definition(false, 2);
    let delta = intercept_proxy_domain::RuleLifecycleDelta {
        rule_id: baseline.rule_id(),
        expected_revision: baseline.revision(),
        hit_count_increment: 0,
        last_hit_at: None,
        disable_one_shot: false,
        nth_counter_advance: Some(NthCounterAdvance {
            rule_id: baseline.rule_id(),
            terminal: TerminalIdentity {
                source_ip: "10.0.0.1".into(),
                certificate_sha256: "cert".into(),
            },
            expected_attempts: 0,
            increment: 1,
        }),
    };

    assert_eq!(baseline.apply_lifecycle_delta(&delta).unwrap(), baseline);
}

fn definition(one_shot: bool, hit_count: u64) -> RuleDefinition {
    RuleDefinition::create(
        RuleDefinitionDraft {
            name: "lifecycle".into(),
            enabled: true,
            priority: 1,
            listener_id: ListenerId::new(),
            stage: RuleStage::ProxyToUpstream,
            one_shot,
            content: RuleContent::Http(HttpRuleContent {
                description: String::new(),
                conditions: vec![Condition::NthHit {
                    count: hit_count + 1,
                }],
                actions: vec![intercept_proxy_domain::UnifiedAction::RecordMatch],
            }),
        },
        1,
    )
    .and_then(|created| {
        RuleDefinition::restore(
            created.rule_id(),
            created.to_draft(),
            RuleDefinitionRestoreSnapshot {
                revision: Revision::INITIAL,
                created_order: 1,
                lifecycle: RuleLifecycle {
                    hit_count,
                    last_hit_at: None,
                },
            },
        )
    })
    .expect("definition")
}

#[test]
fn save_draft_cannot_supply_runtime_statistics_and_create_initializes_them() {
    let draft = definition(false, 0).to_draft();
    let value = serde_json::to_value(&draft).expect("draft wire");
    assert_eq!(value["one_shot"], false);
    assert!(value.get("lifecycle").is_none());
    assert!(value.get("hit_count").is_none());
    assert!(value.get("last_hit_at").is_none());

    let created = RuleDefinition::create(draft, 9).expect("create");
    assert_eq!(created.lifecycle().hit_count, 0);
    assert_eq!(created.lifecycle().last_hit_at, None);
}

#[test]
fn update_preserves_runtime_statistics_and_copy_resets_them() {
    let mut definition = definition(true, 7);
    let mut draft = definition.to_draft();
    draft.name = "updated".into();
    definition
        .update(Revision::INITIAL, draft)
        .expect("configuration update");
    assert_eq!(definition.lifecycle().hit_count, 7);

    definition.remap_for_workspace_copy(ListenerId::new());
    assert_eq!(definition.revision(), Revision::INITIAL);
    assert_eq!(definition.lifecycle(), &RuleLifecycle::default());
    assert!(definition.to_draft().one_shot);
}

#[test]
fn draft_rejects_forged_runtime_statistics() {
    let mut value = serde_json::to_value(definition(false, 0).to_draft()).expect("draft");
    value["hit_count"] = serde_json::json!(99);
    assert!(serde_json::from_value::<RuleDefinitionDraft>(value).is_err());
}

#[test]
fn lifecycle_delta_is_tentative_until_explicitly_applied() {
    let baseline = definition(false, 2);
    let at = Utc.with_ymd_and_hms(2026, 8, 30, 12, 0, 0).unwrap();

    let delta = baseline.lifecycle_delta_for_successful_match(at);

    assert_eq!(baseline.lifecycle().hit_count, 2);
    assert_eq!(baseline.revision().get(), 1);
    let committed = baseline
        .apply_lifecycle_delta(&delta)
        .expect("matching baseline accepts delta");
    assert_eq!(committed.lifecycle().hit_count, 3);
    assert_eq!(committed.lifecycle().last_hit_at, Some(at));
    assert!(committed.enabled());
    assert_eq!(committed.revision().get(), 1);
}

#[test]
fn one_shot_disables_and_advances_revision_only_when_delta_is_applied() {
    let baseline = definition(true, 0);
    let at = Utc.with_ymd_and_hms(2026, 8, 30, 12, 1, 0).unwrap();
    let delta = baseline.lifecycle_delta_for_successful_match(at);

    assert!(baseline.enabled());
    assert_eq!(baseline.revision().get(), 1);
    let committed = baseline.apply_lifecycle_delta(&delta).expect("commit");
    assert!(!committed.enabled());
    assert_eq!(committed.revision().get(), 2);
    assert_eq!(committed.lifecycle().hit_count, 1);
}

#[test]
fn nth_only_delta_cannot_disable_one_shot_without_a_successful_hit() {
    let baseline = definition(true, 0);
    let delta = RuleLifecycleDelta {
        rule_id: baseline.rule_id(),
        expected_revision: baseline.revision(),
        hit_count_increment: 0,
        last_hit_at: None,
        disable_one_shot: true,
        nth_counter_advance: Some(NthCounterAdvance {
            rule_id: baseline.rule_id(),
            terminal: TerminalIdentity {
                source_ip: "10.0.0.2".into(),
                certificate_sha256: "AA:BB".into(),
            },
            expected_attempts: 0,
            increment: 1,
        }),
    };

    assert!(baseline.apply_lifecycle_delta(&delta).is_err());
    assert!(baseline.enabled());
    assert_eq!(baseline.revision(), Revision::INITIAL);
    assert_eq!(baseline.lifecycle(), &RuleLifecycle::default());
}

#[test]
fn nth_hit_is_a_common_leaf_and_a_miss_does_not_consume_lifecycle() {
    let baseline = definition(false, 1);
    let document = Document::new(DocumentValue::Null(()));

    let matched = match baseline.content() {
        RuleContent::Http(content) => {
            intercept_proxy_domain::evaluate_conditions_with_nth(
                &content.conditions,
                &document,
                2,
                &mut |_, _| Ok::<_, intercept_proxy_domain::DomainError>(false),
            )
            .expect("match")
            .matched
        }
        RuleContent::Socket(_) => unreachable!(),
    };

    assert!(matched);
    assert_eq!(baseline.lifecycle().hit_count, 1);
}
