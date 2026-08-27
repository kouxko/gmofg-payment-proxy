#[tokio::test]
async fn workspace_switch_keeps_running_vpn_bound_to_its_original_workspace() {
    let fixture = running_vpn_fixture().await;

    let status = fixture
        .application
        .device_network_status(fixture.serial.clone())
        .await
        .unwrap();

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
    let degraded = fixture
        .application
        .device_network_status(fixture.serial.clone())
        .await
        .unwrap();
    assert_eq!(degraded.state, AndroidNetworkState::Faulted);
    assert!(degraded.message.contains("应用修改"));
    assert_eq!(
        fixture.android.network_apply_calls.load(Ordering::SeqCst),
        0
    );
}

#[tokio::test]
async fn device_network_status_propagates_readiness_failure_with_owner_context() {
    let fixture = running_vpn_fixture().await;
    fixture
        .android
        .runtime_ready_fails
        .store(true, Ordering::SeqCst);

    let error = fixture
        .application
        .device_network_status(fixture.serial.clone())
        .await
        .unwrap_err();

    assert_eq!(error.view_model.code, "ANDROID_RUNTIME_READINESS_FAILED");
    assert_eq!(
        error.view_model.entity_id.as_deref(),
        Some(fixture.serial.as_str())
    );
    assert_eq!(error.view_model.runtime_epoch, Some(fixture.runtime_epoch));
}

#[tokio::test]
async fn device_network_status_enriches_raw_port_failure_with_authoritative_owner_context() {
    let fixture = running_vpn_fixture().await;
    fixture
        .android
        .network_status_fails
        .store(true, Ordering::SeqCst);

    let error = fixture
        .application
        .device_network_status(fixture.serial.clone())
        .await
        .unwrap_err();

    assert_eq!(error.view_model.code, "ANDROID_RUNTIME_STATUS_FAILED");
    assert_eq!(
        error.view_model.entity_id.as_deref(),
        Some(fixture.serial.as_str())
    );
    assert_eq!(error.view_model.runtime_epoch, Some(fixture.runtime_epoch));
}

#[tokio::test]
async fn device_network_stop_enriches_raw_port_failure_with_authoritative_owner_context() {
    let fixture = running_vpn_fixture().await;
    fixture
        .android
        .network_stop_fails
        .store(true, Ordering::SeqCst);

    let error = fixture
        .application
        .device_network_stop(fixture.serial.clone(), fixture.runtime_epoch)
        .await
        .unwrap_err();

    assert_eq!(error.view_model.code, "ANDROID_ADB_COMMAND_FAILED");
    assert_eq!(error.view_model.message, "raw stop failure");
    assert_eq!(
        error.view_model.entity_id.as_deref(),
        Some(fixture.serial.as_str())
    );
    assert_eq!(error.view_model.runtime_epoch, Some(fixture.runtime_epoch));
}

#[tokio::test]
async fn workspace_and_device_selection_wait_for_device_network_start_to_finish() {
    let fixture =
        running_vpn_fixture_with_listener_state(false, Some(ListenerRuntimeState::Running)).await;
    fixture
        .workspaces
        .select(fixture.original_id)
        .await
        .unwrap();
    fixture.android.block_start.store(true, Ordering::SeqCst);
    let application = Arc::new(fixture.application);

    let start_application = Arc::clone(&application);
    let serial = fixture.serial.clone();
    let profile_id = fixture.profile_id.clone();
    let start = tokio::spawn(async move {
        start_application
            .device_network_start(serial, profile_id, false)
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
