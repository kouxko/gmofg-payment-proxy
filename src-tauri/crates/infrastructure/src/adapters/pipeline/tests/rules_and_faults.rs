include!("rules_and_faults/conflict_retry.rs");
#[tokio::test]
async fn failed_http_action_keeps_one_shot_and_hit_metadata_unchanged() {
    let mut view = one_shot_delay_rule();
    view.draft.actions = vec![intercept_proxy_application::RuleAction::SetJsonField {
        path: "$.amount".into(),
        value_json: "200".into(),
    }];
    let rule = view_to_domain_rule(view).expect("rule");
    let original_rule_revision = rule.revision;
    let rules = Arc::new(StaticRules {
        snapshot: Mutex::new(RuleRuntimeSnapshot::new(vec![rule])),
    });
    let pipeline = RuntimePipelineAdapter::new(
        test_product_hooks(),
        rules.clone(),
        Arc::new(InMemorySessionStore::default()),
        Arc::new(BreakpointCoordinator::default()),
        Arc::new(EventHub::new(16)),
        test_capture_repository(),
    );
    let epoch = Uuid::new_v4();
    let context = test_context(epoch, Uuid::new_v4(), transaction_channel());
    pipeline.runtime_started(epoch).await;
    let message = request_message("not-json");
    let original = message.clone();
    let collection_revision = rules.snapshot.lock().collection_revision;

    let error = pipeline
        .evaluate(
            &context,
            DomainMessageStage::Request,
            Some(&message),
            test_body_codec(),
        )
        .await
        .expect_err("invalid JSON action must fail atomically");

    assert_eq!(error.code, "JSON_INVALID");
    assert_eq!(message.body, original.body);
    assert_eq!(message.headers, original.headers);
    let persisted = rules.snapshot.lock();
    assert_eq!(persisted.collection_revision, collection_revision);
    assert_eq!(persisted.rules[0].revision, original_rule_revision);
    assert!(persisted.rules[0].enabled);
    assert_eq!(persisted.rules[0].hit_count, 0);
    assert_eq!(persisted.rules[0].last_hit_at, None);
}

#[tokio::test]
async fn tls_handshake_policy_matches_the_peer_under_current_verification() {
    let fingerprint = "11:22:33:44";
    let pipeline = adapter(vec![tls_fingerprint_reject_rule(fingerprint)], 10);
    let epoch = Uuid::new_v4();
    let mut context = test_context(epoch, Uuid::new_v4(), transaction_channel());
    context.tls_peer = None;
    pipeline.runtime_started(epoch).await;
    pipeline.rule_runtime.prepare_epoch(epoch).unwrap();
    let matching_peer = TlsPeerIdentity {
        sha256_fingerprint: fingerprint.into(),
        subject_summary: "CN=blocked".into(),
    };
    let rejected = tokio::task::spawn_blocking({
        let pipeline = Arc::clone(&pipeline);
        let context = context.clone();
        move || pipeline.reject_tls_handshake(&context, &matching_peer)
    })
    .await
    .unwrap()
    .expect("policy");
    assert!(rejected);

    let other_peer = TlsPeerIdentity {
        sha256_fingerprint: "AA:BB".into(),
        subject_summary: "CN=allowed".into(),
    };
    let rejected = tokio::task::spawn_blocking({
        let pipeline = Arc::clone(&pipeline);
        let context = context.clone();
        move || pipeline.reject_tls_handshake(&context, &other_peer)
    })
    .await
    .unwrap()
    .expect("policy");
    assert!(!rejected);
}

#[tokio::test(flavor = "current_thread")]
async fn aborting_http_caller_after_commit_started_does_not_cancel_actor_state_machine() {
    let rule = view_to_domain_rule(one_shot_delay_rule()).unwrap();
    let commit_entered = Arc::new(tokio::sync::Notify::new());
    let commit_release = Arc::new(tokio::sync::Notify::new());
    let rules = Arc::new(BlockingCommitRules {
        snapshot: Mutex::new(RuleRuntimeSnapshot::new(vec![rule])),
        commit_entered: Arc::clone(&commit_entered),
        commit_release: Arc::clone(&commit_release),
    });
    let pipeline = Arc::new(RuntimePipelineAdapter::new(
        test_product_hooks(),
        rules.clone(),
        Arc::new(InMemorySessionStore::new(10, 64 * 1024 * 1024)),
        Arc::new(BreakpointCoordinator::default()),
        Arc::new(EventHub::new(128)),
        test_capture_repository(),
    ));
    let context = test_context(Uuid::new_v4(), Uuid::new_v4(), transaction_channel());
    pipeline.runtime_started(context.runtime_epoch).await;
    let message = request_message(r#"{"amount":100}"#);
    let entered_wait = commit_entered.notified();
    let caller = tokio::spawn({
        let pipeline = Arc::clone(&pipeline);
        let context = context.clone();
        let message = message.clone();
        async move {
            pipeline
                .evaluate(
                    &context,
                    DomainMessageStage::Request,
                    Some(&message),
                    test_body_codec(),
                )
                .await
        }
    });
    entered_wait.await;

    caller.abort();
    assert!(caller.await.unwrap_err().is_cancelled());
    commit_release.notify_one();

    let next = pipeline
        .evaluate(
            &context,
            DomainMessageStage::Request,
            Some(&message),
            test_body_codec(),
        )
        .await
        .unwrap();
    assert!(next.actions.is_empty(), "durable one-shot was consumed exactly once");
    let persisted = rules.snapshot.lock();
    assert!(!persisted.rules[0].enabled);
    assert_eq!(persisted.rules[0].hit_count, 1);
}

#[tokio::test(flavor = "current_thread")]
async fn aborted_runtime_stopping_still_retires_epoch_and_resets_actor() {
    let collection_id = Uuid::new_v4();
    let entered = Arc::new(tokio::sync::Notify::new());
    let release = Arc::new(tokio::sync::Notify::new());
    let completed = Arc::new(tokio::sync::Notify::new());
    let rules = Arc::new(BlockingStopRules {
        snapshot: Mutex::new(RuleRuntimeSnapshot::with_collection_identity(
            Some(collection_id), 1, vec![view_to_domain_rule(one_shot_delay_rule()).unwrap()],
        )),
        reset_calls: AtomicUsize::new(0),
        stop_reset_entered: Arc::clone(&entered),
        stop_reset_release: Arc::clone(&release),
        stop_reset_completed: Arc::clone(&completed),
    });
    let pipeline = Arc::new(RuntimePipelineAdapter::new(
        test_product_hooks(), rules.clone(), Arc::new(InMemorySessionStore::default()),
        Arc::new(BreakpointCoordinator::default()), Arc::new(EventHub::new(16)),
        test_capture_repository(),
    ));
    let epoch = Uuid::new_v4();
    let context = test_context(epoch, Uuid::new_v4(), transaction_channel());
    pipeline.runtime_started(epoch).await;
    pipeline.evaluate(
        &context, DomainMessageStage::Request, Some(&request_message("body")),
        test_body_codec(),
    ).await.unwrap();
    let entered_wait = entered.notified();
    let completed_wait = completed.notified();
    let stopping = tokio::spawn({
        let pipeline = Arc::clone(&pipeline);
        async move { pipeline.rule_runtime.runtime_stopping(epoch).await }
    });
    entered_wait.await;

    stopping.abort();
    assert!(stopping.await.unwrap_err().is_cancelled());
    release.notify_one();
    completed_wait.await;

    assert_eq!(rules.snapshot.lock().rules[0].hit_count, 0);
    assert!(pipeline.rule_runtime.prepare_epoch(epoch).is_err());
}

#[tokio::test(flavor = "current_thread")]
async fn cancelling_before_mailbox_capacity_is_acquired_never_executes_the_command() {
    let mut rule = view_to_domain_rule(one_shot_delay_rule()).unwrap();
    rule.one_shot = false;
    let entered = Arc::new(tokio::sync::Notify::new());
    let release = Arc::new(tokio::sync::Notify::new());
    let rules = Arc::new(CapacityRules {
        snapshot: Mutex::new(RuleRuntimeSnapshot::new(vec![rule])),
        commit_calls: AtomicUsize::new(0),
        first_commit_entered: Arc::clone(&entered),
        first_commit_release: Arc::clone(&release),
    });
    let pipeline = Arc::new(RuntimePipelineAdapter::new(
        test_product_hooks(), rules.clone(), Arc::new(InMemorySessionStore::default()),
        Arc::new(BreakpointCoordinator::default()), Arc::new(EventHub::new(16)),
        test_capture_repository(),
    ));
    let context = test_context(Uuid::new_v4(), Uuid::new_v4(), transaction_channel());
    pipeline.runtime_started(context.runtime_epoch).await;
    let message = request_message("body");
    let first = tokio::spawn({
        let pipeline = Arc::clone(&pipeline);
        let context = context.clone();
        let message = message.clone();
        async move { pipeline.evaluate(
            &context, DomainMessageStage::Request, Some(&message), test_body_codec(),
        ).await }
    });
    entered.notified().await;

    let mut queued = Vec::new();
    for _ in 0..super::rule_runtime::MAILBOX_CAPACITY {
        let (enqueued, enqueued_wait) = tokio::sync::oneshot::channel();
        queued.push(tokio::spawn({
            let pipeline = Arc::clone(&pipeline);
            let context = context.clone();
            let message = message.clone();
            async move { pipeline.rule_runtime.evaluate_with_enqueue_notification(
                &context, DomainMessageStage::Request, Some(&message),
                test_body_codec().as_ref(), enqueued,
            ).await }
        }));
        enqueued_wait.await.unwrap();
    }
    let (overflow_enqueued, overflow_wait) = tokio::sync::oneshot::channel();
    let overflow_codec = test_body_codec();
    let mut overflow = Box::pin(pipeline.rule_runtime.evaluate_with_enqueue_notification(
        &context, DomainMessageStage::Request, Some(&message), overflow_codec.as_ref(),
        overflow_enqueued,
    ));
    assert!(matches!(poll_body_codec_policy_once(overflow.as_mut()).await, std::task::Poll::Pending));
    drop(overflow);
    assert!(overflow_wait.await.is_err(), "cancelled waiter never enqueued");

    release.notify_one();
    first.await.unwrap().unwrap();
    for task in queued { task.await.unwrap().unwrap(); }
    assert_eq!(
        rules.commit_calls.load(AtomicOrdering::Acquire),
        super::rule_runtime::MAILBOX_CAPACITY + 1,
    );
}

#[tokio::test(flavor = "current_thread")]
async fn repeated_epoch_restart_returns_actor_registry_to_baseline_and_old_epochs_fail_closed() {
    let pipeline = RuntimePipelineAdapter::new(
        test_product_hooks(),
        Arc::new(StaticRules {
            snapshot: Mutex::new(RuleRuntimeSnapshot::new(Vec::new())),
        }),
        Arc::new(InMemorySessionStore::default()),
        Arc::new(BreakpointCoordinator::default()),
        Arc::new(EventHub::new(16)),
        test_capture_repository(),
    );
    let mut retired = Vec::new();
    for _ in 0..128 {
        let epoch = Uuid::new_v4();
        pipeline.runtime_started(epoch).await;
        assert_eq!(pipeline.rule_runtime.registry_counts(), (1, 1));
        pipeline.runtime_stopping(epoch).await;
        assert_eq!(pipeline.rule_runtime.registry_counts(), (0, 0));
        retired.push(epoch);
    }

    let current = Uuid::new_v4();
    pipeline.runtime_started(current).await;
    assert!(pipeline.rule_runtime.prepare_epoch(current).is_ok());
    for epoch in retired {
        assert!(pipeline.rule_runtime.prepare_epoch(epoch).is_err());
    }
    pipeline.runtime_stopping(current).await;
    assert_eq!(pipeline.rule_runtime.registry_counts(), (0, 0));
    assert!(pipeline.state.lock().active_epochs.is_empty());
}

#[test]
fn rule_mutations_use_injected_codec_and_preserve_action_order() {
    let body_codec = test_body_codec();
    let mut message = request_message(r#"{"payment":{"amount":100}}"#);
    let actions = vec![
        RuleAction::SetJsonField {
            path: "$.payment.amount".into(),
            value: json!(200),
        },
        RuleAction::SetHeader {
            name: "x-test".into(),
            value: "yes".into(),
        },
        RuleAction::Delay { milliseconds: 25 },
        RuleAction::Pause,
    ];
    let (faults, pause) =
        apply_rule_actions(body_codec.as_ref(), &mut message, &actions, 42).expect("apply");
    assert!(pause);
    assert_eq!(faults, vec![FaultAction::Delay(Duration::from_millis(25))]);
    assert_eq!(
        decode_json(body_codec.as_ref(), &message.body).expect("json")["payment"]["amount"],
        200
    );
    assert_eq!(message.declared_content_length(), Some(message.body.len()));
    assert_eq!(header_value(&message, "x-test").as_deref(), Some("yes"));

    let mock = map_terminal_action(&TerminalAction::MockResponse {
        status: 503,
        headers: vec![("x-mock".into(), "enabled".into())],
        body_bytes: br#"{"mock":true}"#.to_vec(),
    })
    .expect("mock");
    let FaultAction::MockResponse {
        status,
        headers,
        body,
    } = mock
    else {
        panic!("expected mock action");
    };
    assert_eq!(status.as_u16(), 503);
    assert_eq!(headers["x-mock"], "enabled");
    assert_eq!(body, Bytes::from_static(br#"{"mock":true}"#));
}

// RULE-008~009, ACTION-012~013, MESSAGE-004~006, TEST-RULE:
// later body/header mutations win and Rust rebuilds Content-Length exactly once.
#[test]
fn body_replacement_and_repeated_header_updates_preserve_action_order() {
    let body_codec = test_body_codec();
    let mut message = request_message(r#"{"original":true}"#);
    message
        .headers
        .push(RawHeader::new(b"x-test".to_vec(), b"old".to_vec()));
    let actions = vec![
        RuleAction::ReplaceBodyText("最初".into()),
        RuleAction::ReplaceBodyText("最終".into()),
        RuleAction::SetHeader {
            name: "x-test".into(),
            value: "first".into(),
        },
        RuleAction::SetHeader {
            name: "x-test".into(),
            value: "last".into(),
        },
    ];

    let (faults, pause) = apply_rule_actions(body_codec.as_ref(), &mut message, &actions, 42)
        .expect("apply mutations");
    assert!(faults.is_empty());
    assert!(!pause);
    assert_eq!(
        decode_body(body_codec.as_ref(), &message.body).expect("decode"),
        "最終"
    );
    assert_eq!(message.declared_content_length(), Some(message.body.len()));
    assert_eq!(header_value(&message, "x-test").as_deref(), Some("last"));
    assert_eq!(
        message
            .headers
            .iter()
            .filter(|header| header.name.eq_ignore_ascii_case(b"x-test"))
            .count(),
        1,
        "SetHeader replaces all earlier values for the same header"
    );
}

// ACTION-001~011, TEST-FAULT:
// all terminal domain actions map to one explicit transport disposition.
#[test]
fn every_terminal_action_maps_to_the_expected_transport_fault() {
    let cases = vec![
        (TerminalAction::RejectTlsHandshake, FaultAction::RejectTls),
        (
            TerminalAction::DisconnectBeforeUpstream,
            FaultAction::DisconnectBeforeUpstream,
        ),
        (
            TerminalAction::UpstreamConnectTimeout { milliseconds: 1 },
            FaultAction::UpstreamConnectTimeout(Duration::from_millis(1)),
        ),
        (
            TerminalAction::UpstreamWriteTimeout { milliseconds: 1 },
            FaultAction::UpstreamWriteTimeout(Duration::from_millis(1)),
        ),
        (
            TerminalAction::UpstreamReadTimeout { milliseconds: 1 },
            FaultAction::UpstreamReadTimeout(Duration::from_millis(1)),
        ),
        (
            TerminalAction::DropUpstreamResponse {
                mode: DropResponseMode::ReadCompleteResponse,
            },
            FaultAction::DropResponse {
                read_upstream: true,
            },
        ),
        (
            TerminalAction::DropUpstreamResponse {
                mode: DropResponseMode::CloseAfterRequestWrite,
            },
            FaultAction::DropResponse {
                read_upstream: false,
            },
        ),
        (
            TerminalAction::InvalidJson {
                body_bytes: b"{".to_vec(),
            },
            FaultAction::ReplaceBody {
                body: Bytes::from_static(b"{"),
            },
        ),
        (
            TerminalAction::IncorrectContentLength { delta: -1 },
            FaultAction::ContentLengthOffset(-1),
        ),
        (
            TerminalAction::TruncateResponse { bytes: 2 },
            FaultAction::TruncateResponse(2),
        ),
    ];
    for (domain, expected) in cases {
        assert_eq!(
            map_terminal_action(&domain).expect("map terminal action"),
            expected,
            "unexpected transport mapping for {domain:?}"
        );
    }

    let mock = map_terminal_action(&TerminalAction::MockResponse {
        status: 202,
        headers: vec![("x-mock".into(), "yes".into())],
        body_bytes: br#"{"mock":true}"#.to_vec(),
    })
    .expect("map mock response");
    let FaultAction::MockResponse {
        status,
        headers,
        body,
    } = mock
    else {
        panic!("expected mock response transport fault");
    };
    assert_eq!(status.as_u16(), 202);
    assert_eq!(headers["x-mock"], "yes");
    assert_eq!(body, Bytes::from_static(br#"{"mock":true}"#));
}
