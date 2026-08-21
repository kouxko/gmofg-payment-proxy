use std::sync::Arc;

use chrono::{Duration, Utc};
use tempfile::TempDir;

use super::{ApplicationLogLevel, ApplicationLogQuery, RuntimeLogStore};

#[test]
fn persisted_logs_survive_reopen_with_stable_ids() {
    let directory = TempDir::new().unwrap();
    let path = directory.path().join("application-runtime.jsonl");
    let store = RuntimeLogStore::open(path.clone(), 8, 1_000_000).unwrap();
    let first = store.record(
        ApplicationLogLevel::Info,
        "intercept_proxy::listener",
        "listener_id=entry-1 event=started",
    );
    let second = store.record(
        ApplicationLogLevel::Error,
        "intercept_proxy::socket",
        "connection_id=conn-1 error=write_failed cause=peer_closed",
    );
    drop(store);

    let reopened = RuntimeLogStore::open(path, 8, 1_000_000).unwrap();
    assert_eq!(
        reopened.get(first).unwrap().message,
        "listener_id=entry-1 event=started"
    );
    assert_eq!(
        reopened.get(second).unwrap().level,
        ApplicationLogLevel::Error
    );
    assert_eq!(
        reopened.record(
            ApplicationLogLevel::Info,
            "intercept_proxy::mcp",
            "reopened"
        ),
        second + 1
    );
}

#[test]
fn retention_and_filters_are_explicit_in_query_results() {
    let store = Arc::new(RuntimeLogStore::memory(3));
    store.record(
        ApplicationLogLevel::Info,
        "listener",
        "listener_id=a started",
    );
    store.record(
        ApplicationLogLevel::Warning,
        "socket",
        "connection_id=b slow write",
    );
    store.record(
        ApplicationLogLevel::Error,
        "socket",
        "connection_id=b peer closed",
    );
    store.record(ApplicationLogLevel::Info, "mcp", "query completed");

    let page = store.query(&ApplicationLogQuery {
        level: Some(ApplicationLogLevel::Error),
        target: Some("socket".into()),
        keyword: Some("peer".into()),
        occurred_from: Some(Utc::now() - Duration::minutes(1)),
        occurred_to: Some(Utc::now() + Duration::minutes(1)),
        before_log_id: None,
        limit: 50,
    });

    assert_eq!(page.rows.len(), 1);
    assert!(page.rows[0].message.contains("peer closed"));
    assert_eq!(page.evicted_count, 1);
    assert_eq!(page.oldest_retained_log_id, Some(2));
    assert_eq!(page.newest_retained_log_id, Some(4));
    assert!(!page.has_more);
}

#[test]
fn time_range_filters_do_not_change_stable_log_ids() {
    let store = RuntimeLogStore::memory(4);
    let log_id = store.record(ApplicationLogLevel::Info, "listener", "started");

    let excluded = store.query(&ApplicationLogQuery {
        occurred_to: Some(Utc::now() - Duration::minutes(1)),
        ..ApplicationLogQuery::default()
    });
    let included = store.query(&ApplicationLogQuery {
        occurred_from: Some(Utc::now() - Duration::minutes(1)),
        ..ApplicationLogQuery::default()
    });

    assert!(excluded.rows.is_empty());
    assert_eq!(included.rows[0].log_id, log_id);
}

#[test]
fn cursor_pages_are_stable_and_newest_first() {
    let store = RuntimeLogStore::memory(10);
    for index in 0..5 {
        store.record(ApplicationLogLevel::Info, "test", &format!("event={index}"));
    }

    let first = store.query(&ApplicationLogQuery {
        limit: 2,
        ..ApplicationLogQuery::default()
    });
    assert_eq!(
        first.rows.iter().map(|row| row.log_id).collect::<Vec<_>>(),
        [5, 4]
    );
    assert!(first.has_more);

    let second = store.query(&ApplicationLogQuery {
        before_log_id: Some(4),
        limit: 2,
        ..ApplicationLogQuery::default()
    });
    assert_eq!(
        second.rows.iter().map(|row| row.log_id).collect::<Vec<_>>(),
        [3, 2]
    );
    assert!(second.has_more);
}

#[test]
fn oversized_single_log_is_truncated_without_breaking_jsonl() {
    let directory = TempDir::new().unwrap();
    let path = directory.path().join("bounded.jsonl");
    let store = RuntimeLogStore::open(path.clone(), 8, 1_000_000).unwrap();
    let log_id = store.record(ApplicationLogLevel::Error, "test", &"界".repeat(80_000));

    let entry = store.get(log_id).unwrap();
    assert!(entry.message_truncated);
    assert!(entry.message.chars().count() <= 65_536);
    drop(store);
    assert!(
        RuntimeLogStore::open(path, 8, 1_000_000)
            .unwrap()
            .get(log_id)
            .is_some()
    );
}

#[test]
fn byte_budget_rotation_persists_each_retained_entry_once() {
    let directory = TempDir::new().unwrap();
    let path = directory.path().join("rotation.jsonl");
    let store = RuntimeLogStore::open(path.clone(), 32, 512).unwrap();

    for index in 0..12 {
        store.record(
            ApplicationLogLevel::Info,
            "rotation",
            &format!("event={index} payload={}", "x".repeat(80)),
        );
    }
    let in_memory = store.query(&ApplicationLogQuery {
        limit: 500,
        ..ApplicationLogQuery::default()
    });
    drop(store);

    let lines = std::fs::read_to_string(&path)
        .unwrap()
        .lines()
        .map(|line| serde_json::from_str::<super::ApplicationLogEntry>(line).unwrap())
        .collect::<Vec<_>>();
    let persisted_ids = lines.iter().map(|entry| entry.log_id).collect::<Vec<_>>();
    let memory_ids = in_memory
        .rows
        .iter()
        .rev()
        .map(|entry| entry.log_id)
        .collect::<Vec<_>>();

    assert_eq!(persisted_ids, memory_ids);
    assert!(persisted_ids.windows(2).all(|ids| ids[0] < ids[1]));
    assert!(std::fs::metadata(path).unwrap().len() <= 512);
}

#[test]
fn byte_budget_bounds_memory_and_disk_during_sustained_writes() {
    let directory = TempDir::new().unwrap();
    let path = directory.path().join("bounded.jsonl");
    let store = RuntimeLogStore::open(path.clone(), 1_000, 640).unwrap();

    for index in 0..100 {
        store.record(
            ApplicationLogLevel::Warning,
            "sustained",
            &format!("event={index} payload={}", "界".repeat(80)),
        );
        assert!(std::fs::metadata(&path).unwrap().len() <= 640);
    }

    let page = store.query(&ApplicationLogQuery {
        limit: 500,
        ..ApplicationLogQuery::default()
    });
    let retained_jsonl_bytes = page
        .rows
        .iter()
        .map(|entry| serde_json::to_vec(entry).unwrap().len() as u64 + 1)
        .sum::<u64>();
    assert!(retained_jsonl_bytes <= 640);
    assert_eq!(page.evicted_count + page.rows.len() as u64, 100);
    assert_eq!(page.newest_retained_log_id, Some(100));
}

#[test]
fn count_capacity_rotation_rewrites_in_batches() {
    let directory = TempDir::new().unwrap();
    let path = directory.path().join("count-rotation.jsonl");
    let store = RuntimeLogStore::open(path.clone(), 8, 1_000_000).unwrap();

    for index in 0..40 {
        store.record(
            ApplicationLogLevel::Info,
            "count-rotation",
            &format!("event={index}"),
        );
    }

    let page = store.query(&ApplicationLogQuery {
        limit: 500,
        ..ApplicationLogQuery::default()
    });
    assert_eq!(page.newest_retained_log_id, Some(40));
    assert_eq!(page.evicted_count + page.rows.len() as u64, 40);
    assert!(page.rows.len() <= 8);
    assert_eq!(store.persistence_rewrite_count(), 11);

    drop(store);
    let reopened = RuntimeLogStore::open(path, 8, 1_000_000).unwrap();
    assert_eq!(
        reopened
            .query(&ApplicationLogQuery::default())
            .newest_retained_log_id,
        Some(40)
    );
}

#[test]
fn reopen_after_rotation_preserves_unique_ids_and_cursor_order() {
    let directory = TempDir::new().unwrap();
    let path = directory.path().join("reopen.jsonl");
    let store = RuntimeLogStore::open(path.clone(), 100, 512).unwrap();
    for index in 0..20 {
        store.record(
            ApplicationLogLevel::Info,
            "restart",
            &format!("before={index} payload={}", "x".repeat(60)),
        );
    }
    let newest_before_restart = store
        .query(&ApplicationLogQuery::default())
        .newest_retained_log_id
        .unwrap();
    drop(store);

    let reopened = RuntimeLogStore::open(path, 100, 512).unwrap();
    let first_after_restart =
        reopened.record(ApplicationLogLevel::Info, "restart", "after restart");
    assert_eq!(first_after_restart, newest_before_restart + 1);

    let page = reopened.query(&ApplicationLogQuery {
        limit: 500,
        ..ApplicationLogQuery::default()
    });
    assert!(
        page.rows
            .windows(2)
            .all(|rows| rows[0].log_id > rows[1].log_id)
    );
    let mut ids = page
        .rows
        .iter()
        .map(|entry| entry.log_id)
        .collect::<Vec<_>>();
    ids.sort_unstable();
    ids.dedup();
    assert_eq!(ids.len(), page.rows.len());
    assert_eq!(page.evicted_count + page.rows.len() as u64, 21);
}

#[test]
fn file_budget_smaller_than_one_jsonl_record_is_rejected() {
    let directory = TempDir::new().unwrap();
    let path = directory.path().join("too-small.jsonl");

    let error = RuntimeLogStore::open(path, 8, 64).unwrap_err();

    assert_eq!(error.kind(), std::io::ErrorKind::InvalidInput);
    assert!(error.to_string().contains("minimum"));
}

#[test]
fn minimum_file_budget_fits_one_bounded_record() {
    let directory = TempDir::new().unwrap();
    let path = directory.path().join("minimum.jsonl");
    let store = RuntimeLogStore::open(path.clone(), 8, 256).unwrap();

    let log_id = store.record(
        ApplicationLogLevel::Error,
        &"target".repeat(1_000),
        &"payload".repeat(20_000),
    );

    let retained = store.get(log_id).unwrap();
    assert!(retained.message_truncated);
    assert!(std::fs::metadata(path).unwrap().len() <= 256);
}

#[test]
fn reopen_rejects_exhausted_log_id_space_instead_of_reusing_an_id() {
    let directory = TempDir::new().unwrap();
    let path = directory.path().join("exhausted.jsonl");
    let entry = super::ApplicationLogEntry {
        log_id: u64::MAX,
        occurred_at: Utc::now(),
        level: ApplicationLogLevel::Error,
        target: "test".into(),
        message: "last possible id".into(),
        message_truncated: false,
    };
    std::fs::write(
        &path,
        format!("{}\n", serde_json::to_string(&entry).unwrap()),
    )
    .unwrap();

    let error = RuntimeLogStore::open(path, 8, 1_000_000).unwrap_err();

    assert_eq!(error.kind(), std::io::ErrorKind::InvalidData);
    assert!(error.to_string().contains("id space"));
}
