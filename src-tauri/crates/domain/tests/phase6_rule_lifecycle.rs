use chrono::{TimeZone, Utc};
use intercept_proxy_domain::{
    Condition, HttpRuleContent, ListenerId, MatchField, MatchOperator, Revision, RuleContent,
    RuleDefinition, RuleDefinitionDraft, RuleDefinitionRestoreSnapshot, RuleLifecycle, RuleStage,
};

fn definition(hit_count: u64) -> RuleDefinition {
    RuleDefinition::create(
        RuleDefinitionDraft {
            name: "lifecycle".into(),
            enabled: true,
            priority: 1,
            listener_id: ListenerId::new(),
            stage: RuleStage::ProxyToUpstream,
            content: RuleContent::Http(HttpRuleContent {
                description: String::new(),
                condition: Condition::Http {
                    field: MatchField::Method,
                    operator: MatchOperator::Equals("GET".into()),
                },
                action: intercept_proxy_domain::UnifiedAction::Http(
                    intercept_proxy_domain::HttpAction::Delay { milliseconds: 1 },
                ),
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
    let draft = definition(0).to_draft();
    let value = serde_json::to_value(&draft).expect("draft wire");
    assert!(value.get("lifecycle").is_none());
    assert!(value.get("hit_count").is_none());
    assert!(value.get("last_hit_at").is_none());

    let created = RuleDefinition::create(draft, 9).expect("create");
    assert_eq!(created.lifecycle().hit_count, 0);
    assert_eq!(created.lifecycle().last_hit_at, None);
}

#[test]
fn draft_rejects_removed_configuration_fields() {
    let mut value = serde_json::to_value(definition(0).to_draft()).expect("draft");
    value[concat!("one", "_shot")] = serde_json::json!(true);
    assert!(serde_json::from_value::<RuleDefinitionDraft>(value).is_err());
}

#[test]
fn successful_match_never_disables_the_rule() {
    let baseline = definition(0);
    let at = Utc.with_ymd_and_hms(2026, 8, 30, 12, 2, 0).unwrap();
    let committed = baseline
        .apply_lifecycle_delta(&baseline.lifecycle_delta_for_successful_match(at))
        .expect("commit");

    assert!(committed.enabled());
    assert_eq!(committed.revision(), Revision::INITIAL);
}

#[test]
fn update_preserves_runtime_statistics_and_copy_resets_them() {
    let mut definition = definition(7);
    let mut draft = definition.to_draft();
    draft.name = "updated".into();
    definition
        .update(Revision::INITIAL, draft)
        .expect("configuration update");
    assert_eq!(definition.lifecycle().hit_count, 7);

    definition.remap_for_workspace_copy(ListenerId::new());
    assert_eq!(definition.revision(), Revision::INITIAL);
    assert_eq!(definition.lifecycle(), &RuleLifecycle::default());
}

#[test]
fn draft_rejects_forged_runtime_statistics() {
    let mut value = serde_json::to_value(definition(0).to_draft()).expect("draft");
    value["hit_count"] = serde_json::json!(99);
    assert!(serde_json::from_value::<RuleDefinitionDraft>(value).is_err());
}

#[test]
fn lifecycle_delta_is_tentative_until_explicitly_applied() {
    let baseline = definition(2);
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
