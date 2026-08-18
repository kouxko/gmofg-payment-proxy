use super::*;

#[test]
fn current_rule_persistence_round_trips_strictly() {
    let rule = Rule::create(
        to_domain_draft(&request_delay_draft("current", false), 1).expect("current draft"),
    )
    .expect("current rule");
    let value = serialize_persisted_rule(&rule).expect("serialize current rule");

    let decoded = deserialize_persisted_rule(value).expect("deserialize current rule");

    assert_eq!(decoded, rule);
}

#[test]
fn rule_persistence_rejects_unversioned_and_unknown_fields() {
    let rule = Rule::create(
        to_domain_draft(&request_delay_draft("current", false), 1).expect("current draft"),
    )
    .expect("current rule");
    let mut unversioned = serialize_persisted_rule(&rule).expect("serialize rule");
    unversioned
        .as_object_mut()
        .expect("rule object")
        .remove(PERSISTENCE_VERSION_FIELD);
    assert!(deserialize_persisted_rule(unversioned).is_err());

    let mut unknown = serialize_persisted_rule(&rule).expect("serialize rule");
    unknown
        .as_object_mut()
        .expect("rule object")
        .insert("shift_jis_body".into(), serde_json::json!([123]));
    assert!(deserialize_persisted_rule(unknown).is_err());
}

#[tokio::test]
async fn runtime_commit_is_full_signature_cas_and_reset_preserves_enabled() {
    let adapter = adapter();
    let created = adapter
        .save(request_delay_draft("one-shot", true))
        .await
        .expect("create");
    let snapshot = runtime_snapshot(&adapter);
    let epoch = RuntimeEpoch::new();
    let terminal = TerminalIdentity {
        source_ip: "127.0.0.1".into(),
        certificate_sha256: String::new(),
    };
    let mut engine = RuleEngine::new(epoch, snapshot.rules.clone());
    engine.evaluate(
        &MatchContext {
            runtime_epoch: epoch,
            channel: ChannelId::new("alpha").unwrap(),
            stage: MessageStage::Request,
            terminal: &terminal,
            path_or_request_type: None,
            json_body: None,
        },
        Utc.timestamp_opt(1_700_000_000, 0).unwrap(),
    );
    adapter
        .commit_runtime_snapshot(&snapshot, engine.rules())
        .expect("runtime commit");

    let fired = adapter
        .get_domain(created.summary.rule_id)
        .expect("fired rule");
    assert!(!fired.enabled);
    assert_eq!(fired.revision, Revision::new(2));
    assert_eq!(fired.hit_count, 1);
    assert!(fired.last_hit_at.is_some());

    adapter
        .reset_runtime_hit_metadata(snapshot.collection_id.expect("workspace id"))
        .expect("explicit reset");
    let reset = adapter
        .get_domain(created.summary.rule_id)
        .expect("reset rule");
    assert!(!reset.enabled);
    assert_eq!(reset.revision, Revision::new(2));
    assert_eq!(reset.hit_count, 0);
    assert_eq!(reset.last_hit_at, None);

    let stale = runtime_snapshot(&adapter);
    adapter
        .toggle_domain(created.summary.rule_id, 2, true)
        .expect("concurrent config update");
    let error = adapter
        .commit_runtime_snapshot(&stale, &stale.rules)
        .expect_err("stale runtime commit");
    assert_eq!(error.view_model.code, "REVISION_CONFLICT");
    let configured = adapter
        .get_domain(created.summary.rule_id)
        .expect("configured rule");
    assert!(configured.enabled);
    assert_eq!(configured.revision, Revision::new(3));
}
