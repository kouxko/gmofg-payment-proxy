use super::*;

#[tokio::test]
async fn different_devices_can_reserve_owners_concurrently() {
    let temp = tempfile::tempdir().unwrap();
    let adapter = Arc::new(AndroidAdbAdapter::with_runner(
        temp.path(),
        Arc::new(FakeRunner),
    ));
    let owner_a = runtime_owner("DEVICE-A", AndroidRuntimeOwnerState::Active);
    let owner_b = runtime_owner("DEVICE-B", AndroidRuntimeOwnerState::Active);

    let (saved_a, saved_b) = tokio::join!(
        adapter.save_owner(owner_a.clone()),
        adapter.save_owner(owner_b.clone()),
    );
    saved_a.unwrap();
    saved_b.unwrap();

    assert_eq!(
        adapter.runtime_owner_snapshot_for("DEVICE-A").await,
        Some(owner_a)
    );
    assert_eq!(
        adapter.runtime_owner_snapshot_for("DEVICE-B").await,
        Some(owner_b)
    );
    assert_eq!(
        adapter
            .runtime_store
            .load_android_runtime_owners()
            .unwrap()
            .len(),
        2
    );
}

#[tokio::test]
async fn concurrent_reservation_for_same_serial_has_one_winner() {
    let temp = tempfile::tempdir().unwrap();
    let adapter = Arc::new(AndroidAdbAdapter::with_runner(
        temp.path(),
        Arc::new(FakeRunner),
    ));
    let first = runtime_owner("DEVICE-A", AndroidRuntimeOwnerState::Active);
    let second = runtime_owner("DEVICE-A", AndroidRuntimeOwnerState::Active);

    let (first_result, second_result) =
        tokio::join!(adapter.save_owner(first), adapter.save_owner(second));
    let results = [first_result, second_result];

    assert_eq!(results.iter().filter(|result| result.is_ok()).count(), 1);
    let error = results.into_iter().find_map(Result::err).unwrap();
    assert_eq!(error.view_model.code, "ANDROID_RUNTIME_ALREADY_MANAGED");
    assert_eq!(
        adapter
            .runtime_store
            .load_android_runtime_owners()
            .unwrap()
            .len(),
        1
    );
}

#[tokio::test]
async fn ninth_retained_owner_is_rejected_without_overwriting_the_first_eight() {
    let temp = tempfile::tempdir().unwrap();
    let adapter = AndroidAdbAdapter::with_runner(temp.path(), Arc::new(FakeRunner));
    for index in 0..8 {
        adapter
            .save_owner(runtime_owner(
                &format!("DEVICE-{index}"),
                AndroidRuntimeOwnerState::Active,
            ))
            .await
            .unwrap();
    }

    let error = adapter
        .save_owner(runtime_owner("DEVICE-8", AndroidRuntimeOwnerState::Active))
        .await
        .unwrap_err();

    assert_eq!(error.view_model.code, "ANDROID_RUNTIME_CAPACITY_EXCEEDED");
    assert_eq!(
        adapter
            .runtime_store
            .load_android_runtime_owners()
            .unwrap()
            .len(),
        8
    );
}

#[tokio::test]
async fn reopened_adapter_restores_all_owners_in_serial_order() {
    let store = Arc::new(crate::SqliteStore::in_memory().unwrap());
    store
        .reserve_android_runtime_owner(&crate::AndroidRuntimeOwnerRecord {
            owner: runtime_owner("DEVICE-B", AndroidRuntimeOwnerState::Active),
            reverse_ports: Vec::new(),
            resume_state: None,
            runtime_endpoints: Vec::new(),
        })
        .unwrap();
    store
        .reserve_android_runtime_owner(&crate::AndroidRuntimeOwnerRecord {
            owner: runtime_owner("DEVICE-A", AndroidRuntimeOwnerState::Active),
            reverse_ports: Vec::new(),
            resume_state: None,
            runtime_endpoints: Vec::new(),
        })
        .unwrap();

    let adapter = AndroidAdbAdapter::new(None, store).await.unwrap();
    let owners = adapter.runtime_owners().await.unwrap();

    assert_eq!(
        owners
            .iter()
            .map(|owner| owner.serial.as_str())
            .collect::<Vec<_>>(),
        vec!["DEVICE-A", "DEVICE-B"]
    );
    assert!(
        owners
            .iter()
            .all(|owner| owner.source == AndroidRuntimeOwnerSource::Recovery)
    );
}
