use super::super::owner::runtime_mode;
use super::*;
use uuid::Uuid;

#[tokio::test]
async fn prepared_owner_lifecycle_preserves_epoch_across_reconnect_states() {
    let temp = tempfile::tempdir().unwrap();
    let adapter = AndroidAdbAdapter::with_runner(temp.path(), Arc::new(FakeRunner));
    let owner = runtime_owner("DEVICE-A", AndroidRuntimeOwnerState::CleanupRequired);
    let prepared = prepared_runtime(owner.clone());
    adapter.save_owner(owner.clone()).await.unwrap();

    let already_managed = adapter.ensure_can_start("DEVICE-A").await.unwrap_err();
    assert_eq!(
        already_managed.view_model.code,
        "ANDROID_RUNTIME_ALREADY_MANAGED"
    );
    assert_eq!(
        already_managed.view_model.entity_id.as_deref(),
        Some("DEVICE-A")
    );
    assert_eq!(already_managed.view_model.runtime_epoch, Some(owner.epoch));

    adapter
        .publish_prepared_owner(
            &prepared,
            AndroidRuntimeOwnerState::Active,
            AndroidRuntimeOwnerTransitionReason::ActivationConfirmed,
        )
        .await
        .unwrap();
    assert_eq!(
        adapter
            .runtime_owner_snapshot_for("DEVICE-A")
            .await
            .unwrap()
            .state,
        AndroidRuntimeOwnerState::Active
    );

    assert_active_and_cleanup_reconnect(&adapter, &owner, &prepared).await;
    assert_stop_stopped_and_uncertain_reconnect(&adapter, &owner, &prepared).await;

    assert!(
        !adapter
            .clear_owner_if_epoch_under_gate("DEVICE-A", Uuid::new_v4())
            .await
            .unwrap()
    );
    assert!(
        adapter
            .clear_owner_if_epoch_under_gate("DEVICE-A", owner.epoch)
            .await
            .unwrap()
    );
    assert!(adapter.ensure_can_start("DEVICE-A").await.is_ok());
}

async fn assert_active_and_cleanup_reconnect(
    adapter: &AndroidAdbAdapter,
    owner: &AndroidRuntimeOwnerViewModel,
    prepared: &PreparedUsbProxyRuntime,
) {
    adapter
        .mark_owner_waiting_reconnect("DEVICE-A", owner.epoch)
        .await
        .unwrap();
    adapter
        .mark_owner_waiting_reconnect("DEVICE-A", owner.epoch)
        .await
        .unwrap();
    adapter
        .mark_owner_reconnected("DEVICE-A", owner.epoch, Some(AndroidNetworkState::Running))
        .await
        .unwrap();
    assert_eq!(
        adapter
            .runtime_owner_snapshot_for("DEVICE-A")
            .await
            .unwrap()
            .state,
        AndroidRuntimeOwnerState::Active
    );

    adapter
        .stage_prepared_cleanup(prepared, vec![16_127], Some(owner.epoch))
        .await
        .unwrap();
    adapter
        .mark_owner_waiting_reconnect("DEVICE-A", owner.epoch)
        .await
        .unwrap();
    adapter
        .mark_owner_reconnected("DEVICE-A", owner.epoch, None)
        .await
        .unwrap();
    assert_eq!(
        adapter
            .runtime_owner_snapshot_for("DEVICE-A")
            .await
            .unwrap()
            .state,
        AndroidRuntimeOwnerState::CleanupRequired
    );
}

async fn assert_stop_stopped_and_uncertain_reconnect(
    adapter: &AndroidAdbAdapter,
    owner: &AndroidRuntimeOwnerViewModel,
    prepared: &PreparedUsbProxyRuntime,
) {
    adapter
        .mark_owner_stop_failed("DEVICE-A", owner.epoch, "stop failed".into())
        .await
        .unwrap();
    adapter
        .mark_owner_waiting_reconnect("DEVICE-A", owner.epoch)
        .await
        .unwrap();
    adapter
        .mark_owner_reconnected("DEVICE-A", owner.epoch, None)
        .await
        .unwrap();
    assert_eq!(
        adapter
            .runtime_owner_snapshot_for("DEVICE-A")
            .await
            .unwrap()
            .state,
        AndroidRuntimeOwnerState::StopFailed
    );

    adapter
        .publish_prepared_owner(
            prepared,
            AndroidRuntimeOwnerState::Active,
            AndroidRuntimeOwnerTransitionReason::ActivationConfirmed,
        )
        .await
        .unwrap();
    adapter
        .mark_owner_waiting_reconnect("DEVICE-A", owner.epoch)
        .await
        .unwrap();
    adapter
        .mark_owner_reconnected("DEVICE-A", owner.epoch, Some(AndroidNetworkState::Stopped))
        .await
        .unwrap();
    assert_eq!(
        adapter
            .runtime_owner_snapshot_for("DEVICE-A")
            .await
            .unwrap()
            .state,
        AndroidRuntimeOwnerState::CleanupRequired
    );

    adapter
        .publish_prepared_owner(
            prepared,
            AndroidRuntimeOwnerState::Active,
            AndroidRuntimeOwnerTransitionReason::ActivationConfirmed,
        )
        .await
        .unwrap();
    adapter
        .mark_owner_waiting_reconnect("DEVICE-A", owner.epoch)
        .await
        .unwrap();
    adapter
        .mark_owner_reconnected("DEVICE-A", owner.epoch, None)
        .await
        .unwrap();
    assert_eq!(
        adapter
            .runtime_owner_snapshot_for("DEVICE-A")
            .await
            .unwrap()
            .state,
        AndroidRuntimeOwnerState::Uncertain
    );
}

#[tokio::test]
async fn owner_conflicts_use_authoritative_persistence_context() {
    let temp = tempfile::tempdir().unwrap();
    let adapter = AndroidAdbAdapter::with_runner(temp.path(), Arc::new(FakeRunner));

    let missing = adapter.runtime_owner_conflict_error("DEVICE-A").await;
    assert_eq!(missing.view_model.code, "ANDROID_RUNTIME_NOT_MANAGED");
    assert_eq!(missing.view_model.entity_id.as_deref(), Some("DEVICE-A"));

    let owner = runtime_owner("DEVICE-A", AndroidRuntimeOwnerState::Active);
    adapter.save_owner(owner.clone()).await.unwrap();
    let stale = adapter.runtime_owner_conflict_error("DEVICE-A").await;
    assert_eq!(stale.view_model.code, "ANDROID_RUNTIME_EPOCH_STALE");
    assert_eq!(stale.view_model.runtime_epoch, Some(owner.epoch));

    let stale_target = adapter
        .required_runtime_target("DEVICE-A", Uuid::new_v4())
        .await
        .unwrap_err();
    assert_eq!(stale_target.view_model.runtime_epoch, Some(owner.epoch));
    let missing_target = adapter
        .required_runtime_target("DEVICE-B", Uuid::new_v4())
        .await
        .unwrap_err();
    assert_eq!(
        missing_target.view_model.code,
        "ANDROID_RUNTIME_NOT_MANAGED"
    );

    adapter
        .runtime_store
        .clear_android_runtime_owner("DEVICE-A", owner.epoch)
        .unwrap();
    let conflict = adapter
        .publish_prepared_owner(
            &prepared_runtime(owner),
            AndroidRuntimeOwnerState::Active,
            AndroidRuntimeOwnerTransitionReason::ActivationConfirmed,
        )
        .await
        .unwrap_err();
    assert_eq!(conflict.view_model.code, "ANDROID_RUNTIME_NOT_MANAGED");
}

#[tokio::test]
async fn persistence_failures_keep_device_and_fallback_epoch_context() {
    let temp = tempfile::tempdir().unwrap();
    let store = Arc::new(crate::SqliteStore::in_memory().unwrap());
    let adapter = AndroidAdbAdapter::with_store_and_runner(
        temp.path(),
        Arc::clone(&store),
        Arc::new(FakeRunner),
    );
    store
        .execute_test_batch("DROP TABLE android_runtime_owners;")
        .unwrap();

    let read_error = adapter.authoritative_runtime_owners().await.unwrap_err();
    assert_eq!(
        read_error.view_model.code,
        "ANDROID_RUNTIME_OWNER_PERSISTENCE_FAILED"
    );

    let fallback_epoch = Uuid::new_v4();
    let contextualized = adapter
        .contextualize_authoritative_owner_error(
            "DEVICE-A",
            AppError::new("ANDROID_ADB_COMMAND_FAILED", "adb failed").epoch(fallback_epoch),
        )
        .await;
    assert_eq!(
        contextualized.view_model.entity_id.as_deref(),
        Some("DEVICE-A")
    );
    assert_eq!(
        contextualized.view_model.runtime_epoch,
        Some(fallback_epoch)
    );

    let conflict = adapter.runtime_owner_conflict_error("DEVICE-A").await;
    assert_eq!(
        conflict.view_model.code,
        "ANDROID_RUNTIME_OWNER_PERSISTENCE_FAILED"
    );
    assert_eq!(conflict.view_model.entity_id.as_deref(), Some("DEVICE-A"));
}

#[test]
fn runtime_mode_distinguishes_device_reverse_and_lan() {
    assert_eq!(runtime_mode(0, false), AndroidRuntimeOwnerMode::DeviceOnly);
    assert_eq!(runtime_mode(1, true), AndroidRuntimeOwnerMode::AdbReverse);
    assert_eq!(runtime_mode(1, false), AndroidRuntimeOwnerMode::Lan);
}
