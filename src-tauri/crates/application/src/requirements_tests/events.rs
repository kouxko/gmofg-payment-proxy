use super::*;

#[test]
fn capture_batching_uses_size_and_time_boundaries() {
    let hub = EventHub::default();
    let epoch = Uuid::nil();
    for event_id in 1..200 {
        assert!(
            hub.push_capture(epoch, timestamp(0), capture_row(event_id))
                .is_none()
        );
    }
    let batch = hub
        .push_capture(epoch, timestamp(0), capture_row(200))
        .expect("200th row flushes");
    let UiEventPayload::CaptureRowsAdded(rows) = batch.payload else {
        panic!("expected capture batch");
    };
    assert_eq!(rows.len(), 200);

    hub.push_capture(epoch, timestamp(0), capture_row(201));
    assert!(hub.flush_due(timestamp(0)).is_none());
    let flushed = hub
        .flush_due(timestamp(1))
        .expect("elapsed time flushes pending row");
    let UiEventPayload::CaptureRowsAdded(rows) = flushed.payload else {
        panic!("expected capture batch");
    };
    assert_eq!(rows.len(), 1);
}

// TEST-EVENT, NFR-004: the production ticker flushes a partial batch without UI polling.
#[tokio::test]
async fn capture_flush_task_flushes_partial_batch_on_clock() {
    let hub = Arc::new(EventHub::default());
    let epoch = Uuid::nil();
    let cancellation = tokio_util::sync::CancellationToken::new();
    let mut subscription = hub.subscribe_default(0).unwrap();
    let task = Arc::clone(&hub).spawn_capture_flush_task(cancellation.clone());
    assert!(
        hub.push_capture(epoch, Utc::now(), capture_row(1))
            .is_none()
    );
    let event = tokio::time::timeout(Duration::from_secs(1), subscription.live.recv())
        .await
        .expect("flush completes before timeout")
        .expect("capture event");
    assert!(matches!(
        event.payload,
        UiEventPayload::CaptureRowsAdded(ref rows) if rows.len() == 1
    ));
    cancellation.cancel();
    task.await.expect("flush task exits");
}

// TEST-EVENT, NFR-005~007: slow subscribers terminate independently without truncating replay.
#[tokio::test]
async fn subscriber_overflow_is_non_blocking_and_replay_log_is_independent() {
    let hub = EventHub::new(4_096);
    let mut slow = hub.subscribe(0, 1).unwrap();
    let mut healthy = hub.subscribe(0, 4).unwrap();
    let payload = || UiEventPayload::ResourceWarning {
        message: "测试".into(),
    };

    hub.publish(None, timestamp(0), None, None, payload());
    assert_eq!(
        healthy
            .live
            .recv()
            .await
            .expect("healthy receives")
            .event_id,
        1
    );
    hub.publish(None, timestamp(1), None, None, payload());
    assert_eq!(
        healthy
            .live
            .recv()
            .await
            .expect("healthy still receives")
            .event_id,
        2
    );
    hub.publish(None, timestamp(2), None, None, payload());
    assert_eq!(
        healthy
            .live
            .recv()
            .await
            .expect("healthy still receives")
            .event_id,
        3
    );

    let failure = hub
        .take_subscription_failure(slow.subscription_id)
        .expect("overflow reason remains available to the channel adapter");
    assert!(matches!(
        failure.payload,
        UiEventPayload::SnapshotRequired { .. }
    ));
    assert!(hub.take_subscription_failures().is_empty());
    assert_eq!(
        hub.replay_after(0)
            .events
            .iter()
            .filter(|event| event.event_id <= 3)
            .count(),
        3,
        "subscriber overflow does not delete replay history"
    );
    assert!(
        slow.live.recv().await.is_none(),
        "overflow cancels and releases the slow queue immediately"
    );
}

// TEST-EVENT, NFR-005: subscriptions and their queued logical bytes remain bounded.
#[tokio::test]
async fn event_subscriptions_are_bounded_and_queue_bytes_are_released_on_receive() {
    let hub = EventHub::default();
    let mut first = hub.subscribe_default(0).unwrap();
    let mut remaining = (1..EventHub::MAX_SUBSCRIBERS)
        .map(|_| hub.subscribe_default(0).unwrap())
        .collect::<Vec<_>>();
    assert_eq!(
        hub.subscribe_default(0)
            .expect_err("subscriber limit must fail closed")
            .view_model
            .code,
        "RESOURCE_EXHAUSTED"
    );

    let envelope = hub.publish(
        None,
        timestamp(0),
        None,
        None,
        UiEventPayload::ResourceWarning {
            message: "有界队列".into(),
        },
    );
    let before_receive = hub.logical_bytes();
    assert_eq!(
        first.live.recv().await.expect("queued event").event_id,
        envelope.event_id
    );
    assert_eq!(
        before_receive - hub.logical_bytes(),
        envelope.logical_bytes(),
        "receiving an event releases its tracked queue bytes"
    );

    for subscription in &mut remaining {
        let _ = subscription.live.recv().await;
    }
}

#[tokio::test]
async fn event_subscription_drop_and_explicit_cancel_release_all_live_state() {
    let hub = Arc::new(EventHub::default());
    let mut subscriptions = (0..EventHub::MAX_SUBSCRIBERS)
        .map(|_| hub.subscribe_default(0).expect("subscribe"))
        .collect::<Vec<_>>();
    let event = hub.publish(
        None,
        timestamp(0),
        None,
        None,
        UiEventPayload::ResourceWarning {
            message: "RAII".into(),
        },
    );
    let queued = hub.logical_bytes();
    let retained = hub.replay_after(0).events[0].logical_bytes();
    assert_eq!(
        queued,
        retained.saturating_mul((EventHub::MAX_SUBSCRIBERS + 1) as u64)
    );

    let mut cancelled = subscriptions.pop().expect("subscription");
    hub.unsubscribe(cancelled.subscription_id);
    assert_eq!(
        hub.logical_bytes(),
        queued,
        "cancellation must not release bytes while the receiver still owns its buffered event"
    );
    assert!(
        cancelled.live.recv().await.is_none(),
        "explicit cancellation ends the receiver without draining its queue"
    );
    assert_eq!(
        hub.logical_bytes(),
        queued,
        "observing cancellation still leaves queue ownership with the live receiver"
    );
    drop(cancelled);
    assert_eq!(
        hub.logical_bytes(),
        queued.saturating_sub(event.logical_bytes()),
        "dropping the cancelled receiver releases its queued bytes exactly once"
    );
    assert!(
        hub.subscribe_default(event.event_id).is_ok(),
        "explicit cancellation frees a subscriber slot"
    );

    drop(subscriptions);
    assert_eq!(
        hub.logical_bytes(),
        retained,
        "dropping receivers removes every queued-byte counter"
    );
}

// TEST-EVENT, NFR-006: expired replay cursor produces SnapshotRequired.
#[test]
fn expired_event_cursor_requires_bootstrap() {
    let hub = EventHub::new(3);
    for second in 0..5 {
        hub.publish(
            None,
            timestamp(second),
            None,
            None,
            UiEventPayload::ResourceWarning {
                message: second.to_string(),
            },
        );
    }
    let replay = hub.replay_after(0);
    assert!(replay.snapshot_required);
    assert!(matches!(
        replay.events[0].payload,
        UiEventPayload::SnapshotRequired { .. }
    ));
}

// NFR-008: all recoverable failures cross the adapter boundary as stable Chinese view models.
#[test]
fn app_error_view_model_preserves_stable_contract() {
    let error = AppError::new("CONFIG_INVALID", "设置存在字段错误。")
        .retryable("请修正标记字段后重试。")
        .entity("settings");
    let view: AppErrorViewModel = error.into();
    assert_eq!(view.code, "CONFIG_INVALID");
    assert_eq!(view.message, "设置存在字段错误。");
    assert_eq!(view.entity_id.as_deref(), Some("settings"));
    assert!(view.retryable);
}
