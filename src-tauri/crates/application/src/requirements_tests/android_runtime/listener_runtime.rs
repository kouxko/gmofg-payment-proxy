#[tokio::test]
async fn device_network_start_rejects_enabled_listener_without_running_runtime() {
    for listener_state in [None, Some(ListenerRuntimeState::Stopped)] {
        let fixture = running_vpn_fixture_with_listener_state(false, listener_state).await;
        fixture
            .workspaces
            .select(fixture.original_id)
            .await
            .unwrap();

        let error = fixture
            .application
            .device_network_start(fixture.serial.clone(), fixture.profile_id.clone(), false)
            .await
            .expect_err("enabled listener without running runtime must block start");

        assert_eq!(error.view_model.code, "ANDROID_PROXY_LISTENER_NOT_RUNNING");
        assert_eq!(
            error.view_model.suggested_action.as_deref(),
            Some("请先启动对应代理入口，确认状态为“运行中”后重试。")
        );
        assert_eq!(
            fixture.android.network_start_calls.load(Ordering::SeqCst),
            0
        );
    }
}

#[tokio::test]
async fn device_network_apply_rejects_enabled_listener_without_running_runtime() {
    for listener_state in [None, Some(ListenerRuntimeState::Stopped)] {
        let fixture = running_vpn_fixture_with_listener_state(false, listener_state).await;
        fixture
            .workspaces
            .select(fixture.original_id)
            .await
            .unwrap();

        let error = fixture
            .application
            .device_network_apply(
                fixture.serial.clone(),
                fixture.runtime_epoch,
                fixture.profile_id.clone(),
                false,
            )
            .await
            .expect_err("enabled listener without running runtime must block apply");

        assert_eq!(error.view_model.code, "ANDROID_PROXY_LISTENER_NOT_RUNNING");
        assert_eq!(
            error.view_model.suggested_action.as_deref(),
            Some("请先启动对应代理入口，确认状态为“运行中”后重试。")
        );
        assert_eq!(
            fixture.android.network_apply_calls.load(Ordering::SeqCst),
            0
        );
    }
}

#[tokio::test]
async fn device_network_start_and_apply_accept_running_listener_runtime() {
    let fixture =
        running_vpn_fixture_with_listener_state(false, Some(ListenerRuntimeState::Running)).await;
    fixture
        .workspaces
        .select(fixture.original_id)
        .await
        .unwrap();

    fixture
        .application
        .device_network_start(fixture.serial.clone(), fixture.profile_id.clone(), false)
        .await
        .unwrap();
    fixture
        .application
        .device_network_apply(
            fixture.serial.clone(),
            fixture.runtime_epoch,
            fixture.profile_id.clone(),
            false,
        )
        .await
        .unwrap();

    assert_eq!(
        fixture.android.network_start_calls.load(Ordering::SeqCst),
        1
    );
    assert_eq!(
        fixture.android.network_apply_calls.load(Ordering::SeqCst),
        1
    );
}

#[tokio::test]
async fn device_network_start_rejects_listener_unreachable_from_adb_reverse() {
    let fixture =
        running_vpn_fixture_with_listener_state(false, Some(ListenerRuntimeState::Running)).await;
    fixture
        .workspaces
        .select(fixture.original_id)
        .await
        .unwrap();

    let mut workspace = fixture.workspaces.get(fixture.original_id).await.unwrap();
    workspace.listeners[0].bind_address = "192.0.2.10".into();
    workspace.listeners[0].http_mut().unwrap().authentication =
        intercept_proxy_domain::ForwardProxyAuthentication::Basic {
            credential: intercept_proxy_domain::SecretReference {
                provider: "test".into(),
                key: "credential".into(),
            },
        };
    fixture.workspaces.save(workspace).await.unwrap();

    let error = fixture
        .application
        .device_network_start(fixture.serial.clone(), fixture.profile_id.clone(), false)
        .await
        .expect_err("ADB reverse 只能连接本机回环或未指定地址上的监听");

    assert_eq!(
        error.view_model.code,
        "ANDROID_PROXY_LISTENER_BIND_UNREACHABLE"
    );
    assert_eq!(
        fixture.android.network_start_calls.load(Ordering::SeqCst),
        0
    );
}
