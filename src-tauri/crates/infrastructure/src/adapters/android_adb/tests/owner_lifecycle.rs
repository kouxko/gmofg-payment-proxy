use super::*;
use intercept_proxy_application::{AndroidDeviceTarget, AndroidRuntimeTarget};

#[tokio::test]
async fn status_without_owner_is_exactly_scoped_to_requested_serial() {
    let temp = tempfile::tempdir().unwrap();
    let runner = Arc::new(RecordingRunner::default());
    let adapter = AndroidAdbAdapter::with_runner(temp.path(), runner.clone());
    *adapter.selected_serial.write().unwrap() = Some("DEVICE-B".into());

    let status = adapter
        .network_status(AndroidDeviceTarget {
            serial: "DEVICE-A".into(),
        })
        .await
        .unwrap();

    assert_eq!(status.serial, "DEVICE-A");
    assert_eq!(status.runtime_epoch, None);
    assert_eq!(status.state, AndroidNetworkState::Stopped);
    assert!(runner.calls.lock().unwrap().is_empty());
}

#[tokio::test]
async fn selected_device_change_does_not_retarget_stop() {
    let temp = tempfile::tempdir().unwrap();
    let runner = Arc::new(RecordingRunner::default());
    let adapter = AndroidAdbAdapter::with_runner(temp.path(), runner.clone());
    let mut owner = runtime_owner("DEVICE-A", AndroidRuntimeOwnerState::Active);
    owner.mode = AndroidRuntimeOwnerMode::DeviceOnly;
    adapter.save_owner(owner.clone()).await.unwrap();
    *adapter.selected_serial.write().unwrap() = Some("DEVICE-B".into());

    let status = adapter
        .network_stop(AndroidRuntimeTarget {
            serial: owner.serial.clone(),
            expected_epoch: owner.epoch,
        })
        .await
        .unwrap();

    assert_eq!(status.serial, "DEVICE-A");
    assert_eq!(status.runtime_epoch, Some(owner.epoch));
    assert!(
        adapter
            .runtime_owner_snapshot_for("DEVICE-A")
            .await
            .is_none()
    );
    assert!(runner.calls.lock().unwrap().iter().all(|args| {
        args.windows(2).any(|pair| pair == ["-s", "DEVICE-A"])
            || args.first().is_some_and(|arg| arg == "version")
    }));
}

#[tokio::test]
async fn stale_epoch_preserves_current_owner() {
    let temp = tempfile::tempdir().unwrap();
    let adapter = AndroidAdbAdapter::with_runner(temp.path(), Arc::new(FakeRunner));
    let owner = runtime_owner("DEVICE-A", AndroidRuntimeOwnerState::Active);
    adapter.save_owner(owner.clone()).await.unwrap();

    let error = adapter
        .emergency_restore(AndroidRuntimeTarget {
            serial: "DEVICE-A".into(),
            expected_epoch: uuid::Uuid::new_v4(),
        })
        .await
        .unwrap_err();

    assert_eq!(error.view_model.code, "ANDROID_RUNTIME_EPOCH_STALE");
    assert_eq!(error.view_model.entity_id.as_deref(), Some("DEVICE-A"));
    assert_eq!(error.view_model.runtime_epoch, Some(owner.epoch));
    assert_eq!(
        adapter.runtime_owner_snapshot_for("DEVICE-A").await,
        Some(owner)
    );
}
