#[test]
fn nth_hit_is_per_terminal_and_resets_on_restart_reenable_and_condition_change() {
    let epoch = RuntimeEpoch::new();
    let mut rule = Rule::create(draft(
        MessageStage::Request,
        vec![MatchCondition::NthHit(2)],
        vec![RuleAction::Pause],
    ))
    .unwrap();
    rule.one_shot = true;
    let id = rule.id;
    let terminal = TerminalIdentity {
        source_ip: "10.0.0.8".into(),
        certificate_sha256: "cert".into(),
    };
    let other_terminal = TerminalIdentity {
        source_ip: "10.0.0.9".into(),
        certificate_sha256: "other-cert".into(),
    };
    let mut engine = RuleEngine::new(epoch, vec![rule]);
    assert!(
        !engine
            .evaluate(&context(epoch, &terminal, None), Utc::now())
            .traces[0]
            .matched
    );
    assert!(
        !engine
            .evaluate(&context(epoch, &other_terminal, None), Utc::now())
            .traces[0]
            .matched
    );
    assert!(
        engine
            .evaluate(&context(epoch, &terminal, None), Utc::now())
            .traces[0]
            .matched
    );
    assert!(!engine.rules()[0].enabled);
    let revision = engine.rules()[0].revision;
    engine.toggle(id, revision, true).unwrap();
    assert!(
        !engine
            .evaluate(&context(epoch, &terminal, None), Utc::now())
            .traces[0]
            .matched
    );
    engine.restart(RuntimeEpoch::new());
    let new_epoch = engine.runtime_epoch.unwrap();
    assert!(
        !engine
            .evaluate(&context(new_epoch, &terminal, None), Utc::now())
            .traces[0]
            .matched
    );
}

// RULE-007
#[test]
fn changing_match_conditions_resets_existing_hit_counters() {
    let epoch = RuntimeEpoch::new();
    let rule = Rule::create(draft(
        MessageStage::Request,
        vec![MatchCondition::NthHit(2)],
        vec![RuleAction::Pause],
    ))
    .unwrap();
    let id = rule.id;
    let terminal = TerminalIdentity {
        source_ip: "10.0.0.8".into(),
        certificate_sha256: "cert".into(),
    };
    let mut engine = RuleEngine::new(epoch, vec![rule]);
    assert!(
        !engine
            .evaluate(&context(epoch, &terminal, None), Utc::now())
            .traces[0]
            .matched
    );
    let mut changed = draft(
        MessageStage::Request,
        vec![MatchCondition::NthHit(3)],
        vec![RuleAction::Pause],
    );
    changed.expected_revision = Some(Revision::INITIAL);
    engine.save(id, changed).unwrap();
    assert!(
        !engine
            .evaluate(&context(epoch, &terminal, None), Utc::now())
            .traces[0]
            .matched
    );
    assert!(
        !engine
            .evaluate(&context(epoch, &terminal, None), Utc::now())
            .traces[0]
            .matched
    );
}

// RULE-007
#[test]
fn reconcile_preserves_unrelated_rule_counters_and_resets_changed_rule() {
    let epoch = RuntimeEpoch::new();
    let unchanged = Rule::create(draft(
        MessageStage::Request,
        vec![MatchCondition::NthHit(3)],
        vec![RuleAction::Pause],
    ))
    .unwrap();
    let mut changed = Rule::create(draft(
        MessageStage::Request,
        vec![MatchCondition::NthHit(2)],
        vec![RuleAction::Pause],
    ))
    .unwrap();
    changed.priority = 20;
    let unchanged_id = unchanged.id;
    let changed_id = changed.id;
    let terminal = TerminalIdentity {
        source_ip: "10.0.0.8".into(),
        certificate_sha256: "cert".into(),
    };
    let mut engine = RuleEngine::new(epoch, vec![unchanged.clone(), changed.clone()]);

    let first = engine.evaluate(&context(epoch, &terminal, None), Utc::now());
    assert!(
        first.traces.iter().all(|trace| !trace.matched),
        "both counters should be below their thresholds"
    );

    changed.conditions = vec![MatchCondition::NthHit(3)];
    changed.revision = changed.revision.next();
    engine.reconcile(vec![unchanged, changed]);

    let second = engine.evaluate(&context(epoch, &terminal, None), Utc::now());
    assert!(
        second
            .traces
            .iter()
            .find(|trace| trace.rule_id == unchanged_id)
            .is_some_and(|trace| !trace.matched)
    );
    assert!(
        second
            .traces
            .iter()
            .find(|trace| trace.rule_id == changed_id)
            .is_some_and(|trace| !trace.matched)
    );

    let third = engine.evaluate(&context(epoch, &terminal, None), Utc::now());
    assert!(
        third
            .traces
            .iter()
            .find(|trace| trace.rule_id == unchanged_id)
            .is_some_and(|trace| trace.matched),
        "editing another rule must not reset this rule's Nth-hit counter"
    );
    assert!(
        third
            .traces
            .iter()
            .find(|trace| trace.rule_id == changed_id)
            .is_some_and(|trace| !trace.matched),
        "the changed rule must restart its Nth-hit counter"
    );
}

// RULE-002, RULE-007
#[test]
fn displayed_hit_metadata_resets_on_restart_and_reenable() {
    let epoch = RuntimeEpoch::new();
    let rule = Rule::create(draft(
        MessageStage::Request,
        Vec::new(),
        vec![RuleAction::Pause],
    ))
    .unwrap();
    let id = rule.id;
    let terminal = TerminalIdentity {
        source_ip: "10.0.0.8".into(),
        certificate_sha256: "cert".into(),
    };
    let mut engine = RuleEngine::new(epoch, vec![rule]);
    engine.evaluate(&context(epoch, &terminal, None), Utc::now());
    assert_eq!(engine.rules()[0].hit_count, 1);
    assert!(engine.rules()[0].last_hit_at.is_some());

    engine.restart(RuntimeEpoch::new());
    assert_eq!(engine.rules()[0].hit_count, 0);
    assert!(engine.rules()[0].last_hit_at.is_none());

    let revision = engine.rules()[0].revision;
    engine.toggle(id, revision, false).unwrap();
    let revision = engine.rules()[0].revision;
    engine.toggle(id, revision, true).unwrap();
    assert_eq!(engine.rules()[0].hit_count, 0);
    assert!(engine.rules()[0].last_hit_at.is_none());
}

// ENGINE-005, RULE-011, ACTION-001, ACTION-009
