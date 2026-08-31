use super::*;

/// HTTP Plain 与 Socket Direct 没有协议脚本边界。保存、校验和启动都不能因为应用装配了
/// 协议包服务就产生隐式查询，否则原样转发会错误依赖一个完全无关的注册表。
#[tokio::test]
async fn direct_listener_gates_never_touch_protocol_package_ports() {
    for data_plane in [
        ListenerDataPlane::Http(HttpListenerSettings::default()),
        ListenerDataPlane::Socket(SocketRelaySettings::relay(
            SocketEndpoint {
                host: "127.0.0.1".into(),
                port: 9_999,
            },
            SocketRelaySecurity::Transparent,
            10,
            SocketPayloadProcessing::Direct,
        )),
    ] {
        let (application, services, _, _) = fixture();
        let mut workspace = application.workspace_create("Direct".into()).await.unwrap();
        workspace.listeners[0].data_plane = data_plane;
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
        assert_eq!(services.describe_calls.load(Ordering::SeqCst), 0);
        assert_eq!(services.usage_calls.load(Ordering::SeqCst), 0);
    }
}

#[tokio::test]
async fn http_protocol_listener_save_requires_an_installed_http_package() {
    let (application, services, workspaces, _) = fixture();
    let package = pkg("http-json", "1.0.0");
    let listener_id = configure_http(&services, &workspaces, &package).await;
    let selected = workspaces.list().await.unwrap().remove(0);
    let workspace = workspaces.get(selected.id).await.unwrap();
    let listener = workspace
        .listeners
        .iter()
        .find(|listener| listener.id == listener_id)
        .unwrap()
        .clone();

    application
        .listener_validate(
            workspace.id,
            workspace.revision.get(),
            listener.clone(),
            Vec::new(),
        )
        .await
        .unwrap();
    application
        .listener_save(workspace.id, workspace.revision.get(), listener, Vec::new())
        .await
        .unwrap();
    assert_eq!(services.describe_calls.load(Ordering::SeqCst), 2);
}

#[tokio::test]
async fn http_protocol_listener_rejects_missing_and_socket_packages_before_save() {
    for expected_code in [
        "PROTOCOL_PACKAGE_NOT_FOUND",
        "PORTABLE_PROTOCOL_PACKAGE_INVALID",
    ] {
        let (application, services, workspaces, _) = fixture();
        let package = pkg("http-invalid", "1.0.0");
        if expected_code == "PORTABLE_PROTOCOL_PACKAGE_INVALID" {
            services.insert(record(package.clone(), true));
            services.set_description(package.clone(), description(package.clone()));
        }
        let mut workspace = application.workspace_create("HTTP".into()).await.unwrap();
        workspace.listeners[0].data_plane = ListenerDataPlane::Http(HttpListenerSettings {
            body_processing: HttpBodyProcessing::Protocol {
                package: package.clone(),
            },
            ..HttpListenerSettings::default()
        });
        let listener = workspace.listeners[0].clone();

        let validation_error = application
            .listener_validate(
                workspace.id,
                workspace.revision.get(),
                listener.clone(),
                Vec::new(),
            )
            .await
            .unwrap_err();
        assert_eq!(error_code(&validation_error), expected_code);
        let expected_field = if expected_code == "PROTOCOL_PACKAGE_NOT_FOUND" {
            "listener.data_plane.http.body_processing.package"
        } else {
            "listener.data_plane.http.body_processing"
        };
        assert!(
            validation_error
                .view_model
                .field_errors
                .contains_key(expected_field)
        );
        let save_error = application
            .listener_save(workspace.id, workspace.revision.get(), listener, Vec::new())
            .await
            .unwrap_err();
        assert_eq!(error_code(&save_error), expected_code);
        assert!(
            save_error
                .view_model
                .field_errors
                .contains_key(expected_field)
        );
        assert_eq!(
            workspaces.get(workspace.id).await.unwrap().revision,
            workspace.revision
        );
    }
}

#[tokio::test]
async fn listener_validation_accepts_current_unified_document_rule_shapes() {
    let (application, services, workspaces, _) = fixture();
    let package = pkg("unified-listener", "1.0.0");
    let listener_id = configure_relay(&services, &workspaces, &package).await;
    let selected = workspaces.list().await.unwrap().remove(0);
    let mut workspace = workspaces.get(selected.id).await.unwrap();
    workspace.rule_definitions =
        vec![
            intercept_proxy_domain::RuleDefinition::create(
                intercept_proxy_domain::RuleDefinitionDraft {
                    name: "listener unified rule".into(),
                    enabled: true,
                    priority: 0,
                    listener_id,
                    stage: intercept_proxy_domain::RuleStage::ProxyToUpstream,
                    one_shot: false,
                    content: intercept_proxy_domain::RuleContent::Socket(
                        intercept_proxy_domain::SocketRuleContent {
                            package: package.clone(),
                            condition: intercept_proxy_domain::ConditionTree::Any(vec![
                        intercept_proxy_domain::ConditionTree::Leaf(
                            intercept_proxy_domain::Condition::DocumentPattern {
                                path: intercept_proxy_domain::DocumentMatchPath::parse("/raw/*")
                                    .unwrap(),
                                predicate: intercept_proxy_domain::DocumentPredicate::Number(
                                    intercept_proxy_domain::NumberPredicate {
                                        operator: intercept_proxy_domain::NumberOperator::Equal,
                                        value: intercept_proxy_domain::DocumentNumber::new(7.0)
                                            .unwrap(),
                                    },
                                ),
                            },
                        ),
                        intercept_proxy_domain::ConditionTree::Leaf(
                            intercept_proxy_domain::Condition::NthHit { count: 2 },
                        ),
                    ]),
                            actions: vec![
                                intercept_proxy_domain::UnifiedAction::Document(
                                    intercept_proxy_domain::DocumentMutation::Insert {
                                        path: intercept_proxy_domain::JsonPointer::property("raw"),
                                        index: 0,
                                        value: intercept_proxy_domain::DocumentValue::integer(1)
                                            .unwrap(),
                                    },
                                ),
                                intercept_proxy_domain::UnifiedAction::Document(
                                    intercept_proxy_domain::DocumentMutation::Append {
                                        path: intercept_proxy_domain::JsonPointer::property("raw"),
                                        value: intercept_proxy_domain::DocumentValue::integer(2)
                                            .unwrap(),
                                    },
                                ),
                            ],
                        },
                    ),
                },
                1,
            )
            .unwrap(),
        ];
    workspace.rule_created_order_high_water = 1;
    workspace = workspaces.save(workspace).await.unwrap();
    let listener = workspace
        .listeners
        .iter()
        .find(|listener| listener.id == listener_id)
        .unwrap()
        .clone();

    let validation = application
        .listener_validate(workspace.id, workspace.revision.get(), listener, Vec::new())
        .await
        .expect("current unified rule shapes must pass listener package validation");

    assert!(validation.valid);
}

/// `LocalResponder` 启动前仍读取精确外部包描述；通过门禁后与其他 Listener 一样
/// 进入 runtime，并把持久化 enabled 状态与实际运行态一起提交。
#[tokio::test]
async fn local_responder_reaches_runtime_after_fresh_package_description() {
    let (application, services, workspaces, runtime) = fixture();
    let package = pkg("iso-8583", "1.0.0");
    let listener_id = configure_relay(&services, &workspaces, &package).await;
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
    let before_start_describe_calls = services.describe_calls.load(Ordering::SeqCst);
    let status = application
        .listener_start(workspace.id, workspace.revision.get(), listener_id)
        .await
        .unwrap();

    assert_eq!(status.state, ListenerRuntimeState::Running);
    assert_eq!(
        services.describe_calls.load(Ordering::SeqCst),
        before_start_describe_calls + 1
    );
    assert_eq!(runtime.statuses().await.unwrap(), vec![status]);
    assert!(workspaces.get(workspace.id).await.unwrap().listeners[0].enabled);
}
