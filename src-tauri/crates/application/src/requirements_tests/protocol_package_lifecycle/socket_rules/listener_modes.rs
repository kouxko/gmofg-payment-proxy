use super::*;

/// HTTP/Direct Listener 没有协议脚本边界。保存、校验和启动都不能因为应用装配了协议包
/// 服务就产生隐式查询，否则透明转发会错误依赖一个完全无关的注册表。
#[tokio::test]
async fn direct_listener_gates_never_touch_protocol_package_ports() {
    let (application, services, _, _) = fixture();
    let mut workspace = application.workspace_create("Direct".into()).await.unwrap();
    let listener = workspace.listeners[0].clone();

    application
        .listener_validate(
            workspace.id,
            workspace.revision.get(),
            listener.clone(),
            Vec::new(),
        )
        .await
        .unwrap();
    workspace = application
        .listener_save(
            workspace.id,
            workspace.revision.get(),
            listener.clone(),
            Vec::new(),
        )
        .await
        .unwrap();
    application
        .listener_start(workspace.id, workspace.revision.get(), listener.id)
        .await
        .unwrap();

    assert_eq!(services.get_calls.load(Ordering::SeqCst), 0);
    assert_eq!(services.compile_calls.load(Ordering::SeqCst), 0);
    assert_eq!(services.describe_calls.load(Ordering::SeqCst), 0);
    assert_eq!(services.usage_calls.load(Ordering::SeqCst), 0);
}

/// T21 移除的只是 Application Facade 的一律 unavailable。当前内存 runtime 仍代表尚未
/// 接入 T22 的 Host，因此最终返回同一稳定错误；fresh compile 调用数证明请求已经穿过
/// Application 的包/Schema/方向门禁并真正到达 runtime。
#[tokio::test]
async fn local_responder_reaches_runtime_after_fresh_script_validation() {
    let (application, services, workspaces, runtime) = fixture();
    let package = pkg("iso-8583", "1.0.0");
    let listener_id = configure_relay(
        &services,
        &workspaces,
        &package,
        DirectionProcessingOptions {
            decode_enabled: true,
            encode_enabled: false,
        },
        DirectionProcessingOptions {
            decode_enabled: false,
            encode_enabled: true,
        },
    )
    .await;
    let mut description = description_with_blob(package.clone());
    description.capabilities.downstream.encode = true;
    services.set_description(package, description);
    let selected = workspaces.list().await.unwrap().remove(0);
    let mut workspace = workspaces.get(selected.id).await.unwrap();
    let ListenerDataPlane::Socket(settings) = &mut workspace.listeners[0].data_plane else {
        unreachable!()
    };
    settings.topology = SocketTopology::LocalResponder(SocketLocalResponderTopology::default());
    workspace = application
        .listener_save(
            workspace.id,
            workspace.revision.get(),
            workspace.listeners[0].clone(),
            Vec::new(),
        )
        .await
        .unwrap();
    let before_start_compile_calls = services.compile_calls.load(Ordering::SeqCst);
    let error = application
        .listener_start(workspace.id, workspace.revision.get(), listener_id)
        .await
        .unwrap_err();

    assert_eq!(error_code(&error), "LOCAL_RESPONDER_NOT_AVAILABLE");
    assert_eq!(
        error.view_model.message,
        "LocalResponder 数据面尚未接入当前运行时。"
    );
    assert_eq!(
        services.compile_calls.load(Ordering::SeqCst),
        before_start_compile_calls + 1
    );
    assert!(runtime.statuses().await.unwrap().is_empty());
    assert!(!workspaces.get(workspace.id).await.unwrap().listeners[0].enabled);
}
