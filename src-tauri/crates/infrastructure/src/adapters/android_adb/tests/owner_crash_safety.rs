use super::super::*;
use super::{RecordingRunner, SequenceRunner, seed_active_runtime, test_activation};
use intercept_proxy_domain::ListenerId;
use std::path::Path;

fn output(success: bool, stderr: &str) -> AdbOutput {
    AdbOutput {
        success,
        stdout: String::new(),
        stderr: stderr.into(),
    }
}

fn selected_adapter(
    data_dir: &Path,
    store: Arc<crate::SqliteStore>,
    runner: Arc<dyn AdbCommandRunner>,
) -> AndroidAdbAdapter {
    let adapter = AndroidAdbAdapter::with_store_and_runner(data_dir, store, runner);
    *adapter.selected_serial.write().unwrap() = Some("DEVICE-A".into());
    adapter
}

async fn reopen_and_stop(path: &Path, expected_ports: &[u16]) {
    let store = Arc::new(crate::SqliteStore::open(path).unwrap());
    let outputs = std::iter::once(output(false, "offline"))
        .chain(std::iter::once(output(true, "")))
        .chain(expected_ports.iter().map(|_| output(true, "")))
        .collect();
    let runner = Arc::new(SequenceRunner {
        calls: std::sync::Mutex::new(Vec::new()),
        outputs: std::sync::Mutex::new(outputs),
    });
    let mut adapter = AndroidAdbAdapter::new(None, store).unwrap();
    adapter.adb_path = Some(PathBuf::from("adb"));
    adapter.runner = runner.clone();

    adapter.network_stop().await.unwrap();

    {
        let calls = runner.calls.lock().unwrap();
        for port in expected_ports {
            assert!(calls.iter().any(|args| {
                args.ends_with(&["reverse".into(), "--remove".into(), format!("tcp:{port}")])
            }));
        }
    }
    assert!(adapter.runtime_owner_snapshot().await.is_none());
}

#[tokio::test]
async fn interruption_after_reverse_creation_reopens_with_complete_cleanup_ledger() {
    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().join("interrupted.sqlite3");
    let store = Arc::new(crate::SqliteStore::open(&path).unwrap());
    let runner = Arc::new(RecordingRunner::default());
    let adapter = selected_adapter(temp.path(), store.clone(), runner);
    let listener = ListenerId::new();
    let activation = test_activation("profile-new", "203.0.113.20", listener, 36_127);
    let old_port =
        reverse::allocated_reverse_ports(&activation.proxy_routes)[&listener.to_string()];
    seed_active_runtime(&adapter, "DEVICE-A", vec![old_port]).await;

    let prepared = adapter
        .prepare_usb_proxy_runtime(&activation, AndroidRuntimeOwnerSource::Apply)
        .await
        .unwrap();
    let expected = prepared.all_cleanup_ports();
    assert_eq!(
        store
            .load_android_runtime_owner()
            .unwrap()
            .unwrap()
            .reverse_ports,
        expected
    );
    drop(prepared);
    drop(adapter);
    drop(store);

    reopen_and_stop(&path, &expected).await;
}

#[tokio::test]
async fn commit_persistence_failure_never_cleans_old_ports_and_reopen_stops_all() {
    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().join("commit-failure.sqlite3");
    let store = Arc::new(crate::SqliteStore::open(&path).unwrap());
    let runner = Arc::new(RecordingRunner::default());
    let adapter = selected_adapter(temp.path(), store.clone(), runner.clone());
    let listener = ListenerId::new();
    let activation = test_activation("profile-new", "203.0.113.20", listener, 36_127);
    let old_port =
        reverse::allocated_reverse_ports(&activation.proxy_routes)[&listener.to_string()];
    seed_active_runtime(&adapter, "DEVICE-A", vec![old_port]).await;
    let prepared = adapter
        .prepare_usb_proxy_runtime(&activation, AndroidRuntimeOwnerSource::Apply)
        .await
        .unwrap();
    let expected = prepared.all_cleanup_ports();
    store
        .execute_test_batch(
            "CREATE TRIGGER fail_active_owner BEFORE UPDATE ON android_runtime_owner
             WHEN NEW.state = 'active' BEGIN SELECT RAISE(FAIL, 'injected'); END;",
        )
        .unwrap();

    let error = adapter
        .finish_prepared_network_update(prepared, Ok(()))
        .await
        .unwrap_err();

    assert_eq!(
        error.view_model.code,
        "ANDROID_RUNTIME_OWNER_PERSISTENCE_FAILED"
    );
    assert!(runner.calls.lock().unwrap().iter().all(|args| {
        !(args.contains(&"--remove".into()) && args.contains(&format!("tcp:{old_port}")))
    }));
    assert_eq!(
        store
            .load_android_runtime_owner()
            .unwrap()
            .unwrap()
            .reverse_ports,
        expected
    );
    drop(adapter);
    drop(store);
    reopen_and_stop(&path, &expected).await;
}

#[tokio::test]
async fn failed_old_cleanup_is_persisted_exactly_and_retried_after_reopen() {
    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().join("cleanup-failure.sqlite3");
    let store = Arc::new(crate::SqliteStore::open(&path).unwrap());
    let runner = Arc::new(SequenceRunner {
        calls: std::sync::Mutex::new(Vec::new()),
        outputs: std::sync::Mutex::new(std::collections::VecDeque::from([
            output(true, ""),
            output(true, ""),
            output(false, "cannot remove old reverse"),
        ])),
    });
    let adapter = selected_adapter(temp.path(), store.clone(), runner);
    let listener = ListenerId::new();
    let activation = test_activation("profile-new", "203.0.113.20", listener, 36_127);
    let old_port =
        reverse::allocated_reverse_ports(&activation.proxy_routes)[&listener.to_string()];
    seed_active_runtime(&adapter, "DEVICE-A", vec![old_port]).await;
    let prepared = adapter
        .prepare_usb_proxy_runtime(&activation, AndroidRuntimeOwnerSource::Apply)
        .await
        .unwrap();
    let new_port = prepared.reverse.as_ref().unwrap().ports[0];

    adapter
        .finish_prepared_network_update(prepared, Ok(()))
        .await
        .unwrap_err();

    let record = store.load_android_runtime_owner().unwrap().unwrap();
    assert_eq!(
        record.owner.state,
        AndroidRuntimeOwnerState::CleanupRequired
    );
    assert_eq!(record.reverse_ports, vec![old_port, new_port]);
    drop(adapter);
    drop(store);
    reopen_and_stop(&path, &[old_port, new_port]).await;
}

#[tokio::test]
async fn partial_rollback_survives_reopen_with_and_without_previous_owner() {
    assert_partial_rollback_survives_reopen(false).await;
    assert_partial_rollback_survives_reopen(true).await;
}

async fn assert_partial_rollback_survives_reopen(with_previous: bool) {
    let temp = tempfile::tempdir().unwrap();
    let path = temp
        .path()
        .join(format!("rollback-{with_previous}.sqlite3"));
    let store = Arc::new(crate::SqliteStore::open(&path).unwrap());
    let runner = Arc::new(SequenceRunner {
        calls: std::sync::Mutex::new(Vec::new()),
        outputs: std::sync::Mutex::new(std::collections::VecDeque::from([
            output(true, ""),
            output(true, ""),
            output(false, "cannot rollback reverse"),
        ])),
    });
    let adapter = selected_adapter(temp.path(), store.clone(), runner);
    let listener = ListenerId::new();
    let activation = test_activation("profile-new", "203.0.113.20", listener, 36_127);
    let old_ports = if with_previous {
        let port =
            reverse::allocated_reverse_ports(&activation.proxy_routes)[&listener.to_string()];
        seed_active_runtime(&adapter, "DEVICE-A", vec![port]).await;
        vec![port]
    } else {
        Vec::new()
    };
    let prepared = adapter
        .prepare_usb_proxy_runtime(&activation, AndroidRuntimeOwnerSource::Start)
        .await
        .unwrap();
    let new_port = prepared.reverse.as_ref().unwrap().ports[0];

    adapter
        .finish_prepared_network_update::<()>(
            prepared,
            Err(AppError::new("ANDROID_CONTROL_FAILED", "injected")),
        )
        .await
        .unwrap_err();

    let mut expected = old_ports;
    expected.push(new_port);
    expected.sort_unstable();
    let record = store.load_android_runtime_owner().unwrap().unwrap();
    assert_eq!(
        record.owner.state,
        AndroidRuntimeOwnerState::CleanupRequired
    );
    assert_eq!(record.reverse_ports, expected);
    drop(adapter);
    drop(store);
    reopen_and_stop(&path, &expected).await;
}
