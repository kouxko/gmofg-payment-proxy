use std::{
    collections::{BTreeMap, VecDeque},
    sync::{
        Arc,
        atomic::{AtomicBool, AtomicUsize, Ordering},
    },
    time::Duration,
};

use async_trait::async_trait;
use chrono::{DateTime, TimeZone, Utc};
use uuid::Uuid;

use crate::*;

fn timestamp(second: u32) -> DateTime<Utc> {
    Utc.with_ymd_and_hms(2026, 7, 28, 0, 0, second)
        .single()
        .expect("valid test time")
}

fn content(body: &[u8]) -> MessageContentViewModel {
    MessageContentViewModel {
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
                channel: ChannelKind::Transaction,
                method: "POST".into(),
                target: "/payment".into(),
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
            channel: ChannelKind::Transaction,
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
        channel: ChannelKind::Transaction,
        channel_text: "交易".into(),
        stage: MessageStage::Request,
        stage_text: "请求".into(),
        method: "POST".into(),
        target: "/payment".into(),
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
    let summary = &record.detail.summary;
    let strings = summary.request_id.len()
        + summary.terminal_ip.len()
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
        + expected_message
        + content(b"draft").logical_bytes();
    assert_eq!(record.logical_bytes(), expected);
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

// DATA-008~011, TEST-CAPACITY: pending UI-event bytes participate in the same byte limit.
#[test]
fn pending_ui_event_bytes_trigger_capacity_eviction() {
    let record = session(Uuid::from_u128(1), 1, false, b"body");
    let record_bytes = record.logical_bytes();
    let store = InMemorySessionStore::new(10, record_bytes + 10);
    store.upsert(record).expect("insert session");
    let evicted = store
        .set_pending_ui_event_bytes(11)
        .expect("event bytes evict completed session");
    assert_eq!(evicted, vec![Uuid::from_u128(1)]);
    assert_eq!(store.logical_bytes(), 11);
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
            channel: Some(ChannelKind::Transaction),
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
        BreakpointOutcome::Decision(BreakpointDecision {
            kind: BreakpointDecisionKind::ForwardOriginal,
            ..
        })
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
    assert_eq!(
        slow.live
            .recv()
            .await
            .expect("first queued event remains")
            .event_id,
        1
    );
    assert!(
        slow.live.recv().await.is_none(),
        "overflow closes only slow queue"
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
    assert!(
        cancelled.live.recv().await.is_none(),
        "explicit cancellation ends the receiver"
    );
    drop(cancelled);
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
        upstream_transaction_url: "https://transaction.example.test/api".into(),
        upstream_dll_url: "https://dll.example.test/api".into(),
        ..SettingsDraft::default()
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
            settings: parking_lot::Mutex::new(fake_settings_view()),
            certificate_overview: parking_lot::Mutex::new(fake_certificate_overview()),
        }
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
        unused()
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
    async fn overview(&self) -> AppResult<CertificateOverviewViewModel> {
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
    Application::new(
        ports.clone(),
        ports.clone(),
        ports.clone(),
        Arc::new(BreakpointCoordinator::default()),
        ports.clone(),
        ports.clone(),
        ports.clone(),
        ports.clone(),
        ports.clone(),
        ports,
        Arc::new(EventHub::default()),
    )
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
        ports.clone(),
        ports.clone(),
        ports.clone(),
        coordinator,
        Arc::new(BreakpointValidator),
        ports.clone(),
        ports.clone(),
        ports.clone(),
        ports.clone(),
        ports,
        Arc::new(EventHub::default()),
    );

    application
        .breakpoint_resolve(
            epoch,
            BreakpointDecision {
                breakpoint_id: id,
                expected_revision: 7,
                kind: BreakpointDecisionKind::ForwardModified,
                message: Some(MessageContentViewModel {
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
async fn settings_use_case_rejects_locally_then_calls_fake_port_for_valid_draft() {
    let ports = Arc::new(FakePorts::default());
    let application = application_with_fake_ports(ports.clone());
    let invalid = SettingsDraft::default();
    let validation = application
        .settings_validate(invalid)
        .await
        .expect("validation result");
    assert!(!validation.valid);
    assert_eq!(ports.settings_validations.load(Ordering::SeqCst), 0);

    let valid = SettingsDraft {
        upstream_transaction_url: " https://transaction.example.test/api ".into(),
        upstream_dll_url: "https://dll.example.test/api".into(),
        ..SettingsDraft::default()
    };
    assert!(
        application
            .settings_validate(valid)
            .await
            .expect("fake validation result")
            .valid
    );
    assert_eq!(ports.settings_validations.load(Ordering::SeqCst), 1);
}

#[tokio::test]
async fn settings_san_raw_input_is_normalized_atomically_in_rust() {
    let ports = Arc::new(FakePorts::default());
    let application = application_with_fake_ports(ports.clone());
    let draft = ports.settings.lock().stored.clone();

    let saved = application
        .settings_save_input(draft, " 127.0.0.1，127.0.0.1, ".into())
        .await
        .expect("save normalized settings");

    assert_eq!(saved.stored.leaf_sans, vec!["127.0.0.1"]);
}

#[tokio::test]
async fn settings_can_be_saved_before_first_certificate_setup() {
    let ports = Arc::new(FakePorts::default());
    {
        let mut overview = ports.certificate_overview.lock();
        overview.ready = false;
        overview.items.clear();
        overview.status_text = "证书配置不完整".into();
        overview.ui_tone = UiTone::Warning;
    }
    let application = application_with_fake_ports(ports);
    let draft = SettingsDraft {
        upstream_transaction_url: "https://transaction.example.test/api".into(),
        upstream_dll_url: "https://dll.example.test/api".into(),
        leaf_sans: vec!["10.0.34.50".into()],
        ..SettingsDraft::default()
    };

    let validation = application.settings_validate(draft).await.unwrap();

    assert!(validation.valid);
    assert!(
        validation
            .warnings
            .iter()
            .any(|warning| warning.contains("证书"))
    );
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
    let candidate = SettingsDraft {
        transaction_port: 20_000,
        ..original.stored.clone()
    };

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
        RuleMatchField::JsonPath { path: "$".into() }
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

#[test]
fn rejected_ui_event_byte_sync_keeps_truthful_external_byte_count_atomically() {
    let store = InMemorySessionStore::new(1, 10);
    let error = store
        .set_pending_ui_event_bytes(11)
        .expect_err("external event bytes exceed capacity");
    assert_eq!(error.view_model.code, "RESOURCE_EXHAUSTED");
    assert_eq!(
        store.logical_bytes(),
        11,
        "failed capacity admission still accounts the external queue exactly"
    );
}
