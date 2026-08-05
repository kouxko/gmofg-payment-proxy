//! 需求编号到应用层行为的回归测试。
//!
//! 这些测试不是示例代码，而是需求文档的可执行证据。测试名称和注释中的需求编号用于
//! 追踪 UI、Rust 用例和验收条件，修改业务语义时必须同步更新需求文档和对应测试。

use std::{
    collections::{BTreeMap, BTreeSet, VecDeque},
    sync::{
        Arc,
        atomic::{AtomicBool, AtomicUsize, Ordering},
    },
    time::Duration,
};

use async_trait::async_trait;
use chrono::{DateTime, TimeZone, Utc};
use intercept_proxy_domain::{FixedServerSettings, UpstreamTlsSettings};
use intercept_proxy_product_api::{BodyCodec, ProductError};
use uuid::Uuid;

use crate::*;

#[derive(Debug)]
struct Utf8TestCodec;

impl BodyCodec for Utf8TestCodec {
    fn id(&self) -> &'static str {
        "utf-8-test"
    }

    fn name(&self) -> &'static str {
        "UTF-8 Test"
    }

    fn decode(&self, bytes: &[u8]) -> Result<String, ProductError> {
        String::from_utf8(bytes.to_vec())
            .map_err(|error| ProductError::new("BODY_DECODE_FAILED", error.to_string()))
    }

    fn encode(&self, text: &str) -> Result<Vec<u8>, ProductError> {
        Ok(text.as_bytes().to_vec())
    }
}

fn breakpoint_validator() -> BreakpointValidator {
    BreakpointValidator::new(Arc::new(Utf8TestCodec))
}

fn test_channel(id: &str) -> ChannelId {
    ChannelId::new(id).unwrap()
}

fn valid_settings_draft() -> SettingsDraft {
    SettingsDraft {
        channels: vec![
            ChannelSettingsDraft {
                id: test_channel("alpha"),
                display_name: "Alpha".into(),
                enabled: true,
                port: 20_001,
                upstream_url: "https://alpha.example.test".into(),
            },
            ChannelSettingsDraft {
                id: test_channel("beta"),
                display_name: "Beta".into(),
                enabled: true,
                port: 20_002,
                upstream_url: "https://beta.example.test".into(),
            },
        ],
        ..SettingsDraft::default()
    }
}

fn timestamp(second: u32) -> DateTime<Utc> {
    Utc.with_ymd_and_hms(2026, 7, 28, 0, 0, second)
        .single()
        .expect("valid test time")
}

fn content(body: &[u8]) -> MessageContentViewModel {
    MessageContentViewModel {
        http_status: None,
        start_line_bytes: Vec::new(),
        raw_headers: Vec::new(),
        headers: BTreeMap::from([("content-type".into(), vec!["application/json".into()])]),
        body_text: Some(String::from_utf8_lossy(body).into_owned()),
        body_bytes: body.to_vec(),
        json: None,
        content_length: body.len(),
    }
}

fn session(id: SessionId, second: u32, pending: bool, body: &[u8]) -> SessionRecord {
    SessionRecord {
        detail: SessionDetailViewModel {
            summary: SessionSummaryViewModel {
                session_id: id,
                request_id: format!("REQ-{second}"),
                started_at: timestamp(second),
                completed_at: (!pending).then(|| timestamp(second)),
                terminal_ip: format!("10.0.0.{second}"),
                channel: test_channel("alpha"),
                channel_text: "Alpha".into(),
                method: "POST".into(),
                target: "/payment".into(),
                http_status: None,
                result: "成功".into(),
                ui_tone: UiTone::Positive,
                duration_ms: Some(u64::from(second)),
                matched_rule_ids: Vec::new(),
                request_size_bytes: body.len() as u64,
                response_size_bytes: 0,
                pending_breakpoint: pending,
                revision: 1,
            },
            runtime_epoch: Uuid::nil(),
            connection_id: "connection".into(),
            certificate_fingerprint: "fingerprint".into(),
            upstream_host: "example.test".into(),
            app_to_proxy_tls: "TLS 1.2".into(),
            proxy_to_server_tls: "TLS 1.2".into(),
            final_action: "转发".into(),
            timings_ms: BTreeMap::new(),
            request: Some(content(body)),
            response: None,
            rule_trace: vec!["规则轨迹".into()],
            extracted_metadata: BTreeMap::new(),
            response_assertions: Vec::new(),
        },
        breakpoint_draft: pending.then(|| content(b"draft")),
    }
}

fn breakpoint(id: BreakpointId, epoch: RuntimeEpoch, second: u32) -> BreakpointDetailViewModel {
    BreakpointDetailViewModel {
        summary: BreakpointSummaryViewModel {
            breakpoint_id: id,
            session_id: Uuid::new_v4(),
            runtime_epoch: epoch,
            stage: MessageStage::Request,
            title: "请求断点·发送至服务器前".into(),
            terminal_ip: "10.0.0.1".into(),
            channel: test_channel("alpha"),
            channel_text: "Alpha".into(),
            method: "POST".into(),
            target: "/payment".into(),
            waiting_since: timestamp(second),
            certificate_fingerprint_suffix: "A1:B2".into(),
            state: BreakpointState::Pending,
            state_text: String::new(),
            ui_tone: UiTone::Neutral,
            revision: 7,
        },
        original: content(br#"{"a":1}"#),
        effective: content(br#"{"a":1}"#),
        can_resolve: true,
        resolve_disabled_reason: None,
        available_actions: Vec::new(),
    }
}

#[test]
fn breakpoint_action_view_model_is_stage_specific_and_rust_owned() {
    let coordinator = BreakpointCoordinator::default();
    let epoch = Uuid::new_v4();
    let request = coordinator
        .register(breakpoint(Uuid::new_v4(), epoch, 1))
        .expect("request breakpoint");
    let request_kinds = request
        .detail
        .available_actions
        .iter()
        .map(|action| action.kind)
        .collect::<Vec<_>>();
    assert!(request_kinds.contains(&BreakpointDecisionKind::MockResponse));
    assert!(request_kinds.contains(&BreakpointDecisionKind::DisconnectBeforeUpstream));
    assert!(!request_kinds.contains(&BreakpointDecisionKind::CustomHttpStatus));

    let mut response_detail = breakpoint(Uuid::new_v4(), epoch, 2);
    response_detail.summary.stage = MessageStage::Response;
    let response = coordinator
        .register(response_detail)
        .expect("response breakpoint");
    let response_kinds = response
        .detail
        .available_actions
        .iter()
        .map(|action| action.kind)
        .collect::<Vec<_>>();
    assert!(!response_kinds.contains(&BreakpointDecisionKind::MockResponse));
    assert!(!response_kinds.contains(&BreakpointDecisionKind::DisconnectBeforeUpstream));
    assert!(response_kinds.contains(&BreakpointDecisionKind::CustomHttpStatus));
}

fn capture_row(event_id: u64) -> CaptureRowViewModel {
    CaptureRowViewModel {
        event_id,
        runtime_epoch: Uuid::nil(),
        session_id: Uuid::new_v4(),
        occurred_at: timestamp(0),
        terminal_ip: "10.0.0.1".into(),
        channel: test_channel("alpha"),
        channel_text: "交易".into(),
        stage: MessageStage::Request,
        stage_text: "请求".into(),
        method: "POST".into(),
        target: "/payment".into(),
        http_status: None,
        result: "成功".into(),
        ui_tone: UiTone::Positive,
        duration_ms: Some(1),
        matched_rule_ids: Vec::new(),
        size_bytes: 1,
        breakpoint_id: None,
        can_go_to_breakpoint: false,
        breakpoint_disabled_reason: Some(DisabledReason {
            code: "NO_BREAKPOINT".into(),
            message: "该会话没有待处理断点。".into(),
        }),
    }
}

// DATA-008, TEST-CAPACITY: logical bytes are deterministic and use lengths, not allocations.
#[test]
fn logical_byte_accounting_is_exact_and_repeatable() {
    let message = content(b"12345");
    let expected_message = MessageContentViewModel::ENTITY_FIXED_OVERHEAD_BYTES
        + "content-type".len() as u64
        + "application/json".len() as u64
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
async fn breakpoint_resolution_is_atomic_and_epoch_scoped() {
    let coordinator = BreakpointCoordinator::default();
    let id = Uuid::from_u128(10);
    let epoch = Uuid::from_u128(20);
    let ticket = coordinator
        .register(breakpoint(id, epoch, 1))
        .expect("register");
    assert_eq!(ticket.detail.summary.state_text, "等待处理");
    let decision = BreakpointDecision {
        breakpoint_id: id,
        expected_revision: 7,
        kind: BreakpointDecisionKind::ForwardOriginal,
        message: None,
        delay_ms: None,
        http_status: None,
        content_length_delta: None,
        truncate_at: None,
    };
    let summary = coordinator
        .resolve(epoch, decision.clone())
        .expect("first resolution");
    assert_eq!(summary.state, BreakpointState::Resolved);
    assert!(matches!(
        ticket.outcome.await.expect("outcome delivered"),
        BreakpointOutcome::Decision(decision)
            if decision.kind == BreakpointDecisionKind::ForwardOriginal
    ));
    assert_eq!(
        coordinator
            .resolve(epoch, decision)
            .expect_err("second resolution fails")
            .view_model
            .code,
        "BREAKPOINT_ALREADY_RESOLVED"
    );
}

// BREAKPOINT-013~015: cancellation delivers the exact terminal cause to the waiting runtime.
#[tokio::test]
async fn breakpoint_cancellation_preserves_client_and_proxy_causes() {
    let coordinator = BreakpointCoordinator::default();
    let epoch = Uuid::from_u128(20);

    let client_id = Uuid::from_u128(11);
    let client_ticket = coordinator
        .register(breakpoint(client_id, epoch, 1))
        .expect("register client breakpoint");
    let client = coordinator
        .client_disconnected(client_id)
        .expect("terminate disconnected client");
    assert_eq!(client.state, BreakpointState::ClientDisconnected);
    assert_eq!(
        client_ticket.outcome.await.expect("client outcome"),
        BreakpointOutcome::ClientDisconnected
    );

    let stop_id = Uuid::from_u128(12);
    let stop_ticket = coordinator
        .register(breakpoint(stop_id, epoch, 2))
        .expect("register stop breakpoint");
    let stopped = coordinator.proxy_stopped(epoch);
    assert_eq!(stopped.len(), 1);
    assert_eq!(stopped[0].state, BreakpointState::ProxyStopped);
    assert_eq!(
        stop_ticket.outcome.await.expect("stop outcome"),
        BreakpointOutcome::ProxyStopped
    );
}

// DATA-005: terminal tombstones are bounded and old identifiers expire deterministically.
#[test]
fn breakpoint_terminal_tombstones_are_bounded() {
    let coordinator = BreakpointCoordinator::default();
    let epoch = Uuid::from_u128(20);
    let first = Uuid::from_u128(1);
    for index in 1..=4_097_u128 {
        let id = Uuid::from_u128(index);
        coordinator
            .register(breakpoint(id, epoch, 1))
            .expect("register");
        coordinator
            .client_disconnected(id)
            .expect("terminate breakpoint");
    }
    assert_eq!(
        coordinator
            .client_disconnected(first)
            .expect_err("old tombstone expired")
            .view_model
            .code,
        "BREAKPOINT_NOT_FOUND"
    );
    assert_eq!(
        coordinator
            .client_disconnected(Uuid::from_u128(4_097))
            .expect_err("new tombstone retained")
            .view_model
            .code,
        "BREAKPOINT_CLIENT_DISCONNECTED"
    );
}

// TEST-EVENT, NFR-004: capture events flush at 200 rows or 100 ms.
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

fn fake_settings_view() -> SettingsViewModel {
    let stored = SettingsDraft {
        expected_revision: Some(1),
        ..valid_settings_draft()
    };
    SettingsViewModel {
        stored: stored.clone(),
        effective: Some(stored),
        pending_changes: false,
        requires_restart: false,
        restart_reason: None,
        revision: 1,
        can_write: true,
        disabled_reason: None,
        fixed_tls_version: "TLS 1.2".into(),
        redirects_enabled: false,
        retries_enabled: false,
        payload_policy_text: "Payload 仅保存在内存中。".into(),
    }
}

fn fake_certificate_overview() -> CertificateOverviewViewModel {
    CertificateOverviewViewModel {
        revision: 1,
        ready: true,
        status_text: "证书已就绪".into(),
        ui_tone: UiTone::Positive,
        items: vec![CertificateItemViewModel {
            kind: "Proxy 叶子证书".into(),
            subject: "CN=proxy.test".into(),
            usage: "App → Proxy TLS 服务端身份".into(),
            sans: vec!["127.0.0.1".into()],
            valid_from: None,
            valid_until: None,
            sha256_fingerprint: "fingerprint".into(),
            status_text: "有效".into(),
            ui_tone: UiTone::Positive,
        }],
        can_initialize: false,
        can_change: true,
        disabled_reason: None,
    }
}

#[derive(Debug)]
struct FakePorts {
    settings_validations: AtomicUsize,
    proxy_state: parking_lot::Mutex<ProxyState>,
    start_results: parking_lot::Mutex<VecDeque<AppResult<ProxyStatusViewModel>>>,
    start_calls: AtomicUsize,
    stop_calls: AtomicUsize,
    block_start: AtomicBool,
    start_entered: tokio::sync::Notify,
    continue_start: tokio::sync::Notify,
    settings_save_calls: AtomicUsize,
    certificate_import_calls: AtomicUsize,
    certificate_status_calls: AtomicUsize,
    certificate_overview_calls: AtomicUsize,
    certificate_discard_calls: AtomicUsize,
    block_certificate_discard: AtomicBool,
    certificate_discard_entered: tokio::sync::Notify,
    continue_certificate_discard: tokio::sync::Notify,
    discarded_certificate_references: parking_lot::Mutex<BTreeSet<String>>,
    settings: parking_lot::Mutex<SettingsViewModel>,
    certificate_overview: parking_lot::Mutex<CertificateOverviewViewModel>,
}

impl Default for FakePorts {
    fn default() -> Self {
        Self {
            settings_validations: AtomicUsize::new(0),
            proxy_state: parking_lot::Mutex::new(ProxyState::Stopped),
            start_results: parking_lot::Mutex::new(VecDeque::new()),
            start_calls: AtomicUsize::new(0),
            stop_calls: AtomicUsize::new(0),
            block_start: AtomicBool::new(false),
            start_entered: tokio::sync::Notify::new(),
            continue_start: tokio::sync::Notify::new(),
            settings_save_calls: AtomicUsize::new(0),
            certificate_import_calls: AtomicUsize::new(0),
            certificate_status_calls: AtomicUsize::new(0),
            certificate_overview_calls: AtomicUsize::new(0),
            certificate_discard_calls: AtomicUsize::new(0),
            block_certificate_discard: AtomicBool::new(false),
            certificate_discard_entered: tokio::sync::Notify::new(),
            continue_certificate_discard: tokio::sync::Notify::new(),
            discarded_certificate_references: parking_lot::Mutex::new(BTreeSet::new()),
            settings: parking_lot::Mutex::new(fake_settings_view()),
            certificate_overview: parking_lot::Mutex::new(fake_certificate_overview()),
        }
    }
}

#[async_trait]
impl ListenerCertificateImportPort for FakePorts {
    async fn import_downstream_server_identity(
        &self,
        _label: String,
    ) -> AppResult<Option<ListenerCertificateImportViewModel>> {
        Ok(None)
    }

    async fn import_downstream_client_trust(
        &self,
        _label: String,
    ) -> AppResult<Option<ListenerCertificateImportViewModel>> {
        Ok(None)
    }

    async fn import_upstream_client_identity(
        &self,
        _label: String,
        _password: String,
    ) -> AppResult<Option<ListenerCertificateImportViewModel>> {
        Ok(None)
    }

    async fn import_upstream_server_trust(
        &self,
        _label: String,
    ) -> AppResult<Option<ListenerCertificateImportViewModel>> {
        Ok(None)
    }

    async fn inspect(
        &self,
        reference: CertificateReference,
    ) -> AppResult<CertificateItemViewModel> {
        if self
            .discarded_certificate_references
            .lock()
            .contains(&reference.reference)
        {
            return Err(AppError::new(
                "LISTENER_CERTIFICATE_MATERIAL_UNAVAILABLE",
                "托管证书材料已被清理。",
            ));
        }
        Ok(fake_certificate_overview().items.remove(0))
    }

    async fn discard(&self, reference: CertificateReference) -> AppResult<()> {
        self.certificate_discard_calls
            .fetch_add(1, Ordering::SeqCst);
        if self.block_certificate_discard.load(Ordering::SeqCst) {
            self.certificate_discard_entered.notify_one();
            self.continue_certificate_discard.notified().await;
        }
        self.discarded_certificate_references
            .lock()
            .insert(reference.reference);
        Ok(())
    }
}

fn unused<T>() -> AppResult<T> {
    Err(AppError::new("UNUSED_FAKE_PORT", "测试未使用此端口。"))
}

#[async_trait]
impl ProxySupervisorPort for FakePorts {
    async fn status(&self) -> AppResult<ProxyStatusViewModel> {
        Ok(proxy_status(*self.proxy_state.lock()))
    }
    async fn start(&self, _: SettingsDraft) -> AppResult<ProxyStatusViewModel> {
        self.start_calls.fetch_add(1, Ordering::SeqCst);
        if self.block_start.load(Ordering::SeqCst) {
            self.start_entered.notify_one();
            self.continue_start.notified().await;
        }
        if let Some(result) = self.start_results.lock().pop_front() {
            if let Ok(status) = &result {
                *self.proxy_state.lock() = status.state;
            }
            return result;
        }
        *self.proxy_state.lock() = ProxyState::Running;
        Ok(proxy_status(ProxyState::Running))
    }
    async fn stop(&self) -> AppResult<ProxyStatusViewModel> {
        self.stop_calls.fetch_add(1, Ordering::SeqCst);
        *self.proxy_state.lock() = ProxyState::Stopped;
        Ok(proxy_status(ProxyState::Stopped))
    }
}

#[async_trait]
impl CaptureRepositoryPort for FakePorts {
    async fn query(&self, _: CaptureQuery) -> AppResult<CapturePageViewModel> {
        Ok(CapturePageViewModel {
            rows: Vec::new(),
            total: 0,
            page: 1,
            page_size: 5,
            total_pages: 0,
            event_cursor: 0,
            oldest_event_id: None,
            runtime_epoch: None,
            snapshot_required: false,
            empty_message: "暂无抓包记录。".into(),
        })
    }
    async fn get_detail(&self, _: SessionId, _: RuntimeEpoch) -> AppResult<CaptureDetailViewModel> {
        unused()
    }
    async fn clear_view(&self, _: u64) -> AppResult<u64> {
        unused()
    }
}

#[async_trait]
impl SessionQueryPort for FakePorts {
    async fn query(&self, _: SessionQuery) -> AppResult<SessionPageViewModel> {
        unused()
    }
    async fn get(&self, _: SessionId) -> AppResult<SessionDetailViewModel> {
        unused()
    }
    async fn clear_completed(&self) -> AppResult<usize> {
        unused()
    }
}

impl BreakpointValidationPort for FakePorts {
    fn format_json(&self, _: BreakpointDraft) -> AppResult<BreakpointDraft> {
        unused()
    }
    fn restore_original(&self, _: &BreakpointDetailViewModel) -> AppResult<BreakpointDraft> {
        unused()
    }
    fn validate(
        &self,
        _: &BreakpointDetailViewModel,
        _: &BreakpointDraft,
    ) -> AppResult<BreakpointValidationViewModel> {
        unused()
    }
    fn validate_decision(
        &self,
        _: &BreakpointDetailViewModel,
        _: &BreakpointDecision,
    ) -> AppResult<BreakpointValidationViewModel> {
        unused()
    }
}

#[async_trait]
impl RuleRepositoryPort for FakePorts {
    async fn list(&self) -> AppResult<Vec<RuleSummaryViewModel>> {
        unused()
    }
    async fn get(&self, _: RuleId) -> AppResult<RuleViewModel> {
        unused()
    }
    async fn new_draft(&self) -> AppResult<RuleDraft> {
        Ok(RuleDraft {
            rule_id: None,
            expected_revision: None,
            name: "新建规则".into(),
            description: String::new(),
            enabled: true,
            priority: 100,
            channel: None,
            stage: Some(MessageStage::Request),
            conditions: Vec::new(),
            actions: Vec::new(),
            one_shot: false,
        })
    }
    async fn create_from_session(&self, _: SessionId) -> AppResult<RuleDraft> {
        unused()
    }
    async fn validate(&self, _: &RuleDraft) -> AppResult<RuleValidationViewModel> {
        unused()
    }
    async fn save(&self, _: RuleDraft) -> AppResult<RuleViewModel> {
        unused()
    }
    async fn copy(&self, _: RuleId) -> AppResult<RuleViewModel> {
        unused()
    }
    async fn delete(&self, _: RuleId, _: u64) -> AppResult<OperationResultViewModel> {
        unused()
    }
    async fn toggle(&self, _: RuleId, _: u64, _: bool) -> AppResult<RuleViewModel> {
        unused()
    }
    async fn import(&self) -> AppResult<OperationResultViewModel> {
        unused()
    }
    async fn export(&self) -> AppResult<OperationResultViewModel> {
        unused()
    }
}

#[async_trait]
impl FaultServicePort for FakePorts {
    async fn templates(&self) -> AppResult<Vec<FaultTemplateViewModel>> {
        unused()
    }
    async fn configure(&self, _: FaultConfigurationDraft) -> AppResult<ActiveFaultViewModel> {
        unused()
    }
    async fn active(&self) -> AppResult<Vec<ActiveFaultViewModel>> {
        unused()
    }
    async fn stop(&self, _: RuleId, _: u64) -> AppResult<ActiveFaultViewModel> {
        unused()
    }
}

#[async_trait]
impl CertificateServicePort for FakePorts {
    async fn status(&self) -> AppResult<CertificateOverviewViewModel> {
        self.certificate_status_calls.fetch_add(1, Ordering::SeqCst);
        Ok(self.certificate_overview.lock().clone())
    }

    async fn overview(&self) -> AppResult<CertificateOverviewViewModel> {
        self.certificate_overview_calls
            .fetch_add(1, Ordering::SeqCst);
        Ok(self.certificate_overview.lock().clone())
    }
    async fn generate_ca(&self, _: Vec<String>) -> AppResult<CertificateOverviewViewModel> {
        unused()
    }
    async fn export_ca(&self) -> AppResult<OperationResultViewModel> {
        unused()
    }
    async fn reissue_leaf(
        &self,
        _: u64,
        _: Vec<String>,
    ) -> AppResult<CertificateOverviewViewModel> {
        unused()
    }
    async fn import_pkcs12(&self, _: String) -> AppResult<CertificateOverviewViewModel> {
        self.certificate_import_calls.fetch_add(1, Ordering::SeqCst);
        Ok(fake_certificate_overview())
    }
    async fn import_upstream_ca(&self) -> AppResult<CertificateOverviewViewModel> {
        unused()
    }
    async fn validate(&self) -> AppResult<CertificateValidationViewModel> {
        Ok(FieldValidationViewModel {
            valid: true,
            field_errors: BTreeMap::new(),
            warnings: Vec::new(),
        })
    }
    async fn reset_ca(&self, _: u64) -> AppResult<CertificateOverviewViewModel> {
        unused()
    }
}

#[async_trait]
impl SettingsRepositoryPort for FakePorts {
    async fn defaults(&self) -> AppResult<SettingsDraft> {
        Ok(valid_settings_draft())
    }

    async fn get(&self) -> AppResult<SettingsViewModel> {
        Ok(self.settings.lock().clone())
    }
    async fn validate(&self, _: &SettingsDraft) -> AppResult<SettingsValidationViewModel> {
        self.settings_validations.fetch_add(1, Ordering::SeqCst);
        Ok(FieldValidationViewModel {
            valid: true,
            field_errors: BTreeMap::new(),
            warnings: Vec::new(),
        })
    }
    async fn save(&self, mut draft: SettingsDraft) -> AppResult<SettingsViewModel> {
        self.settings_save_calls.fetch_add(1, Ordering::SeqCst);
        let mut settings = self.settings.lock();
        settings.revision = settings.revision.saturating_add(1);
        draft.expected_revision = Some(settings.revision);
        settings.stored = draft;
        settings.pending_changes = settings
            .effective
            .as_ref()
            .is_some_and(|effective| effective != &settings.stored);
        settings.requires_restart = settings.pending_changes;
        Ok(settings.clone())
    }
    async fn restore(&self, settings: SettingsViewModel) -> AppResult<SettingsViewModel> {
        *self.settings.lock() = settings.clone();
        Ok(settings)
    }
    async fn apply_effective(&self, effective: SettingsDraft) -> AppResult<SettingsViewModel> {
        let mut settings = self.settings.lock();
        settings.effective = Some(effective);
        settings.pending_changes = false;
        settings.requires_restart = false;
        Ok(settings.clone())
    }
    async fn clear_effective(&self) -> AppResult<SettingsViewModel> {
        let mut settings = self.settings.lock();
        settings.effective = None;
        settings.pending_changes = false;
        settings.requires_restart = false;
        Ok(settings.clone())
    }
}

#[async_trait]
impl FileExportPort for FakePorts {
    async fn export_session(
        &self,
        _: SessionDetailViewModel,
        _: bool,
    ) -> AppResult<OperationResultViewModel> {
        unused()
    }
}

fn proxy_status(state: ProxyState) -> ProxyStatusViewModel {
    let (state_text, ui_tone) = state.display_zh();
    ProxyStatusViewModel {
        state,
        state_text: state_text.into(),
        ui_tone,
        runtime_epoch: (state == ProxyState::Running).then(|| Uuid::from_u128(20)),
        revision: 1,
        channels: Vec::new(),
        app_to_proxy_health: ConnectionHealthViewModel {
            state: ConnectionHealthState::Unavailable,
            state_text: "未监听".into(),
            detail: "测试状态".into(),
            ui_tone: UiTone::Neutral,
        },
        proxy_to_server_health: ConnectionHealthViewModel {
            state: ConnectionHealthState::Unavailable,
            state_text: "尚未连接".into(),
            detail: "测试状态".into(),
            ui_tone: UiTone::Neutral,
        },
        active_sessions: 0,
        pending_breakpoints: 0,
        logical_memory_bytes: 0,
        logical_memory_text: "0 B".into(),
        memory_capacity_bytes: 256 * 1024 * 1024,
        memory_capacity_text: "256.0 MiB".into(),
        memory_usage_percent: 0,
        session_capacity: 500,
        default_timeout_seconds: 70,
        can_start: state == ProxyState::Stopped,
        start_disabled_reason: None,
        can_stop: state == ProxyState::Running,
        stop_disabled_reason: None,
        can_restart: false,
        restart_disabled_reason: None,
        fault_reason: None,
    }
}

fn application_with_fake_ports(ports: Arc<FakePorts>) -> Application {
    application_with_workspace_ports(
        ports,
        Arc::new(InMemoryWorkspaceStore::default()),
        Arc::new(InMemoryWorkspaceDocumentStore::default()),
    )
}

fn application_with_workspace_ports(
    ports: Arc<FakePorts>,
    workspaces: Arc<InMemoryWorkspaceStore>,
    workspace_documents: Arc<InMemoryWorkspaceDocumentStore>,
) -> Application {
    Application::new(
        "Test Product".into(),
        ApplicationDependencies {
            proxy: ports.clone(),
            capture: ports.clone(),
            sessions: ports.clone(),
            breakpoints: Arc::new(BreakpointCoordinator::default()),
            breakpoint_validation: ports.clone(),
            rules: ports.clone(),
            faults: ports.clone(),
            certificates: ports.clone(),
            settings: ports.clone(),
            listener_certificates: ports.clone(),
            file_export: ports,
            workspaces,
            workspace_documents,
            listener_runtime: Arc::new(InMemoryListenerRuntime::default()),
            events: Arc::new(EventHub::default()),
        },
    )
}

#[derive(Debug)]
struct RunningAndroidControl {
    status: AndroidNetworkStatusViewModel,
    observed_activation: parking_lot::Mutex<Option<AndroidNetworkActivation>>,
    runtime_ready: AtomicBool,
    network_apply_calls: AtomicUsize,
    block_start: AtomicBool,
    start_entered: tokio::sync::Notify,
    start_release: tokio::sync::Notify,
}

#[async_trait]
impl AndroidControlPort for RunningAndroidControl {
    async fn adb_get(&self) -> AppResult<AndroidAdbViewModel> {
        unused()
    }
    async fn adb_select(&self, _: String) -> AppResult<AndroidAdbViewModel> {
        unused()
    }
    async fn device_list(&self) -> AppResult<Vec<AndroidDeviceViewModel>> {
        unused()
    }
    async fn package_list(&self) -> AppResult<Vec<AndroidPackageViewModel>> {
        Ok(vec![AndroidPackageViewModel {
            package_name: "example.target".into(),
            uid: 10_001,
            shared_uid: None,
        }])
    }
    async fn package_get(&self, _: String) -> AppResult<AndroidPackageViewModel> {
        unused()
    }
    async fn companion_install(&self, _: bool) -> AppResult<AndroidCompanionInstallViewModel> {
        unused()
    }
    async fn vpn_open_consent(&self) -> AppResult<AndroidNetworkStatusViewModel> {
        unused()
    }
    async fn network_start(
        &self,
        _: AndroidNetworkActivation,
    ) -> AppResult<AndroidNetworkStatusViewModel> {
        self.start_entered.notify_one();
        if self.block_start.load(Ordering::SeqCst) {
            self.start_release.notified().await;
        }
        Ok(self.status.clone())
    }
    async fn network_apply(
        &self,
        _: AndroidNetworkActivation,
    ) -> AppResult<AndroidNetworkStatusViewModel> {
        self.network_apply_calls.fetch_add(1, Ordering::SeqCst);
        Ok(self.status.clone())
    }
    async fn network_runtime_ready(
        &self,
        activation: &AndroidNetworkActivation,
        _: &AndroidNetworkStatusViewModel,
    ) -> AppResult<bool> {
        *self.observed_activation.lock() = Some(activation.clone());
        Ok(self.runtime_ready.load(Ordering::SeqCst))
    }
    async fn network_stop(&self) -> AppResult<AndroidNetworkStatusViewModel> {
        unused()
    }
    async fn emergency_restore(&self) -> AppResult<AndroidNetworkStatusViewModel> {
        unused()
    }
    async fn network_status(&self) -> AppResult<AndroidNetworkStatusViewModel> {
        Ok(self.status.clone())
    }
}

#[tokio::test]
async fn bootstrap_uses_non_secret_certificate_status_instead_of_full_overview() {
    let ports = Arc::new(FakePorts::default());
    let application = application_with_fake_ports(ports.clone());

    application
        .app_bootstrap()
        .await
        .expect("bootstrap snapshot");

    assert_eq!(ports.certificate_status_calls.load(Ordering::SeqCst), 1);
    assert_eq!(ports.certificate_overview_calls.load(Ordering::SeqCst), 0);
}

#[tokio::test]
async fn workspace_facade_exposes_complete_headless_crud_document_and_event_flow() {
    let ports = Arc::new(FakePorts::default());
    let workspaces = Arc::new(InMemoryWorkspaceStore::new_empty());
    let documents = Arc::new(InMemoryWorkspaceDocumentStore::default());
    let application =
        application_with_workspace_ports(ports, Arc::clone(&workspaces), Arc::clone(&documents));
    let mut events = application.app_subscribe_events(0).unwrap();

    let created = application.workspace_create("Lab".into()).await.unwrap();
    assert_eq!(application.workspace_list().await.unwrap().len(), 1);
    assert_eq!(
        application.workspace_get(created.id).await.unwrap(),
        created
    );
    let created_event = events.live.recv().await.unwrap();
    assert!(matches!(
        created_event.payload,
        UiEventPayload::WorkspaceChanged(WorkspaceChangedViewModel {
            kind: WorkspaceChangeKind::Created,
            ..
        })
    ));

    let mut invalid = created.clone();
    invalid.name.clear();
    assert!(!application.workspace_validate(invalid).await.unwrap().valid);
    let mut edited = created.clone();
    edited.name = "Lab Updated".into();
    let saved = application.workspace_save(edited).await.unwrap();
    assert_eq!(saved.revision.get(), 2);
    let copied = application.workspace_copy(saved.id).await.unwrap();
    application.workspace_select(copied.id).await.unwrap();

    application.workspace_export(saved.id).await.unwrap();
    let (file_name, exported) = documents.take_last_export().unwrap();
    assert_eq!(file_name, "Lab_Updated.intercept-workspace");
    documents.set_next_import(exported);
    let imported = application.workspace_import().await.unwrap();
    assert!(imported.success);
    assert_eq!(application.workspace_list().await.unwrap().len(), 3);

    application
        .workspace_delete(copied.id, copied.revision.get())
        .await
        .unwrap();
    assert!(application.workspace_get(copied.id).await.is_err());
}

struct RunningVpnFixture {
    application: Application,
    android: Arc<RunningAndroidControl>,
    workspaces: Arc<InMemoryWorkspaceStore>,
    original_id: WorkspaceId,
    profile_id: String,
    listener_id: ListenerId,
}

async fn running_vpn_fixture() -> RunningVpnFixture {
    let ports = Arc::new(FakePorts::default());
    let workspaces = Arc::new(InMemoryWorkspaceStore::default());
    let original_id = workspaces
        .list()
        .await
        .unwrap()
        .into_iter()
        .find(|summary| summary.selected)
        .expect("selected default workspace")
        .id;
    let mut original = workspaces
        .get(original_id)
        .await
        .expect("default workspace");
    let listener_id = original.listeners[0].id;
    original.listeners[0].enabled = true;
    original.listeners[0].port = 41_273;
    let profile_id = Uuid::new_v4().to_string();
    original
        .android_network_profiles
        .push(AndroidNetworkProfile {
            id: profile_id.clone(),
            name: "运行中的方案".into(),
            target_applications: vec![AndroidTargetApplication {
                package_name: "example.target".into(),
                uid: 10_001,
                display_name: Some("测试应用".into()),
            }],
            destination_targets: Vec::new(),
            proxy_routes: vec![intercept_proxy_domain::AndroidProxyRoute {
                destination: "service.example.test".into(),
                ports: vec![41_273],
                listener_id,
            }],
            confirmed_shared_uids: BTreeSet::default(),
            auto_resume_after_reboot: false,
            weak_network: WeakNetworkProfile::default(),
        });
    workspaces.save(original.clone()).await.unwrap();
    let other = workspaces.create("其他 Workspace".into()).await.unwrap();
    workspaces.select(other.id).await.unwrap();

    let android = Arc::new(RunningAndroidControl {
        status: AndroidNetworkStatusViewModel {
            serial: "device-1".into(),
            state: AndroidNetworkState::Running,
            state_text: "运行中".into(),
            ui_tone: UiTone::Positive,
            verified: true,
            transport: AndroidControlTransport::LocalAbstractSocket,
            active_profile_id: Some(profile_id.clone()),
            active_profile_fingerprint: Some("profile-fingerprint".into()),
            active_route_fingerprint: Some("route-fingerprint".into()),
            active_route_count: 1,
            companion_process_running: Some(true),
            message: "运行中".into(),
            unsupported_fields: Vec::new(),
            stats: None,
        },
        observed_activation: parking_lot::Mutex::new(None),
        runtime_ready: AtomicBool::new(true),
        network_apply_calls: AtomicUsize::new(0),
        block_start: AtomicBool::new(false),
        start_entered: tokio::sync::Notify::new(),
        start_release: tokio::sync::Notify::new(),
    });
    let application = Application::new_with_android(
        "Test Product".into(),
        ApplicationDependencies {
            proxy: ports.clone(),
            capture: ports.clone(),
            sessions: ports.clone(),
            breakpoints: Arc::new(BreakpointCoordinator::default()),
            breakpoint_validation: ports.clone(),
            rules: ports.clone(),
            faults: ports.clone(),
            certificates: ports.clone(),
            settings: ports.clone(),
            listener_certificates: ports.clone(),
            file_export: ports,
            workspaces: workspaces.clone(),
            workspace_documents: Arc::new(InMemoryWorkspaceDocumentStore::default()),
            listener_runtime: Arc::new(InMemoryListenerRuntime::default()),
            events: Arc::new(EventHub::default()),
        },
        android.clone(),
    );

    RunningVpnFixture {
        application,
        android,
        workspaces,
        original_id,
        profile_id,
        listener_id,
    }
}

#[tokio::test]
async fn workspace_switch_keeps_running_vpn_bound_to_its_original_workspace() {
    let fixture = running_vpn_fixture().await;

    let status = fixture.application.device_network_status().await.unwrap();

    assert_eq!(
        status.active_profile_id.as_deref(),
        Some(fixture.profile_id.as_str())
    );
    let activation = fixture
        .android
        .observed_activation
        .lock()
        .clone()
        .expect("runtime readiness uses the active profile");
    assert_eq!(activation.profile.id, fixture.profile_id);
    assert_eq!(
        activation.proxy_routes[0].listener_id,
        fixture.listener_id.to_string()
    );
    assert_eq!(activation.proxy_routes[0].desktop_listener_port, 41_273);
}

#[tokio::test]
async fn device_network_status_does_not_reapply_a_stale_runtime() {
    let fixture = running_vpn_fixture().await;

    fixture.android.runtime_ready.store(false, Ordering::SeqCst);
    let degraded = fixture.application.device_network_status().await.unwrap();
    assert_eq!(degraded.state, AndroidNetworkState::Faulted);
    assert!(degraded.message.contains("应用修改"));
    assert_eq!(
        fixture.android.network_apply_calls.load(Ordering::SeqCst),
        0
    );
}

#[tokio::test]
async fn workspace_and_device_selection_wait_for_device_network_start_to_finish() {
    let fixture = running_vpn_fixture().await;
    fixture
        .workspaces
        .select(fixture.original_id)
        .await
        .unwrap();
    fixture.android.block_start.store(true, Ordering::SeqCst);
    let application = Arc::new(fixture.application);

    let start_application = Arc::clone(&application);
    let profile_id = fixture.profile_id.clone();
    let start = tokio::spawn(async move {
        start_application
            .device_network_start(profile_id, false)
            .await
    });
    fixture.android.start_entered.notified().await;

    let other_workspace = fixture
        .workspaces
        .list()
        .await
        .unwrap()
        .into_iter()
        .find(|summary| summary.id != fixture.original_id)
        .unwrap();
    let select_application = Arc::clone(&application);
    let mut select = tokio::spawn(async move {
        select_application
            .workspace_select(other_workspace.id)
            .await
    });
    assert!(
        tokio::time::timeout(std::time::Duration::from_millis(50), &mut select)
            .await
            .is_err(),
        "Workspace 切换必须等待设备网络启动释放统一变更锁"
    );
    let adb_application = Arc::clone(&application);
    let mut adb_select =
        tokio::spawn(async move { adb_application.android_adb_select("device-2".into()).await });
    assert!(
        tokio::time::timeout(std::time::Duration::from_millis(50), &mut adb_select)
            .await
            .is_err(),
        "设备切换必须等待设备网络启动释放统一变更锁"
    );

    fixture.android.start_release.notify_one();
    start.await.unwrap().unwrap();
    select.await.unwrap().unwrap();
    assert!(adb_select.await.unwrap().is_err());
}

#[tokio::test]
async fn profile_delete_checks_runtime_only_after_inflight_start_finishes() {
    let fixture = running_vpn_fixture().await;
    fixture
        .workspaces
        .select(fixture.original_id)
        .await
        .unwrap();
    fixture.android.block_start.store(true, Ordering::SeqCst);
    let application = Arc::new(fixture.application);

    let start_application = Arc::clone(&application);
    let profile_id = fixture.profile_id.clone();
    let start = tokio::spawn(async move {
        start_application
            .device_network_start(profile_id, false)
            .await
    });
    fixture.android.start_entered.notified().await;

    let delete_application = Arc::clone(&application);
    let profile_id = fixture.profile_id.clone();
    let mut delete = tokio::spawn(async move {
        delete_application
            .device_network_profile_delete(profile_id)
            .await
    });
    assert!(
        tokio::time::timeout(std::time::Duration::from_millis(50), &mut delete)
            .await
            .is_err(),
        "方案删除必须等待设备网络启动结束后再检查运行状态"
    );

    fixture.android.start_release.notify_one();
    start.await.unwrap().unwrap();
    let error = delete.await.unwrap().expect_err("活动方案不能被删除");
    assert_eq!(error.view_model.code, "ANDROID_PROFILE_ACTIVE");
}

#[tokio::test]
async fn workspace_delete_rejects_an_active_android_network_profile() {
    let fixture = running_vpn_fixture().await;

    let current = fixture.workspaces.get(fixture.original_id).await.unwrap();
    let delete_error = fixture
        .application
        .workspace_delete(current.id, current.revision.get())
        .await
        .expect_err("活动 VPN 方案所属 Workspace 不能删除");
    assert_eq!(
        delete_error.view_model.code,
        "WORKSPACE_ANDROID_NETWORK_ACTIVE"
    );
}

#[tokio::test]
async fn profile_delete_rejects_the_active_android_network_profile() {
    let fixture = running_vpn_fixture().await;
    fixture
        .workspaces
        .select(fixture.original_id)
        .await
        .unwrap();

    let error = fixture
        .application
        .device_network_profile_delete(fixture.profile_id.clone())
        .await
        .expect_err("活动设备网络方案不能删除");

    assert_eq!(error.view_model.code, "ANDROID_PROFILE_ACTIVE");
    assert!(
        fixture
            .application
            .device_network_profile_get(fixture.profile_id)
            .await
            .is_ok()
    );
}

#[tokio::test]
async fn workspace_rejects_arbitrary_certificate_path_reference_creation() {
    let ports = Arc::new(FakePorts::default());
    let application = application_with_fake_ports(ports);
    let workspace = application.workspace_create("Lab".into()).await.unwrap();

    let error = application
        .workspace_component_new(workspace, "certificate_reference")
        .expect_err("certificate references must be created by a managed import");

    assert_eq!(
        error.view_model.code,
        "WORKSPACE_CERTIFICATE_IMPORT_REQUIRED"
    );
}

#[tokio::test]
async fn listener_save_rejects_unmanaged_file_and_pkcs12_references() {
    let ports = Arc::new(FakePorts::default());
    let application = application_with_fake_ports(ports);
    let workspace = application.workspace_create("Lab".into()).await.unwrap();
    let listener = workspace.listeners[0].clone();

    for value in [
        "file:/tmp/untrusted-server.pem",
        "pkcs12:/tmp/untrusted-client.p12?password_env=PASSWORD",
    ] {
        let reference = CertificateReference {
            id: CertificateReferenceId::new(),
            label: "不得持久化的外部引用".into(),
            kind: CertificateReferenceKind::UpstreamClientIdentity,
            reference: value.into(),
        };
        let error = application
            .listener_save(
                workspace.id,
                workspace.revision.get(),
                listener.clone(),
                vec![reference],
            )
            .await
            .expect_err("listener save must reject certificate paths from IPC");

        assert_eq!(
            error.view_model.code,
            "LISTENER_CERTIFICATE_REFERENCE_UNTRUSTED"
        );
    }
}

#[tokio::test]
async fn listener_certificate_discard_rejects_references_used_by_any_workspace() {
    let ports = Arc::new(FakePorts::default());
    let application = application_with_fake_ports(ports.clone());
    let workspace = application.workspace_create("Lab".into()).await.unwrap();
    let mut listener = workspace.listeners[0].clone();
    let reference = CertificateReference {
        id: CertificateReferenceId::new(),
        label: "正在使用的上游身份".into(),
        kind: CertificateReferenceKind::UpstreamClientIdentity,
        reference: "managed:listener-tls:in-use".into(),
    };
    listener.fixed_server = Some(FixedServerSettings {
        upstream_url: "https://upstream.example.test:443".into(),
        upstream_tls: UpstreamTlsSettings {
            client_identity: Some(reference.id),
            ..UpstreamTlsSettings::default()
        },
    });
    application
        .listener_save(
            workspace.id,
            workspace.revision.get(),
            listener,
            vec![reference.clone()],
        )
        .await
        .unwrap();

    let error = application
        .listener_certificate_discard(reference)
        .await
        .expect_err("a persisted reference must never be discarded");

    assert_eq!(error.view_model.code, "CERTIFICATE_REFERENCE_IN_USE");
    assert_eq!(ports.certificate_discard_calls.load(Ordering::SeqCst), 0);
}

#[tokio::test]
async fn listener_certificate_discard_cleans_an_unreferenced_import() {
    let ports = Arc::new(FakePorts::default());
    let application = application_with_fake_ports(ports.clone());
    application.workspace_create("Lab".into()).await.unwrap();
    let reference = CertificateReference {
        id: CertificateReferenceId::new(),
        label: "已放弃的上游身份".into(),
        kind: CertificateReferenceKind::UpstreamClientIdentity,
        reference: "managed:listener-tls:abandoned".into(),
    };

    let result = application
        .listener_certificate_discard(reference)
        .await
        .unwrap();

    assert!(result.success);
    assert_eq!(ports.certificate_discard_calls.load(Ordering::SeqCst), 1);
}

#[tokio::test]
async fn listener_certificate_discard_is_atomic_with_listener_save() {
    let ports = Arc::new(FakePorts::default());
    ports
        .block_certificate_discard
        .store(true, Ordering::SeqCst);
    let application = Arc::new(application_with_fake_ports(ports.clone()));
    let workspace = application.workspace_create("Lab".into()).await.unwrap();
    let mut listener = workspace.listeners[0].clone();
    let reference = CertificateReference {
        id: CertificateReferenceId::new(),
        label: "并发保存的上游身份".into(),
        kind: CertificateReferenceKind::UpstreamClientIdentity,
        reference: "managed:listener-tls:concurrent-save".into(),
    };
    listener.fixed_server = Some(FixedServerSettings {
        upstream_url: "https://upstream.example.test:443".into(),
        upstream_tls: UpstreamTlsSettings {
            client_identity: Some(reference.id),
            ..UpstreamTlsSettings::default()
        },
    });

    let discard = {
        let application = application.clone();
        let reference = reference.clone();
        tokio::spawn(async move { application.listener_certificate_discard(reference).await })
    };
    ports.certificate_discard_entered.notified().await;

    let mut save = {
        let application = application.clone();
        let reference = reference.clone();
        tokio::spawn(async move {
            application
                .listener_save(
                    workspace.id,
                    workspace.revision.get(),
                    listener,
                    vec![reference],
                )
                .await
        })
    };

    assert!(
        tokio::time::timeout(Duration::from_millis(20), &mut save)
            .await
            .is_err(),
        "listener save must wait until reference check and secret deletion finish"
    );

    ports.continue_certificate_discard.notify_one();
    discard.await.unwrap().unwrap();
    let error = save
        .await
        .unwrap()
        .expect_err("saving a discarded managed reference must fail inspection");

    assert_eq!(
        error.view_model.code,
        "LISTENER_CERTIFICATE_MATERIAL_UNAVAILABLE"
    );
    assert!(
        application
            .workspace_get(workspace.id)
            .await
            .unwrap()
            .certificate_references
            .is_empty()
    );
}

#[tokio::test]
async fn listener_tls_test_rejects_unmanaged_file_and_pkcs12_references() {
    let ports = Arc::new(FakePorts::default());
    let application = application_with_fake_ports(ports);
    let workspace = application.workspace_create("Lab".into()).await.unwrap();
    let mut listener = workspace.listeners[0].clone();
    listener.fixed_server = Some(FixedServerSettings {
        upstream_url: "https://upstream.example.test:443".into(),
        upstream_tls: UpstreamTlsSettings::default(),
    });

    for value in [
        "file:/tmp/untrusted-server.pem",
        "pkcs12:/tmp/untrusted-client.p12?password_env=PASSWORD",
    ] {
        let reference = CertificateReference {
            id: CertificateReferenceId::new(),
            label: "不得用于握手测试的外部引用".into(),
            kind: CertificateReferenceKind::UpstreamClientIdentity,
            reference: value.into(),
        };
        let error = application
            .listener_test_upstream_tls(
                workspace.id,
                workspace.revision.get(),
                listener.clone(),
                vec![reference],
            )
            .await
            .expect_err("TLS test must reject certificate paths from IPC");

        assert_eq!(
            error.view_model.code,
            "LISTENER_CERTIFICATE_REFERENCE_UNTRUSTED"
        );
    }
}

#[tokio::test]
async fn listener_tls_test_validates_the_persisted_workspace_candidate() {
    let ports = Arc::new(FakePorts::default());
    let application = application_with_fake_ports(ports);
    let workspace = application.workspace_create("Lab".into()).await.unwrap();
    let mut listener = workspace.listeners[0].clone();
    listener.fixed_server = Some(FixedServerSettings {
        upstream_url: "https://".into(),
        upstream_tls: UpstreamTlsSettings::default(),
    });

    let error = application
        .listener_test_upstream_tls(workspace.id, workspace.revision.get(), listener, Vec::new())
        .await
        .expect_err("invalid listener drafts must fail before the runtime handshake");

    assert!(
        !error.view_model.field_errors.is_empty(),
        "domain validation must expose the invalid fixed Server URL"
    );
}

#[tokio::test]
async fn listener_validation_uses_persisted_other_listeners_not_unrelated_ui_drafts() {
    let ports = Arc::new(FakePorts::default());
    let application = application_with_fake_ports(ports);
    let workspace = application.workspace_create("Lab".into()).await.unwrap();
    let mut listener = workspace.listeners[0].clone();
    listener.name = "当前有效监听草稿".into();

    let validation = application
        .listener_validate(
            workspace.id,
            workspace.revision.get(),
            listener.clone(),
            workspace.certificate_references.clone(),
        )
        .await
        .expect("target listener validation should be available independently");

    assert!(validation.valid);
    assert_eq!(validation.normalized.listeners.len(), 1);
    assert_eq!(validation.normalized.listeners[0], listener);
}

#[tokio::test]
async fn running_workspace_listener_blocks_configuration_save_and_delete() {
    let ports = Arc::new(FakePorts::default());
    let workspaces = Arc::new(InMemoryWorkspaceStore::new_empty());
    let listener_runtime = Arc::new(InMemoryListenerRuntime::default());
    let application = Application::new(
        "Test Product".into(),
        ApplicationDependencies {
            proxy: ports.clone(),
            capture: ports.clone(),
            sessions: ports.clone(),
            breakpoints: Arc::new(BreakpointCoordinator::default()),
            breakpoint_validation: ports.clone(),
            rules: ports.clone(),
            faults: ports.clone(),
            certificates: ports.clone(),
            settings: ports.clone(),
            listener_certificates: ports.clone(),
            file_export: ports,
            workspaces,
            workspace_documents: Arc::new(InMemoryWorkspaceDocumentStore::default()),
            listener_runtime: listener_runtime.clone(),
            events: Arc::new(EventHub::default()),
        },
    );
    let workspace = application.workspace_create("Live".into()).await.unwrap();
    let listener = workspace.listeners[0].clone();
    listener_runtime
        .start(workspace.clone(), listener.clone())
        .await
        .unwrap();

    let mut edited = workspace.clone();
    edited.listeners[0].port = edited.listeners[0].port.saturating_add(1);
    let save_error = application
        .workspace_save(edited)
        .await
        .expect_err("live workspace save rejected");
    assert_eq!(save_error.view_model.code, "WORKSPACE_RUNTIME_ACTIVE");
    let delete_error = application
        .workspace_delete(workspace.id, workspace.revision.get())
        .await
        .expect_err("live workspace delete rejected");
    assert_eq!(delete_error.view_model.code, "WORKSPACE_RUNTIME_ACTIVE");
    let listener_save_error = application
        .listener_save(
            workspace.id,
            workspace.revision.get(),
            listener.clone(),
            workspace.certificate_references.clone(),
        )
        .await
        .expect_err("live listener save rejected");
    assert_eq!(
        listener_save_error.view_model.code,
        "LISTENER_RUNTIME_ACTIVE"
    );
    let listener_delete_error = application
        .listener_delete(workspace.id, workspace.revision.get(), listener.id())
        .await
        .expect_err("live listener delete rejected");
    assert_eq!(
        listener_delete_error.view_model.code,
        "LISTENER_RUNTIME_ACTIVE"
    );
}

#[tokio::test]
async fn running_workspace_listener_allows_saving_and_starting_a_stopped_listener() {
    let ports = Arc::new(FakePorts::default());
    let workspaces = Arc::new(InMemoryWorkspaceStore::new_empty());
    let listener_runtime = Arc::new(InMemoryListenerRuntime::default());
    let application = Application::new(
        "Test Product".into(),
        ApplicationDependencies {
            proxy: ports.clone(),
            capture: ports.clone(),
            sessions: ports.clone(),
            breakpoints: Arc::new(BreakpointCoordinator::default()),
            breakpoint_validation: ports.clone(),
            rules: ports.clone(),
            faults: ports.clone(),
            certificates: ports.clone(),
            settings: ports.clone(),
            listener_certificates: ports.clone(),
            file_export: ports,
            workspaces,
            workspace_documents: Arc::new(InMemoryWorkspaceDocumentStore::default()),
            listener_runtime: listener_runtime.clone(),
            events: Arc::new(EventHub::default()),
        },
    );
    let mut workspace = application.workspace_create("Live".into()).await.unwrap();
    let running_listener = workspace.listeners[0].clone();
    let mut stopped_listener = application.listener_copy(running_listener.clone()).unwrap();
    stopped_listener.name = "Second listener".into();
    // 使用与产品无关的测试端口偏移，避免回归测试重新引入已删除的业务端口契约。
    stopped_listener.port = running_listener.port.saturating_add(173);
    workspace.listeners.push(stopped_listener.clone());
    workspace = application.workspace_save(workspace).await.unwrap();

    application
        .listener_start(
            workspace.id,
            workspace.revision.get(),
            running_listener.id(),
        )
        .await
        .unwrap();
    workspace = application.workspace_get(workspace.id).await.unwrap();

    workspace.listeners[1].name = "Second listener edited while first runs".into();
    let edited_listener = workspace.listeners[1].clone();
    let saved = application
        .listener_save(
            workspace.id,
            workspace.revision.get(),
            edited_listener,
            workspace.certificate_references.clone(),
        )
        .await
        .expect("stopped listener can be saved while another listener runs");
    let second_status = application
        .listener_start(saved.id, saved.revision.get(), stopped_listener.id())
        .await
        .expect("saved stopped listener can start while first listener remains running");

    assert_eq!(second_status.state, ListenerRuntimeState::Running);
    let statuses = application.listener_statuses().await.unwrap();
    assert_eq!(statuses.len(), 2);

    application
        .listener_stop(saved.id, saved.revision.get(), running_listener.id())
        .await
        .expect("stale aggregate revision must not block stopping a listener");
    let statuses = application.listener_statuses().await.unwrap();
    assert_eq!(statuses.len(), 1);
    assert_eq!(statuses[0].listener_id, stopped_listener.id());

    let latest = application.workspace_get(saved.id).await.unwrap();
    application
        .listener_delete(latest.id, latest.revision.get(), running_listener.id())
        .await
        .expect("a stopped listener can be deleted while another listener remains running");
    let latest = application.workspace_get(saved.id).await.unwrap();
    assert_eq!(latest.listeners.len(), 1);
    assert_eq!(latest.listeners[0].id, stopped_listener.id());
}

#[tokio::test]
async fn running_workspace_listener_allows_device_network_profile_persistence() {
    let ports = Arc::new(FakePorts::default());
    let workspaces = Arc::new(InMemoryWorkspaceStore::new_empty());
    let listener_runtime = Arc::new(InMemoryListenerRuntime::default());
    let application = Application::new(
        "Test Product".into(),
        ApplicationDependencies {
            proxy: ports.clone(),
            capture: ports.clone(),
            sessions: ports.clone(),
            breakpoints: Arc::new(BreakpointCoordinator::default()),
            breakpoint_validation: ports.clone(),
            rules: ports.clone(),
            faults: ports.clone(),
            certificates: ports.clone(),
            settings: ports.clone(),
            listener_certificates: ports.clone(),
            file_export: ports,
            workspaces,
            workspace_documents: Arc::new(InMemoryWorkspaceDocumentStore::default()),
            listener_runtime: listener_runtime.clone(),
            events: Arc::new(EventHub::default()),
        },
    );
    let mut workspace = application.workspace_create("Live".into()).await.unwrap();
    let listener = workspace.listeners[0].clone();
    listener_runtime
        .start(workspace.clone(), listener.clone())
        .await
        .unwrap();

    workspace
        .android_network_profiles
        .push(AndroidNetworkProfile {
            id: "vpn-profile".into(),
            name: "运行时可保存的设备网络方案".into(),
            target_applications: vec![AndroidTargetApplication {
                package_name: "com.example.client".into(),
                display_name: None,
                uid: 10_001,
            }],
            destination_targets: Vec::new(),
            proxy_routes: Vec::new(),
            confirmed_shared_uids: BTreeSet::default(),
            auto_resume_after_reboot: false,
            weak_network: WeakNetworkProfile::default(),
        });

    let saved = application
        .workspace_save(workspace)
        .await
        .expect("VPN profile persistence must remain available while a listener runs");
    assert_eq!(saved.android_network_profiles.len(), 1);
    assert_eq!(
        listener_runtime.statuses().await.unwrap()[0].state,
        ListenerRuntimeState::Running,
        "saving an independent VPN profile must not restart the listener"
    );
}

#[tokio::test]
async fn breakpoint_resolve_normalizes_modified_json_inside_rust_use_case() {
    let ports = Arc::new(FakePorts::default());
    *ports.proxy_state.lock() = ProxyState::Running;
    let coordinator = Arc::new(BreakpointCoordinator::default());
    let epoch = Uuid::from_u128(20);
    let id = Uuid::from_u128(30);
    let ticket = coordinator
        .register(breakpoint(id, epoch, 1))
        .expect("register");
    let application = Application::new(
        "Test Product".into(),
        ApplicationDependencies {
            proxy: ports.clone(),
            capture: ports.clone(),
            sessions: ports.clone(),
            breakpoints: coordinator,
            breakpoint_validation: Arc::new(breakpoint_validator()),
            rules: ports.clone(),
            faults: ports.clone(),
            certificates: ports.clone(),
            settings: ports.clone(),
            listener_certificates: ports.clone(),
            file_export: ports,
            workspaces: Arc::new(InMemoryWorkspaceStore::default()),
            workspace_documents: Arc::new(InMemoryWorkspaceDocumentStore::default()),
            listener_runtime: Arc::new(InMemoryListenerRuntime::default()),
            events: Arc::new(EventHub::default()),
        },
    );

    application
        .breakpoint_resolve(
            epoch,
            BreakpointDecision {
                breakpoint_id: id,
                expected_revision: 7,
                kind: BreakpointDecisionKind::ForwardModified,
                message: Some(MessageContentViewModel {
                    http_status: None,
                    start_line_bytes: Vec::new(),
                    raw_headers: Vec::new(),
                    headers: BTreeMap::from([(
                        "content-type".into(),
                        vec!["application/json".into()],
                    )]),
                    body_text: Some(r#"{"amount":100}"#.into()),
                    body_bytes: b"stale".to_vec(),
                    json: None,
                    content_length: 5,
                }),
                delay_ms: Some(1_000),
                http_status: Some(503),
                content_length_delta: Some(1),
                truncate_at: Some(1),
            },
        )
        .await
        .expect("resolve");

    let BreakpointOutcome::Decision(decision) = ticket.outcome.await.expect("outcome") else {
        panic!("expected decision");
    };
    let message = decision.message.expect("normalized message");
    assert_eq!(
        message.body_text.as_deref(),
        Some("{\n  \"amount\": 100\n}")
    );
    assert_eq!(message.content_length, message.body_bytes.len());
    assert_eq!(
        message.headers.get("content-length"),
        Some(&vec![message.content_length.to_string()])
    );
}

// SETTINGS-001~012, TEST-SETTINGS, TEST-IPC: facade normalizes and validates before fake storage.
#[tokio::test]
async fn settings_use_case_accepts_safe_defaults_and_normalizes_before_port_validation() {
    let ports = Arc::new(FakePorts::default());
    let application = application_with_fake_ports(ports.clone());
    // 通用化以后，首次启动的系统设置必须可以独立保存；代理入口、上游地址和证书
    // 已经属于 Workspace Listener，不能再用旧 Payment 固定通道约束把默认值判无效。
    let safe_defaults = SettingsDraft::default();
    let validation = application
        .settings_validate(safe_defaults)
        .await
        .expect("validation result");
    assert!(validation.valid);
    assert_eq!(ports.settings_validations.load(Ordering::SeqCst), 1);

    let mut valid = valid_settings_draft();
    valid.channels[0].upstream_url = " https://alpha.example.test ".into();
    assert!(
        application
            .settings_validate(valid)
            .await
            .expect("fake validation result")
            .valid
    );
    assert_eq!(ports.settings_validations.load(Ordering::SeqCst), 2);
}

#[tokio::test]
async fn settings_san_raw_input_is_normalized_atomically_in_rust() {
    let ports = Arc::new(FakePorts::default());
    let application = application_with_fake_ports(ports.clone());
    let draft = ports.settings.lock().stored.clone();
    let mut events = application
        .app_subscribe_events(0)
        .expect("subscribe before save");

    let saved = application
        .settings_save_input(draft, " 127.0.0.1，127.0.0.1, ".into())
        .await
        .expect("save normalized settings");

    assert_eq!(saved.stored.leaf_sans, vec!["127.0.0.1"]);
    let event = events.live.recv().await.expect("settings changed event");
    assert_eq!(event.entity_id.as_deref(), Some("settings"));
    assert_eq!(event.entity_revision, Some(saved.revision));
    let UiEventPayload::SettingsChanged(changed) = event.payload else {
        panic!("expected SettingsChanged");
    };
    assert_eq!(changed.stored.leaf_sans, vec!["127.0.0.1"]);
}

#[tokio::test]
async fn settings_can_be_saved_without_listener_certificate_warnings() {
    let ports = Arc::new(FakePorts::default());
    {
        let mut overview = ports.certificate_overview.lock();
        overview.ready = false;
        overview.items.clear();
        overview.can_initialize = true;
        overview.status_text = "证书配置不完整".into();
        overview.ui_tone = UiTone::Warning;
    }
    let application = application_with_fake_ports(ports);
    let draft = SettingsDraft {
        leaf_sans: vec!["10.0.34.50".into()],
        ..valid_settings_draft()
    };

    let validation = application.settings_validate(draft).await.unwrap();

    assert!(validation.valid);
    // Listener 在各自的 Workspace 中校验 TLS 身份。系统设置不能重复推断证书状态，
    // 否则一个无关 Listener 尚未配置也会污染全局容量或超时设置的保存结果。
    assert!(validation.warnings.is_empty());
}

#[tokio::test]
async fn settings_validation_matches_prefixed_certificate_sans() {
    let ports = Arc::new(FakePorts::default());
    ports.certificate_overview.lock().items[0].sans =
        vec!["IP:10.0.34.50".into(), "DNS:Proxy.Local".into()];
    let application = application_with_fake_ports(ports);
    let draft = SettingsDraft {
        leaf_sans: vec!["10.0.34.50".into(), "proxy.local".into()],
        ..valid_settings_draft()
    };

    let validation = application.settings_validate(draft).await.unwrap();

    assert!(validation.valid);
    assert!(!validation.field_errors.contains_key("leaf_sans"));
}

#[tokio::test]
async fn empty_pkcs12_password_is_forwarded_to_the_rust_certificate_parser() {
    let ports = Arc::new(FakePorts::default());
    let application = application_with_fake_ports(ports);

    let overview = application.certificate_import_pkcs12(String::new()).await;

    assert!(overview.is_ok());
}

#[tokio::test]
async fn settings_restart_preserves_candidate_error_after_successful_rollback() {
    let ports = Arc::new(FakePorts::default());
    *ports.proxy_state.lock() = ProxyState::Running;
    ports.start_results.lock().extend([
        Err(AppError::new("PORT_IN_USE", "候选端口已被占用。")),
        Ok(proxy_status(ProxyState::Running)),
    ]);
    let original = ports.settings.lock().clone();
    let application = application_with_fake_ports(ports.clone());
    let mut candidate = original.stored.clone();
    candidate.channels[0].port = 20_003;

    let error = application
        .settings_save_and_restart(candidate)
        .await
        .expect_err("candidate must fail and roll back");

    assert_eq!(error.view_model.code, "PORT_IN_USE");
    assert!(error.view_model.message.contains("已恢复"));
    assert_eq!(ports.start_calls.load(Ordering::SeqCst), 2);
    assert_eq!(*ports.proxy_state.lock(), ProxyState::Running);
    let restored = ports.settings.lock();
    assert_eq!(restored.stored, original.stored);
    assert_eq!(restored.effective, original.effective);
}

#[tokio::test]
async fn starting_and_stopping_block_every_rule_and_fault_write() {
    for state in [ProxyState::Starting, ProxyState::Stopping] {
        let ports = Arc::new(FakePorts::default());
        *ports.proxy_state.lock() = state;
        let application = application_with_fake_ports(ports);
        let draft = application
            .rule_new_draft()
            .await
            .expect("draft is read-only");
        assert_eq!(
            application
                .rule_save(draft)
                .await
                .expect_err("rule save is gated")
                .view_model
                .code,
            "OPERATION_IN_PROGRESS"
        );
        assert_eq!(
            application
                .rule_toggle(Uuid::new_v4(), 1, false)
                .await
                .expect_err("rule toggle is gated")
                .view_model
                .code,
            "OPERATION_IN_PROGRESS"
        );
        assert_eq!(
            application
                .fault_configure(FaultConfigurationDraft {
                    template_id: "delay".into(),
                    existing_rule_id: None,
                    expected_revision: None,
                    channel: None,
                    terminal: None,
                    target: None,
                    nth_hit: None,
                    one_shot: false,
                    priority: 1,
                    parameters: BTreeMap::new(),
                })
                .await
                .expect_err("fault configure is gated")
                .view_model
                .code,
            "OPERATION_IN_PROGRESS"
        );
    }
}

#[tokio::test]
async fn lifecycle_mutations_serialize_settings_and_certificate_writes() {
    let ports = Arc::new(FakePorts::default());
    ports.block_start.store(true, Ordering::SeqCst);
    let application = Arc::new(application_with_fake_ports(ports.clone()));

    let starting = {
        let application = application.clone();
        tokio::spawn(async move { application.proxy_start().await })
    };
    ports.start_entered.notified().await;

    let draft = ports.settings.lock().stored.clone();
    let mut saving = {
        let application = application.clone();
        tokio::spawn(async move { application.settings_save(draft).await })
    };
    let mut importing = {
        let application = application.clone();
        tokio::spawn(async move { application.certificate_import_pkcs12(String::new()).await })
    };

    assert!(
        tokio::time::timeout(Duration::from_millis(20), &mut saving)
            .await
            .is_err(),
        "settings save must wait for the lifecycle mutation"
    );
    assert!(
        tokio::time::timeout(Duration::from_millis(20), &mut importing)
            .await
            .is_err(),
        "certificate import must wait for the lifecycle mutation"
    );

    ports.continue_start.notify_one();
    assert_eq!(
        starting.await.expect("start task").expect("start").state,
        ProxyState::Running
    );
    saving
        .await
        .expect("settings task")
        .expect("settings remain writable while running");
    let import_error = importing
        .await
        .expect("certificate task")
        .expect_err("certificate mutation requires a stopped proxy");

    assert_eq!(import_error.view_model.code, "OPERATION_IN_PROGRESS");
    assert_eq!(ports.settings_save_calls.load(Ordering::SeqCst), 1);
    assert_eq!(ports.certificate_import_calls.load(Ordering::SeqCst), 0);
}

#[tokio::test]
async fn application_shutdown_stops_runtime_clears_effective_settings_and_is_idempotent() {
    let ports = Arc::new(FakePorts::default());
    *ports.proxy_state.lock() = ProxyState::Running;
    let application = application_with_fake_ports(ports.clone());

    let stopped = application.app_shutdown().await.expect("shutdown");
    assert_eq!(stopped.state, ProxyState::Stopped);
    assert_eq!(ports.stop_calls.load(Ordering::SeqCst), 1);
    assert!(ports.settings.lock().effective.is_none());

    let stopped_again = application
        .app_shutdown()
        .await
        .expect("idempotent shutdown");
    assert_eq!(stopped_again.state, ProxyState::Stopped);
    assert_eq!(ports.stop_calls.load(Ordering::SeqCst), 1);
}

#[tokio::test]
async fn application_shutdown_stops_every_dynamic_workspace_listener() {
    let ports = Arc::new(FakePorts::default());
    let listener_runtime = Arc::new(InMemoryListenerRuntime::default());
    let workspace = ProxyWorkspace::default();
    let listener = workspace
        .listeners
        .clone()
        .into_iter()
        .next()
        .expect("default forward listener");
    listener_runtime
        .start(workspace, listener)
        .await
        .expect("listener starts before shutdown");
    let application = Application::new(
        "Test Product".into(),
        ApplicationDependencies {
            proxy: ports.clone(),
            capture: ports.clone(),
            sessions: ports.clone(),
            breakpoints: Arc::new(BreakpointCoordinator::default()),
            breakpoint_validation: ports.clone(),
            rules: ports.clone(),
            faults: ports.clone(),
            certificates: ports.clone(),
            settings: ports.clone(),
            listener_certificates: ports.clone(),
            file_export: ports,
            workspaces: Arc::new(InMemoryWorkspaceStore::default()),
            workspace_documents: Arc::new(InMemoryWorkspaceDocumentStore::default()),
            listener_runtime: listener_runtime.clone(),
            events: Arc::new(EventHub::default()),
        },
    );

    application.app_shutdown().await.expect("shutdown");

    assert!(
        listener_runtime.statuses().await.unwrap().is_empty(),
        "application shutdown must not leave dynamic listener tasks running"
    );
}

#[test]
fn rule_editor_primitives_and_byte_parser_are_owned_by_rust() {
    let application = application_with_fake_ports(Arc::new(FakePorts::default()));
    assert_eq!(
        application.rule_condition_draft(RuleConditionKind::NthHit),
        RuleCondition::NthHit { count: 1 }
    );
    assert!(matches!(
        application.rule_action_draft(RuleActionKind::MockResponse),
        RuleAction::Terminal {
            action: RuleTerminalAction::MockResponse { .. }
        }
    ));
    assert_eq!(
        application.rule_match_field_draft(RuleMatchFieldKind::JsonPath),
        RuleMatchField::JsonPath {
            path: "$.field".into()
        }
    );
    assert_eq!(
        application.rule_match_operator_draft(RuleMatchOperatorKind::Regex),
        RuleMatchOperator::Regex {
            pattern: String::new()
        }
    );
    assert_eq!(
        application
            .rule_parse_byte_input(" 123, 0,255 ")
            .expect("valid bytes"),
        RuleByteInputViewModel {
            bytes: vec![123, 0, 255],
            normalized: "123, 0, 255".into(),
        }
    );
    let error = application
        .rule_parse_byte_input("1, 256")
        .expect_err("out of range");
    assert_eq!(error.view_model.code, "RULE_INVALID");
    assert!(error.view_model.field_errors.contains_key("raw"));
    assert_eq!(
        application
            .rule_parse_header_input(" Content-Type: application/json \nX-Trace: abc:123 ")
            .expect("valid headers"),
        RuleHeaderInputViewModel {
            headers: vec![
                ("content-type".into(), "application/json".into()),
                ("x-trace".into(), "abc:123".into()),
            ],
            normalized: "content-type: application/json\nx-trace: abc:123".into(),
        }
    );
    let error = application
        .rule_parse_header_input("missing-separator")
        .expect_err("invalid header line");
    assert_eq!(error.view_model.code, "RULE_INVALID");
    assert!(error.view_model.field_errors.contains_key("raw"));
}

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
