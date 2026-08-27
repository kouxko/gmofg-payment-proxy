#[tokio::test]
async fn records_the_effective_downstream_status_after_response_rules() {
    let pipeline = adapter(vec![response_status_rule(503)], 10);
    let epoch = Uuid::new_v4();
    let context = test_context(epoch, Uuid::new_v4(), transaction_channel());
    open_test_connection(&pipeline, &context).await;

    let mut request = request_message(r#"{"amount":100}"#);
    pipeline
        .apply_request_policy(&context, &mut request)
        .await
        .expect("request");
    let mut response = response_message();
    let actions = pipeline
        .apply_response_policy(&context, &mut response)
        .await
        .expect("response");
    assert!(matches!(
        actions.as_slice(),
        [FaultAction::CustomStatus(status)] if *status == http::StatusCode::SERVICE_UNAVAILABLE
    ));

    let session_id = pipeline
        .state
        .lock()
        .connection(&context)
        .and_then(|connection| connection.session_id)
        .expect("active session");
    let recorded = pipeline.sessions.get_record(session_id).unwrap();
    let recorded_response = recorded.detail.response.expect("effective response");
    assert_eq!(recorded.detail.summary.http_status, Some(503));
    assert_eq!(recorded_response.http_status, Some(503));
    assert_eq!(
        recorded_response.start_line_bytes,
        b"HTTP/1.1 503 Service Unavailable"
    );
}

#[tokio::test]
async fn pending_breakpoints_are_never_evicted_and_stop_unblocks_waiters() {
    let pipeline = adapter(vec![pause_rule()], 1);
    let epoch = Uuid::new_v4();
    let first_context = test_context(epoch, Uuid::new_v4(), transaction_channel());
    open_test_connection(&pipeline, &first_context).await;

    let first = {
        let pipeline = Arc::clone(&pipeline);
        let context = first_context.clone();
        tokio::spawn(async move {
            let mut message = request_message(r#"{"requestId":"first"}"#);
            pipeline.apply_request_policy(&context, &mut message).await
        })
    };
    for _ in 0..100 {
        if pipeline.breakpoints.query(Some(epoch)).len() == 1 {
            break;
        }
        tokio::task::yield_now().await;
    }
    assert_eq!(pipeline.breakpoints.query(Some(epoch)).len(), 1);

    let second_context = test_context(epoch, Uuid::new_v4(), dll_channel());
    open_test_connection(&pipeline, &second_context).await;
    let mut second_message = request_message(r#"{"requestId":"second"}"#);
    let exhausted = pipeline
        .apply_request_policy(&second_context, &mut second_message)
        .await
        .expect_err("pending session consumes the full capacity");
    assert_eq!(exhausted.code, "RESOURCE_EXHAUSTED");
    assert!(pipeline.events.replay_after(0).events.iter().any(|event| {
        matches!(
            event.payload,
            UiEventPayload::ResourceWarning { ref message }
                if message.contains("容量")
        )
    }));

    pipeline.runtime_stopping(epoch).await;
    let stopped = first.await.expect("request task").expect_err("stopped");
    assert_eq!(stopped.code, ErrorCode::ProxyStopped.as_str());
    assert!(pipeline.breakpoints.query(Some(epoch)).is_empty());
}

#[tokio::test]
async fn one_shot_action_is_not_returned_when_runtime_commit_fails() {
    let rule = view_to_domain_rule(one_shot_delay_rule()).expect("rule");
    let rules = Arc::new(RejectingCommitRules {
        snapshot: Mutex::new(RuleRuntimeSnapshot::new(vec![rule])),
        reject_commit: AtomicBool::new(true),
    });
    let pipeline = RuntimePipelineAdapter::new(
        test_product_hooks(),
        rules.clone(),
        Arc::new(InMemorySessionStore::new(10, 64 * 1024 * 1024)),
        Arc::new(BreakpointCoordinator::default()),
        Arc::new(EventHub::new(128)),
        test_capture_repository(),
    );
    let epoch = Uuid::new_v4();
    let context = test_context(epoch, Uuid::new_v4(), transaction_channel());
    open_test_connection(&pipeline, &context).await;
    let mut message = request_message(r#"{"amount":100}"#);

    let error = pipeline
        .apply_request_policy(&context, &mut message)
        .await
        .expect_err("commit failure must fail closed before returning actions");
    assert_eq!(error.code, "REVISION_CONFLICT");
    let persisted = rules.snapshot.lock();
    assert!(persisted.rules[0].enabled);
    assert_eq!(persisted.rules[0].hit_count, 0);
    assert!(pipeline.events.replay_after(0).events.iter().any(|event| {
        matches!(
            event.payload,
            UiEventPayload::OperationFailed(ref error)
                if error.code == "REVISION_CONFLICT"
        )
    }));
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn concurrent_rule_hits_commit_without_lost_updates() {
    let rule = view_to_domain_rule({
        let mut view = one_shot_delay_rule();
        view.draft.one_shot = false;
        view
    })
    .expect("rule");
    let rules = Arc::new(StaticRules {
        snapshot: Mutex::new(RuleRuntimeSnapshot::new(vec![rule])),
    });
    let pipeline = Arc::new(RuntimePipelineAdapter::new(
        test_product_hooks(),
        rules.clone(),
        Arc::new(InMemorySessionStore::new(32, 64 * 1024 * 1024)),
        Arc::new(BreakpointCoordinator::default()),
        Arc::new(EventHub::new(512)),
        test_capture_repository(),
    ));
    let epoch = Uuid::new_v4();
    let mut tasks = Vec::new();
    for index in 0..20_u128 {
        let pipeline = pipeline.clone();
        let context = test_context(epoch, Uuid::from_u128(index + 1), transaction_channel());
        open_test_connection(&pipeline, &context).await;
        tasks.push(tokio::spawn(async move {
            let mut message = request_message(r#"{"amount":100}"#);
            pipeline.apply_request_policy(&context, &mut message).await
        }));
    }
    for task in tasks {
        let actions = task.await.expect("task").expect("request");
        assert_eq!(actions, vec![FaultAction::Delay(Duration::from_millis(25))]);
    }
    assert_eq!(
        rules.snapshot.lock().rules[0].hit_count,
        20,
        "serialized evaluate+commit preserves every concurrent hit"
    );
}
