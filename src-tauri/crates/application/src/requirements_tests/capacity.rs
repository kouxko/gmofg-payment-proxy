use super::*;

#[test]
fn logical_byte_accounting_is_exact_and_repeatable() {
    let message = content(b"12345");
    let expected_message = MessageContentViewModel::ENTITY_FIXED_OVERHEAD_BYTES
        + "content-type".len() as u64
        + "application/json".len() as u64
        + "application/json".len() as u64
        + "utf-8".len() as u64
        + 5
        + 5;
    assert_eq!(message.logical_bytes(), expected_message);
    assert_eq!(message.logical_bytes(), message.clone().logical_bytes());

    let record = session(Uuid::nil(), 1, true, b"12345");
    let trace_bytes = serde_json::to_vec(&record.detail.rule_trace)
        .expect("serializable trace")
        .len() as u64;
    let policy_result_bytes = serde_json::to_vec(&(
        &record.detail.extracted_metadata,
        &record.detail.response_assertions,
    ))
    .expect("serializable workspace policy results")
    .len() as u64;
    let summary = &record.detail.summary;
    let strings = summary.request_id.len()
        + summary.terminal_ip.len()
        + summary.channel_text.len()
        + summary.method.len()
        + summary.target.len()
        + summary.result.len()
        + record.detail.connection_id.len()
        + record.detail.certificate_fingerprint.len()
        + record.detail.upstream_host.len()
        + record.detail.app_to_proxy_tls.len()
        + record.detail.proxy_to_server_tls.len()
        + record.detail.final_action.len();
    let expected = SessionRecord::ENTITY_FIXED_OVERHEAD_BYTES
        + strings as u64
        + trace_bytes
        + policy_result_bytes
        + expected_message
        + content(b"draft").logical_bytes();
    assert_eq!(record.logical_bytes(), expected);
}

#[test]
fn event_capacity_replacement_is_atomic_on_success_and_failure() {
    let ledger = CapacityLedger::new(100);
    assert!(ledger.try_reserve_event_bytes(60));
    assert!(ledger.try_set_session_bytes(40));

    assert!(
        !ledger.try_replace_event_bytes(60, 61),
        "a larger replacement must fail without exposing the old reservation"
    );
    assert_eq!(ledger.event_bytes(), 60);
    assert_eq!(ledger.logical_bytes(), 100);

    assert!(ledger.try_replace_event_bytes(60, 50));
    assert_eq!(ledger.event_bytes(), 50);
    assert_eq!(ledger.logical_bytes(), 90);
}

// DATA-007~011, TEST-CAPACITY: dual limits evict oldest completed and protect pending sessions.
#[test]
fn capacity_evicts_completed_in_order_and_rejects_when_all_are_pending() {
    let store = InMemorySessionStore::new(2, u64::MAX);
    let oldest = Uuid::from_u128(1);
    let newer = Uuid::from_u128(2);
    let pending = Uuid::from_u128(3);
    store
        .upsert(session(oldest, 1, false, b"a"))
        .expect("insert oldest");
    store
        .upsert(session(newer, 2, false, b"b"))
        .expect("insert newer");
    assert_eq!(
        store
            .upsert(session(pending, 3, true, b"c"))
            .expect("pending insert evicts completed"),
        vec![oldest]
    );

    store
        .upsert(session(newer, 2, true, b"b"))
        .expect("make remaining completed session pending");
    let rejected = store
        .upsert(session(Uuid::from_u128(4), 4, true, b"d"))
        .expect_err("all existing sessions and incoming session are protected");
    assert_eq!(rejected.view_model.code, "RESOURCE_EXHAUSTED");
    assert_eq!(store.len(), 2, "failed insert rolls back atomically");
}

// DATA-009~011: an active session without a breakpoint is never an eviction candidate.
#[test]
fn capacity_never_evicts_active_sessions_without_breakpoints() {
    let store = InMemorySessionStore::new(1, u64::MAX);
    let active_id = Uuid::from_u128(1);
    let mut active = session(active_id, 1, false, b"active");
    active.detail.summary.completed_at = None;
    active.detail.summary.pending_breakpoint = false;
    store.upsert(active).expect("insert active session");

    let completed_id = Uuid::from_u128(2);
    let rejected = store
        .upsert(session(completed_id, 2, false, b"completed"))
        .expect_err("protected active session prevents admitting another session");
    assert_eq!(rejected.view_model.code, "RESOURCE_EXHAUSTED");
    assert!(SessionStore::get(&store, active_id).is_ok());
    assert!(SessionStore::get(&store, completed_id).is_err());
    assert_eq!(
        SessionStore::clear_completed(&store),
        0,
        "clear does not remove active sessions"
    );
}

// DATA-008~011, TEST-CAPACITY: sessions and UI events share one admission ledger.
#[test]
fn shared_capacity_ledger_evicts_only_completed_sessions_without_snapshot_sync() {
    let old = session(Uuid::from_u128(1), 1, false, b"body");
    let replacement = session(Uuid::from_u128(2), 2, false, b"body");
    let payload = UiEventPayload::ResourceWarning {
        message: "共享容量".into(),
    };
    let event_bytes = UiEventEnvelope {
        event_id: 1,
        runtime_epoch: None,
        occurred_at: timestamp(1),
        entity_id: None,
        entity_revision: None,
        payload: payload.clone(),
    }
    .logical_bytes();
    let ledger = Arc::new(CapacityLedger::new(
        old.logical_bytes().saturating_add(event_bytes),
    ));
    let store = InMemorySessionStore::with_capacity_ledger(10, Arc::clone(&ledger));
    let hub = EventHub::with_capacity_ledger(16, Arc::clone(&ledger));
    store.upsert(old).expect("insert completed session");
    hub.publish(None, timestamp(1), None, None, payload);

    let evicted = store
        .upsert(replacement)
        .expect("event allocation makes the older completed session evictable");
    assert_eq!(evicted, vec![Uuid::from_u128(1)]);
    assert_eq!(store.logical_bytes(), ledger.logical_bytes());
    assert!(ledger.logical_bytes() <= ledger.max_bytes());
}

// SESSION-002~003, CAPTURE-004: filtering, sorting, pagination and total are Rust deterministic.
#[test]
fn session_query_is_deterministic() {
    let store = InMemorySessionStore::new(10, u64::MAX);
    for second in [3, 1, 2] {
        store
            .upsert(session(
                Uuid::from_u128(u128::from(second)),
                second,
                false,
                b"x",
            ))
            .expect("insert");
    }
    let page = SessionStore::query(
        &store,
        &SessionQuery {
            keyword: Some("payment".into()),
            terminal_ip: Some("10.0.0".into()),
            channel: Some(test_channel("alpha")),
            result: Some("成功".into()),
            rule_id: None,
            started_from: None,
            started_to: None,
            sort: SessionSort::StartedAt,
            direction: SortDirection::Asc,
            page: PageRequest {
                page: 1,
                page_size: 2,
            },
        },
    );
    assert_eq!(page.total, 3);
    assert_eq!(page.total_pages, 2);
    assert_eq!(
        page.items
            .iter()
            .map(|item| item.request_id.as_str())
            .collect::<Vec<_>>(),
        vec!["REQ-1", "REQ-2"]
    );
}

// DATA-004~006, BREAKPOINT-013~015, TEST-BREAKPOINT.
#[tokio::test]
async fn event_admission_reclaims_replay_and_slow_queues_without_overcommit() {
    let payload = UiEventPayload::ResourceWarning {
        message: "容量压力".into(),
    };
    let event_bytes = UiEventEnvelope {
        event_id: 1,
        runtime_epoch: None,
        occurred_at: timestamp(1),
        entity_id: None,
        entity_revision: None,
        payload: payload.clone(),
    }
    .logical_bytes();
    let ledger = Arc::new(CapacityLedger::new(event_bytes.saturating_mul(2)));
    let hub = EventHub::with_capacity_ledger(4_096, Arc::clone(&ledger));
    let mut slow = hub.subscribe(0, 4).expect("subscribe");

    hub.publish(None, timestamp(1), None, None, payload.clone());
    hub.publish(None, timestamp(2), None, None, payload);

    assert!(ledger.logical_bytes() <= ledger.max_bytes());
    assert!(matches!(
        slow.live
            .recv()
            .await
            .expect("capacity termination returns a control event")
            .payload,
        UiEventPayload::SnapshotRequired { .. }
    ));
    assert!(
        slow.live.recv().await.is_none(),
        "the capacity control event is emitted exactly once"
    );
    assert!(
        hub.replay_after(0).events.len() <= 2,
        "byte pressure shortens replay instead of overcommitting"
    );
}

#[tokio::test]
async fn capture_replacement_pressure_returns_snapshot_instead_of_silent_close() {
    let row = capture_row(1);
    let pending_bytes = serde_json::to_vec(&vec![row.clone()])
        .expect("capture row serializes")
        .len() as u64;
    let ledger = Arc::new(CapacityLedger::new(pending_bytes));
    let hub = EventHub::with_capacity_ledger(4_096, Arc::clone(&ledger));
    let mut subscription = hub.subscribe(0, 4).expect("subscribe");

    assert!(
        hub.push_capture(Uuid::nil(), timestamp(1), row).is_none(),
        "one row remains pending until explicit flush"
    );
    assert!(
        hub.flush_capture(timestamp(2)).is_some(),
        "flush constructs the replacement envelope"
    );

    assert!(matches!(
        subscription
            .live
            .recv()
            .await
            .expect("replacement pressure returns a control event")
            .payload,
        UiEventPayload::SnapshotRequired { .. }
    ));
    assert!(subscription.live.recv().await.is_none());
    assert!(ledger.logical_bytes() <= ledger.max_bytes());
}

#[tokio::test]
async fn live_delivery_replay_eviction_records_one_resource_warning() {
    let payload = UiEventPayload::ResourceWarning {
        message: "x".repeat(2_048),
    };
    let event_bytes = UiEventEnvelope {
        event_id: 1,
        runtime_epoch: None,
        occurred_at: timestamp(1),
        entity_id: None,
        entity_revision: None,
        payload: payload.clone(),
    }
    .logical_bytes();
    let compact_warning_bytes = UiEventEnvelope {
        event_id: 2,
        runtime_epoch: None,
        occurred_at: timestamp(1),
        entity_id: None,
        entity_revision: None,
        payload: UiEventPayload::ResourceWarning {
            message: "UI 补发日志已淘汰旧事件；页面必须重新查询快照。".into(),
        },
    }
    .logical_bytes();
    let ledger = Arc::new(CapacityLedger::new(
        event_bytes.saturating_add(compact_warning_bytes),
    ));
    let hub = EventHub::with_capacity_ledger(4_096, Arc::clone(&ledger));
    let mut subscription = hub.subscribe(0, 4).expect("subscribe");

    hub.publish(None, timestamp(1), None, None, payload);

    assert_eq!(
        subscription
            .live
            .recv()
            .await
            .expect("primary event remains live")
            .event_id,
        1
    );
    let warning = subscription
        .live
        .recv()
        .await
        .expect("dispatch-only replay eviction sends a warning");
    assert!(matches!(
        warning.payload,
        UiEventPayload::ResourceWarning { ref message }
            if message.contains("重新查询快照")
    ));
    assert!(ledger.logical_bytes() <= ledger.max_bytes());
}

#[tokio::test]
async fn live_delivery_overflow_returns_snapshot_when_warning_cannot_be_reserved() {
    let payload = UiEventPayload::ResourceWarning {
        message: "x".repeat(2_048),
    };
    let event_bytes = UiEventEnvelope {
        event_id: 1,
        runtime_epoch: None,
        occurred_at: timestamp(1),
        entity_id: None,
        entity_revision: None,
        payload: payload.clone(),
    }
    .logical_bytes();
    let ledger = Arc::new(CapacityLedger::new(event_bytes));
    let hub = EventHub::with_capacity_ledger(4_096, Arc::clone(&ledger));
    let mut subscription = hub.subscribe(0, 4).expect("subscribe");

    hub.publish(None, timestamp(1), None, None, payload);

    let terminal = subscription
        .live
        .recv()
        .await
        .expect("bounded control path returns a terminal event");
    assert!(matches!(
        terminal.payload,
        UiEventPayload::SnapshotRequired { .. }
    ));
    assert!(
        subscription.live.recv().await.is_none(),
        "terminal notice is emitted exactly once"
    );
    assert!(ledger.logical_bytes() <= ledger.max_bytes());
}

#[tokio::test]
async fn replay_clones_remain_accounted_until_consumed_or_dropped() {
    let ledger = Arc::new(CapacityLedger::new(1024 * 1024));
    let hub = EventHub::with_capacity_ledger(4_096, Arc::clone(&ledger));
    let event = hub.publish(
        None,
        timestamp(1),
        None,
        None,
        UiEventPayload::ResourceWarning {
            message: "补发记账".into(),
        },
    );
    let retained_bytes = ledger.event_bytes();

    let mut consumed = hub.subscribe_default(0).expect("subscribe with replay");
    assert_eq!(
        ledger.event_bytes(),
        retained_bytes.saturating_add(event.logical_bytes())
    );
    consumed
        .replay
        .drain_with(|received| {
            assert_eq!(received.event_id, event.event_id);
            Ok::<_, ()>(())
        })
        .expect("consume replay");
    assert_eq!(ledger.event_bytes(), retained_bytes);
    drop(consumed);

    let unconsumed = hub.subscribe_default(0).expect("subscribe with replay");
    assert_eq!(
        ledger.event_bytes(),
        retained_bytes.saturating_add(event.logical_bytes())
    );
    drop(unconsumed);
    assert_eq!(ledger.event_bytes(), retained_bytes);
}
