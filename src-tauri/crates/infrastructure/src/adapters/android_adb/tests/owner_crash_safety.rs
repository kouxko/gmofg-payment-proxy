use super::*;
use intercept_proxy_application::AndroidDeviceTarget;

#[tokio::test]
async fn failed_reservation_prevents_adb_mutation() {
    let temp = tempfile::tempdir().unwrap();
    let store = Arc::new(crate::SqliteStore::in_memory().unwrap());
    store
        .execute_test_batch(
            "CREATE TRIGGER fail_owner_insert BEFORE INSERT ON android_runtime_owners
             BEGIN SELECT RAISE(FAIL, 'owner insert denied'); END;",
        )
        .unwrap();
    let runner = Arc::new(RecordingRunner::default());
    let adapter = AndroidAdbAdapter::with_store_and_runner(temp.path(), store, runner.clone());
    let activation = test_activation(
        "profile-new",
        "203.0.113.10",
        ListenerId::from_uuid(uuid::Uuid::new_v4()),
        16127,
    );

    let error = adapter
        .network_start(
            AndroidDeviceTarget {
                serial: "DEVICE-A".into(),
            },
            activation,
        )
        .await
        .unwrap_err();

    assert_eq!(
        error.view_model.code,
        "ANDROID_RUNTIME_OWNER_PERSISTENCE_FAILED"
    );
    let calls = runner.calls.lock().unwrap();
    assert!(!calls.iter().any(|args| {
        args.iter().any(|arg| arg == "reverse" || arg == "forward")
            || args
                .windows(3)
                .any(|window| window == ["shell", "am", "start"])
    }));
}

#[tokio::test]
async fn failed_start_without_reverse_restores_empty_owner_slot() {
    let temp = tempfile::tempdir().unwrap();
    let adapter = AndroidAdbAdapter::with_runner(temp.path(), Arc::new(FakeRunner));
    let mut activation = test_activation(
        "profile-new",
        "203.0.113.10",
        ListenerId::from_uuid(uuid::Uuid::new_v4()),
        16127,
    );
    activation.proxy_routes.clear();

    let error = adapter
        .network_start(
            AndroidDeviceTarget {
                serial: "DEVICE-A".into(),
            },
            activation,
        )
        .await
        .unwrap_err();

    assert!(matches!(
        error.view_model.code.as_str(),
        "ANDROID_CONTROL_SOCKET_UNAVAILABLE" | "ANDROID_CONTROL_SOCKET_FAILED"
    ));
    assert!(
        adapter
            .runtime_owner_snapshot_for("DEVICE-A")
            .await
            .is_none()
    );
    assert!(
        adapter
            .runtime_store
            .load_android_runtime_owners()
            .unwrap()
            .is_empty()
    );
}
