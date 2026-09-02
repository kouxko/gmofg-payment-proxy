#[tokio::test]
async fn profile_delete_checks_runtime_only_after_inflight_start_finishes() {
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

    let delete_application = Arc::clone(&application);
    let profile_id = fixture.profile_id.clone();
    let mut delete = tokio::spawn(async move {
        delete_application
            .device_network_profile_delete(profile_id)
            .await
    });
    assert!(
        tokio::time::timeout(std::time::Duration::from_millis(50), &mut delete)
            .await
            .is_err(),
        "方案删除必须等待设备网络启动结束后再检查运行状态"
    );

    fixture.android.start_release.notify_one();
    start.await.unwrap().unwrap();
    let error = delete.await.unwrap().expect_err("活动方案不能被删除");
    assert_eq!(error.view_model.code, "ANDROID_PROFILE_ACTIVE");
}

#[tokio::test]
async fn workspace_delete_rejects_an_active_android_network_profile() {
    let fixture = running_vpn_fixture().await;

    let current = fixture.workspaces.get(fixture.original_id).await.unwrap();
    let delete_error = fixture
        .application
        .workspace_delete(current.id, current.revision.get())
        .await
        .expect_err("活动 VPN 方案所属 Workspace 不能删除");
    assert_eq!(
        delete_error.view_model.code,
        "WORKSPACE_ANDROID_NETWORK_ACTIVE"
    );
}

#[tokio::test]
async fn profile_delete_rejects_the_active_android_network_profile() {
    let fixture = running_vpn_fixture().await;
    fixture
        .workspaces
        .select(fixture.original_id)
        .await
        .unwrap();

    let error = fixture
        .application
        .device_network_profile_delete(fixture.profile_id.clone())
        .await
        .expect_err("活动设备网络方案不能删除");

    assert_eq!(error.view_model.code, "ANDROID_PROFILE_ACTIVE");
    assert!(
        fixture
            .application
            .device_network_profile_get(fixture.profile_id)
            .await
            .is_ok()
    );
}
