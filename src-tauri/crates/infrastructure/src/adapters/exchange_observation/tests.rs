use std::sync::Arc;

use chrono::Utc;
use intercept_proxy_application::{
    CapacityLedger, ExchangeContext, ExchangeObservationEvent, ExchangeObservationQuery,
    ExchangeObservationRecord, ExchangeProtocol, PageRequest,
};
use intercept_proxy_domain::{ListenerId, ProtocolDirection, WorkspaceId};
use uuid::Uuid;

use super::ExchangeObservationStore;

fn record(id: &str, workspace_id: WorkspaceId) -> ExchangeObservationRecord {
    ExchangeObservationRecord {
        exchange_id: id.to_owned(),
        workspace_id,
        listener_id: ListenerId::new(),
        runtime_epoch: Uuid::new_v4(),
        peer_address: "127.0.0.1:1234".to_owned(),
        protocol: ExchangeProtocol::Socket,
        events: vec![ExchangeObservationEvent::Opened {
            observed_at: Utc::now(),
        }],
        evidence_evicted: false,
    }
}

#[test]
fn appends_events_in_vec_order_without_sequence_or_interaction_id() {
    let capacity = Arc::new(CapacityLedger::new(64 * 1024));
    let store = ExchangeObservationStore::new(capacity);
    let workspace = WorkspaceId::new();
    store.open(record("exchange-a", workspace));
    assert!(
        store
            .append(
                "exchange-a",
                ExchangeProtocol::Socket,
                None,
                ExchangeObservationEvent::Closed {
                    observed_at: Utc::now(),
                    outcome: "completed".to_owned(),
                    error: None,
                },
            )
            .is_some()
    );

    let saved = store.get("exchange-a").expect("connection record");
    assert!(matches!(
        saved.events[0],
        ExchangeObservationEvent::Opened { .. }
    ));
    assert!(matches!(
        saved.events[1],
        ExchangeObservationEvent::Closed { .. }
    ));
}

#[test]
fn missing_open_is_ignored_without_synthesizing_connection_metadata() {
    let workspace = WorkspaceId::new();
    let store = ExchangeObservationStore::new(Arc::new(CapacityLedger::new(4096)));
    assert!(
        store
            .append(
                "unknown",
                ExchangeProtocol::Socket,
                None,
                ExchangeObservationEvent::Closed {
                    observed_at: Utc::now(),
                    outcome: "failed".to_owned(),
                    error: Some("read failed".to_owned()),
                },
            )
            .is_none()
    );
    assert!(store.get("unknown").is_none());
    assert_eq!(store.ignored_events(), 1);
    let page = store.query(&ExchangeObservationQuery {
        workspace_id: workspace,
        listener_id: None,
        page: PageRequest {
            page: 1,
            page_size: 20,
        },
    });
    assert_eq!(page.dropped_events, 0);
    assert_eq!(page.ignored_events, 1);
}

#[test]
fn evicts_oldest_connection_without_marking_an_unrelated_timeline() {
    let workspace = WorkspaceId::new();
    let sample = record("exchange-a", workspace);
    let capacity = Arc::new(CapacityLedger::new(sample.logical_bytes() * 2));
    let store = ExchangeObservationStore::new(capacity);
    store.open(sample);
    store.open(record("exchange-b", workspace));
    store.open(record("exchange-c", workspace));

    assert!(store.get("exchange-a").is_none());
    assert!(
        !store
            .get("exchange-b")
            .expect("retained oldest")
            .evidence_evicted
    );
    let page = store.query(&ExchangeObservationQuery {
        workspace_id: workspace,
        listener_id: None,
        page: PageRequest {
            page: 1,
            page_size: 20,
        },
    });
    assert_eq!(page.evicted_records, 1);
    assert_eq!(page.rows.len(), 2);
}

#[test]
fn clear_releases_shared_capacity_allocation() {
    let workspace = WorkspaceId::new();
    let capacity = Arc::new(CapacityLedger::new(4096));
    let store = ExchangeObservationStore::new(Arc::clone(&capacity));
    store.open(record("exchange-a", workspace));
    assert!(capacity.capture_bytes() > 0);
    assert_eq!(store.clear_workspace(workspace), 1);
    assert_eq!(capacity.capture_bytes(), 0);
}

#[test]
fn producer_drop_counter_is_lock_free_and_visible_to_queries() {
    let workspace = WorkspaceId::new();
    let store = ExchangeObservationStore::new(Arc::new(CapacityLedger::new(4096)));
    store.open(record("exchange-a", workspace));

    store.counters().note_dropped();

    let page = store.query(&ExchangeObservationQuery {
        workspace_id: workspace,
        listener_id: None,
        page: PageRequest {
            page: 1,
            page_size: 20,
        },
    });
    assert_eq!(page.dropped_events, 1);
    assert_eq!(page.ignored_events, 0);
}

#[test]
fn query_pages_connections_from_newest_to_oldest() {
    let workspace = WorkspaceId::new();
    let store = ExchangeObservationStore::new(Arc::new(CapacityLedger::new(4096)));
    assert!(store.open(record("exchange-a", workspace)));
    assert!(store.open(record("exchange-b", workspace)));
    assert!(store.open(record("exchange-c", workspace)));

    let query = |page| ExchangeObservationQuery {
        workspace_id: workspace,
        listener_id: None,
        page: PageRequest { page, page_size: 2 },
    };

    let first = store.query(&query(1));
    assert_eq!(first.total, 3);
    assert_eq!(
        first
            .rows
            .iter()
            .map(|record| record.exchange_id.as_str())
            .collect::<Vec<_>>(),
        vec!["exchange-c", "exchange-b"]
    );

    let second = store.query(&query(2));
    assert_eq!(
        second
            .rows
            .iter()
            .map(|record| record.exchange_id.as_str())
            .collect::<Vec<_>>(),
        vec!["exchange-a"]
    );
}

#[test]
fn oversized_append_reverts_event_marks_evidence_evicted_and_counts_ignored() {
    let workspace = WorkspaceId::new();
    let opened = record("exchange-large", workspace);
    let capacity = Arc::new(CapacityLedger::new(opened.logical_bytes() + 1));
    let store = ExchangeObservationStore::new(capacity);
    assert!(store.open(opened));

    let accepted = store.append(
        "exchange-large",
        ExchangeProtocol::Socket,
        None,
        ExchangeObservationEvent::Received {
            observed_at: Utc::now(),
            direction: ProtocolDirection::Upstream,
            context: ExchangeContext::Socket {
                bytes: vec![0xA5; 256],
            },
            document: None,
            display: None,
        },
    );

    assert!(accepted.is_none());
    let retained = store.get("exchange-large").expect("opened record remains");
    assert_eq!(retained.events.len(), 1);
    assert!(retained.evidence_evicted);
    assert_eq!(store.ignored_events(), 1);
}

#[test]
fn impossible_append_does_not_evict_other_connections_before_rejection() {
    let workspace = WorkspaceId::new();
    let first = record("exchange-a", workspace);
    let second = record("exchange-b", workspace);
    let capacity = Arc::new(CapacityLedger::new(
        first.logical_bytes() + second.logical_bytes(),
    ));
    let store = ExchangeObservationStore::new(capacity);
    assert!(store.open(first));
    assert!(store.open(second));

    let accepted = store.append(
        "exchange-b",
        ExchangeProtocol::Socket,
        None,
        ExchangeObservationEvent::Received {
            observed_at: Utc::now(),
            direction: ProtocolDirection::Upstream,
            context: ExchangeContext::Socket {
                bytes: vec![0x5A; 512],
            },
            document: None,
            display: None,
        },
    );

    assert!(accepted.is_none());
    assert!(store.get("exchange-a").is_some());
    let retained = store.get("exchange-b").expect("protected record remains");
    assert_eq!(retained.events.len(), 1);
    assert!(retained.evidence_evicted);
}

#[test]
fn eviction_count_is_scoped_to_the_queried_workspace() {
    let workspace_a = WorkspaceId::new();
    let workspace_b = WorkspaceId::new();
    let sample = record("exchange-a", workspace_a);
    let store =
        ExchangeObservationStore::new(Arc::new(CapacityLedger::new(sample.logical_bytes() * 2)));
    assert!(store.open(sample));
    assert!(store.open(record("exchange-b", workspace_b)));
    assert!(store.open(record("exchange-c", workspace_b)));

    let query = |workspace_id| ExchangeObservationQuery {
        workspace_id,
        listener_id: None,
        page: PageRequest {
            page: 1,
            page_size: 20,
        },
    };

    assert_eq!(store.query(&query(workspace_a)).evicted_records, 1);
    assert_eq!(store.query(&query(workspace_b)).evicted_records, 0);
    assert!(
        !store
            .get("exchange-b")
            .expect("other workspace timeline")
            .evidence_evicted
    );
}
