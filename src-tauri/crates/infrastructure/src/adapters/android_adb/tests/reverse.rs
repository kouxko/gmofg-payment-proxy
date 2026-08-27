use super::*;

#[tokio::test]
async fn reverse_cleanup_for_device_a_never_targets_device_b() {
    let temp = tempfile::tempdir().unwrap();
    let runner = Arc::new(RecordingRunner::default());
    let adapter = AndroidAdbAdapter::with_runner(temp.path(), runner.clone());
    let mut owner_a = runtime_owner("DEVICE-A", AndroidRuntimeOwnerState::Active);
    owner_a.mode = AndroidRuntimeOwnerMode::AdbReverse;
    let mut owner_b = runtime_owner("DEVICE-B", AndroidRuntimeOwnerState::Active);
    owner_b.mode = AndroidRuntimeOwnerMode::AdbReverse;
    adapter.save_owner(owner_a.clone()).await.unwrap();
    adapter.save_owner(owner_b.clone()).await.unwrap();
    {
        let mut states = adapter.owner_states.lock().await;
        states.get_mut("DEVICE-A").unwrap().active_reverse = Some(ActiveReverseOwnership {
            epoch: owner_a.epoch,
            serial: owner_a.serial.clone(),
            profile_id: owner_a.profile_id.clone(),
            ports: vec![40123],
        });
        states.get_mut("DEVICE-B").unwrap().active_reverse = Some(ActiveReverseOwnership {
            epoch: owner_b.epoch,
            serial: owner_b.serial.clone(),
            profile_id: owner_b.profile_id.clone(),
            ports: vec![40124],
        });
    }

    adapter.cleanup_owner_reverse(&owner_a).await.unwrap();

    {
        let calls = runner.calls.lock().unwrap();
        assert!(
            calls
                .iter()
                .all(|args| args.get(1).is_some_and(|serial| serial == "DEVICE-A"))
        );
    }
    assert_eq!(
        adapter
            .owner_state_snapshot_for("DEVICE-B")
            .await
            .active_reverse
            .unwrap()
            .ports,
        vec![40124]
    );
}

#[tokio::test]
async fn preparation_uses_explicit_serial_even_when_selection_changes() {
    let temp = tempfile::tempdir().unwrap();
    let runner = Arc::new(RecordingRunner::default());
    let adapter = AndroidAdbAdapter::with_runner(temp.path(), runner.clone());
    *adapter.selected_serial.write().unwrap() = Some("DEVICE-B".into());
    let activation = test_activation(
        "profile-a",
        "203.0.113.10",
        ListenerId::from_uuid(uuid::Uuid::new_v4()),
        16127,
    );

    let prepared = adapter
        .prepare_usb_proxy_runtime(
            "DEVICE-A",
            None,
            &activation,
            AndroidRuntimeOwnerSource::Start,
        )
        .await
        .unwrap();

    assert_eq!(prepared.owner.serial, "DEVICE-A");
    assert!(
        runner
            .calls
            .lock()
            .unwrap()
            .iter()
            .all(|args| { args.get(1).is_some_and(|serial| serial == "DEVICE-A") })
    );
}
