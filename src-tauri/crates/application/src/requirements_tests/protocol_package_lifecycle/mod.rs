//! T14：协议包生命周期应用用例的可执行需求。

use super::*;

mod support;
use support::*;
mod queries;

fn fixture() -> (
    Application,
    Arc<FakeProtocolPackageServices>,
    Arc<InMemoryWorkspaceStore>,
    Arc<InMemoryListenerRuntime>,
) {
    let services = Arc::new(FakeProtocolPackageServices::default());
    let workspaces = Arc::new(InMemoryWorkspaceStore::default());
    let runtime = Arc::new(InMemoryListenerRuntime::default());
    (
        application(services.clone(), workspaces.clone(), runtime.clone()),
        services,
        workspaces,
        runtime,
    )
}

#[tokio::test]
async fn enable_recompiles_every_time_before_writing_the_enabled_bit() {
    let (application, services, _, _) = fixture();
    let target = package("iso-8583", "1.0.0");
    services.insert(record(target.clone(), false));

    let first = application
        .protocol_package_enable(target.clone())
        .await
        .unwrap();
    let second = application
        .protocol_package_enable(target.clone())
        .await
        .unwrap();

    assert!(first.enabled && second.enabled);
    assert_eq!(services.compile_calls.load(Ordering::SeqCst), 2);
    assert_eq!(services.set_enabled_calls.load(Ordering::SeqCst), 2);
    assert!(services.record(&target).unwrap().enabled);
    assert!(
        services
            .exact_calls
            .lock()
            .iter()
            .all(|item| item == &target)
    );
}

#[tokio::test]
async fn enable_rejects_incompatible_or_wrong_version_receipts_without_mutation() {
    let (application, services, _, _) = fixture();
    let target = package("iso-8583", "1.0.0");
    let other = package("iso-8583", "2.0.0");
    services.insert(record(target.clone(), false));

    for receipt in [
        ProtocolPackageCompilationReceipt {
            package: target.clone(),
            host_api: 1,
            compatible: false,
        },
        ProtocolPackageCompilationReceipt {
            package: other,
            host_api: 1,
            compatible: true,
        },
        ProtocolPackageCompilationReceipt {
            package: target.clone(),
            host_api: 2,
            compatible: true,
        },
    ] {
        services.set_compilation_result(target.clone(), Ok(receipt));
        let error = application
            .protocol_package_enable(target.clone())
            .await
            .unwrap_err();
        assert_eq!(error_code(&error), "PROTOCOL_PACKAGE_API_INCOMPATIBLE");
        assert!(!services.record(&target).unwrap().enabled);
    }
    assert_eq!(services.set_enabled_calls.load(Ordering::SeqCst), 0);
}

#[tokio::test]
async fn enable_failure_paths_never_partially_enable_the_package() {
    let (application, services, _, _) = fixture();
    let target = package("tlv", "1.0.0");
    services.insert(record(target.clone(), false));

    services.failures.lock().get = Some(AppError::new("STORE_READ_FAILED", "read"));
    assert_eq!(
        error_code(
            &application
                .protocol_package_enable(target.clone())
                .await
                .unwrap_err()
        ),
        "STORE_READ_FAILED"
    );
    assert_eq!(services.compile_calls.load(Ordering::SeqCst), 0);
    services.failures.lock().get = None;

    services.failures.lock().compile = Some(AppError::new("SCRIPT_INVALID", "compile"));
    assert_eq!(
        error_code(
            &application
                .protocol_package_enable(target.clone())
                .await
                .unwrap_err()
        ),
        "SCRIPT_INVALID"
    );
    assert_eq!(services.set_enabled_calls.load(Ordering::SeqCst), 0);
    services.failures.lock().compile = None;

    services.failures.lock().set_enabled = Some(AppError::new("STORE_WRITE_FAILED", "write"));
    assert_eq!(
        error_code(
            &application
                .protocol_package_enable(target.clone())
                .await
                .unwrap_err()
        ),
        "STORE_WRITE_FAILED"
    );
    assert!(!services.record(&target).unwrap().enabled);
}

#[tokio::test]
async fn disable_blocks_every_non_stopped_runtime_state_and_allows_stopped_references() {
    let active_states = [
        ListenerRuntimeState::Starting,
        ListenerRuntimeState::Running,
        ListenerRuntimeState::Stopping,
        ListenerRuntimeState::Faulted,
    ];
    for state in active_states {
        let (application, services, _, _) = fixture();
        let target = package("iso-8583", "1.0.0");
        services.insert(record(target.clone(), true));
        services.set_usages(
            target.clone(),
            vec![usage(WorkspaceId::new(), ListenerId::new(), state)],
        );
        let error = application
            .protocol_package_disable(target.clone())
            .await
            .unwrap_err();
        assert_eq!(error_code(&error), "PROTOCOL_PACKAGE_RUNTIME_IN_USE");
        assert!(services.record(&target).unwrap().enabled);
        assert_eq!(services.set_enabled_calls.load(Ordering::SeqCst), 0);
    }

    let (application, services, _, _) = fixture();
    let target = package("iso-8583", "1.0.0");
    services.insert(record(target.clone(), true));
    let stopped = usage(
        WorkspaceId::new(),
        ListenerId::new(),
        ListenerRuntimeState::Stopped,
    );
    services.set_usages(target.clone(), vec![stopped.clone()]);
    let result = application
        .protocol_package_disable(target.clone())
        .await
        .unwrap();
    assert!(!result.enabled);
    assert_eq!(services.usages(&target), vec![stopped]);
}

#[tokio::test]
async fn disable_and_delete_match_only_the_exact_id_and_version() {
    let (application, services, _, _) = fixture();
    let target = package("iso-8583", "2.0.0");
    let same_id_other_version = package("iso-8583", "1.0.0");
    let same_version_other_id = package("tlv", "2.0.0");
    for item in [
        target.clone(),
        same_id_other_version.clone(),
        same_version_other_id.clone(),
    ] {
        services.insert(record(item, true));
    }
    services.set_usages(
        same_id_other_version,
        vec![usage(
            WorkspaceId::new(),
            ListenerId::new(),
            ListenerRuntimeState::Running,
        )],
    );
    services.set_usages(
        same_version_other_id,
        vec![usage(
            WorkspaceId::new(),
            ListenerId::new(),
            ListenerRuntimeState::Stopped,
        )],
    );

    application
        .protocol_package_disable(target.clone())
        .await
        .unwrap();
    application
        .protocol_package_delete(target.clone())
        .await
        .unwrap();
    assert!(services.record(&target).is_none());
}

#[tokio::test]
async fn every_saved_reference_blocks_delete_regardless_of_listener_state() {
    for state in [
        ListenerRuntimeState::Stopped,
        ListenerRuntimeState::Starting,
        ListenerRuntimeState::Running,
        ListenerRuntimeState::Stopping,
        ListenerRuntimeState::Faulted,
    ] {
        let (application, services, _, _) = fixture();
        let target = package("tlv", "1.0.0");
        services.insert(record(target.clone(), false));
        services.set_usages(
            target.clone(),
            vec![usage(WorkspaceId::new(), ListenerId::new(), state)],
        );
        let error = application
            .protocol_package_delete(target.clone())
            .await
            .unwrap_err();
        assert_eq!(error_code(&error), "PROTOCOL_PACKAGE_REFERENCE_IN_USE");
        assert!(services.record(&target).is_some());
        assert_eq!(services.delete_calls.load(Ordering::SeqCst), 0);
    }
}

#[tokio::test]
async fn an_unreferenced_enabled_version_can_be_deleted_without_forced_disable() {
    let (application, services, _, _) = fixture();
    let target = package("tlv", "1.0.0");
    services.insert(record(target.clone(), true));

    let result = application
        .protocol_package_delete(target.clone())
        .await
        .unwrap();
    assert!(result.success);
    assert_eq!(result.entity_id.as_deref(), Some("tlv@1.0.0"));
    assert!(services.record(&target).is_none());
}

#[tokio::test]
async fn lifecycle_commands_reject_a_missing_exact_version_before_other_ports() {
    let (application, services, _, _) = fixture();
    let missing = package("iso-8583", "9.0.0");

    for error in [
        application
            .protocol_package_enable(missing.clone())
            .await
            .unwrap_err(),
        application
            .protocol_package_disable(missing.clone())
            .await
            .unwrap_err(),
        application
            .protocol_package_delete(missing.clone())
            .await
            .unwrap_err(),
    ] {
        assert_eq!(error_code(&error), "PROTOCOL_PACKAGE_NOT_FOUND");
        assert_eq!(
            error.view_model.entity_id.as_deref(),
            Some("iso-8583@9.0.0")
        );
    }
    assert_eq!(services.compile_calls.load(Ordering::SeqCst), 0);
    assert_eq!(services.usage_calls.load(Ordering::SeqCst), 0);
    assert_eq!(services.set_enabled_calls.load(Ordering::SeqCst), 0);
    assert_eq!(services.delete_calls.load(Ordering::SeqCst), 0);
}

#[tokio::test]
async fn commands_recheck_usage_after_an_earlier_detail_snapshot() {
    let (application, services, _, _) = fixture();
    let target = package("iso-8583", "1.0.0");
    services.insert(record(target.clone(), true));
    services.push_usage_response(Ok(Vec::new()));
    let detail = application
        .protocol_package_detail(target.clone())
        .await
        .unwrap();
    assert!(detail.usages.is_empty());

    services.set_usages(
        target.clone(),
        vec![usage(
            WorkspaceId::new(),
            ListenerId::new(),
            ListenerRuntimeState::Running,
        )],
    );
    let disable = application
        .protocol_package_disable(target.clone())
        .await
        .unwrap_err();
    assert_eq!(error_code(&disable), "PROTOCOL_PACKAGE_RUNTIME_IN_USE");
    let delete = application
        .protocol_package_delete(target.clone())
        .await
        .unwrap_err();
    assert_eq!(error_code(&delete), "PROTOCOL_PACKAGE_REFERENCE_IN_USE");
    assert_eq!(services.usage_calls.load(Ordering::SeqCst), 3);
    assert_eq!(services.set_enabled_calls.load(Ordering::SeqCst), 0);
    assert_eq!(services.delete_calls.load(Ordering::SeqCst), 0);
}

#[tokio::test]
async fn protocol_mutation_holds_the_shared_gate_through_usage_check_and_store_write() {
    let (application, services, _, _) = fixture();
    let application = Arc::new(application);
    let target = package("tlv", "1.0.0");
    services.insert(record(target.clone(), true));
    services.block_usage.store(true, Ordering::SeqCst);
    let mut workspace = application
        .workspace_create("Concurrent".into())
        .await
        .unwrap();

    let deletion = {
        let application = application.clone();
        tokio::spawn(async move { application.protocol_package_delete(target).await })
    };
    services.usage_entered.notified().await;

    workspace.name = "Must wait".into();
    let mut save = {
        let application = application.clone();
        tokio::spawn(async move { application.workspace_save(workspace).await })
    };
    assert!(
        tokio::time::timeout(Duration::from_millis(20), &mut save)
            .await
            .is_err(),
        "Workspace mutation must wait for the lifecycle usage check and delete"
    );

    services.continue_usage.notify_one();
    deletion.await.unwrap().unwrap();
    let saved = save.await.unwrap().unwrap();
    assert_eq!(saved.name, "Must wait");
}

#[tokio::test]
async fn usage_and_store_failures_propagate_without_partial_state() {
    let (application, services, _, _) = fixture();
    let target = package("tlv", "1.0.0");
    services.insert(record(target.clone(), true));
    services.failures.lock().usage = Some(AppError::new("USAGE_QUERY_FAILED", "usage"));

    for error in [
        application
            .protocol_package_disable(target.clone())
            .await
            .unwrap_err(),
        application
            .protocol_package_delete(target.clone())
            .await
            .unwrap_err(),
    ] {
        assert_eq!(error_code(&error), "USAGE_QUERY_FAILED");
    }
    assert!(services.record(&target).unwrap().enabled);
    assert_eq!(services.set_enabled_calls.load(Ordering::SeqCst), 0);
    assert_eq!(services.delete_calls.load(Ordering::SeqCst), 0);

    services.failures.lock().usage = None;
    services.failures.lock().set_enabled = Some(AppError::new("STORE_WRITE_FAILED", "write"));
    assert_eq!(
        error_code(
            &application
                .protocol_package_disable(target.clone())
                .await
                .unwrap_err()
        ),
        "STORE_WRITE_FAILED"
    );
    assert!(services.record(&target).unwrap().enabled);

    services.failures.lock().set_enabled = None;
    services.failures.lock().delete = Some(AppError::new("STORE_DELETE_FAILED", "delete"));
    assert_eq!(
        error_code(
            &application
                .protocol_package_delete(target.clone())
                .await
                .unwrap_err()
        ),
        "STORE_DELETE_FAILED"
    );
    assert!(services.record(&target).is_some());
}

#[tokio::test]
async fn disabled_or_invalid_scripted_package_is_rejected_before_listener_runtime_start() {
    let (application, services, _, runtime) = fixture();
    let target = package("iso-8583", "1.0.0");
    services.insert(record(target.clone(), false));
    let mut workspace = application.workspace_create("Socket".into()).await.unwrap();
    workspace.listeners[0].data_plane = ListenerDataPlane::Socket(SocketRelaySettings::relay(
        SocketEndpoint {
            host: "127.0.0.1".into(),
            port: 9_999,
        },
        SocketRelaySecurity::Transparent,
        10,
        SocketPayloadProcessing::Scripted(ScriptedSocketProcessing {
            package: target.clone(),
            upstream: DirectionProcessingOptions::default(),
            downstream: DirectionProcessingOptions::default(),
        }),
    ));
    workspace = application.workspace_save(workspace).await.unwrap();

    let error = application
        .listener_start(
            workspace.id,
            workspace.revision.get(),
            workspace.listeners[0].id,
        )
        .await
        .unwrap_err();
    assert_eq!(error_code(&error), "PROTOCOL_PACKAGE_DISABLED");
    assert!(runtime.statuses().await.unwrap().is_empty());

    let mut invalid = services.record(&target).unwrap();
    invalid.enabled = true;
    invalid.validation = ProtocolPackageValidationViewModel::Invalid {
        code: "SCRIPT_INVALID".into(),
    };
    services.insert(invalid);
    let error = application
        .listener_start(
            workspace.id,
            workspace.revision.get(),
            workspace.listeners[0].id,
        )
        .await
        .unwrap_err();
    assert_eq!(error_code(&error), "PROTOCOL_PACKAGE_INVALID");
    assert!(runtime.statuses().await.unwrap().is_empty());
}
