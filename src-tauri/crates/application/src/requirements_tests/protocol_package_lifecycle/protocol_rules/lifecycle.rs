use super::*;
use intercept_proxy_domain::MAX_JAVASCRIPT_SAFE_INTEGER;

#[tokio::test]
async fn compiler_description_identity_is_checked_for_capability_and_save() {
    let (application, services, workspaces, _) = fixture();
    let package = pkg("iso-8583", "1.0.0");
    let listener_id = configure_relay(&services, &workspaces, &package).await;
    services.set_description(
        package.clone(),
        description_with_blob(pkg("other", "2.0.0")),
    );
    assert_eq!(
        error_code(
            &application
                .protocol_rule_capabilities(listener_id, ProtocolRuleStage::ProxyToUpstream)
                .await
                .unwrap_err()
        ),
        "PROTOCOL_PACKAGE_DESCRIPTION_IDENTITY_MISMATCH"
    );
    assert_eq!(
        error_code(
            &application
                .protocol_rule_save(input(listener_id, package, ProtocolDirection::Upstream, 0))
                .await
                .unwrap_err()
        ),
        "PROTOCOL_PACKAGE_DESCRIPTION_IDENTITY_MISMATCH"
    );
}

#[tokio::test]
async fn deleting_an_imported_high_order_rule_never_reuses_its_created_order() {
    let (application, services, workspaces, _) = fixture();
    let package = pkg("iso-8583", "1.0.0");
    let listener_id = configure_relay(&services, &workspaces, &package).await;
    let selected = workspaces.list().await.unwrap().remove(0);
    let mut workspace = workspaces.get(selected.id).await.unwrap();
    let imported = ProtocolDocumentRuleDefinition::new(
        ProtocolDocumentRuleId::new(),
        true,
        0,
        10_000,
        listener_id,
        package.clone(),
        ProtocolDirection::Upstream,
        Vec::new(),
        vec![DocumentAction::RecordMatch],
    )
    .unwrap();
    workspace.rule_created_order_high_water = imported.created_order();
    workspace
        .replace_document_runtime_rules(vec![imported.clone()])
        .unwrap();
    workspaces.save(workspace).await.unwrap();

    application
        .protocol_rule_delete(imported.rule_id(), imported.revision().get(), true)
        .await
        .unwrap();
    let created = application
        .protocol_rule_save(input(listener_id, package, ProtocolDirection::Upstream, 0))
        .await
        .unwrap();

    assert!(created.created_order() > imported.created_order());
}

#[tokio::test]
async fn created_order_exhaustion_is_stable_and_does_not_write() {
    let (application, services, workspaces, _) = fixture();
    let package = pkg("iso-8583", "1.0.0");
    let listener_id = configure_relay(&services, &workspaces, &package).await;
    let selected = workspaces.list().await.unwrap().remove(0);
    let mut workspace = workspaces.get(selected.id).await.unwrap();
    workspace.rule_created_order_high_water = MAX_JAVASCRIPT_SAFE_INTEGER;
    let before = workspaces.save(workspace).await.unwrap();

    let error = application
        .protocol_rule_save(input(listener_id, package, ProtocolDirection::Upstream, 0))
        .await
        .unwrap_err();

    assert_eq!(error_code(&error), "PROTOCOL_RULE_CREATED_ORDER_EXHAUSTED");
    let after = workspaces.get(before.id).await.unwrap();
    assert_eq!(after.revision, before.revision);
    assert!(after.document_runtime_rules().unwrap().is_empty());
    assert_eq!(
        after.rule_created_order_high_water,
        MAX_JAVASCRIPT_SAFE_INTEGER
    );
}

#[tokio::test]
async fn listener_save_and_validate_fresh_compile_disabled_exact_package_and_identity_free_schema()
{
    let (application, services, workspaces, _) = fixture();
    let package = pkg("iso-8583", "1.0.0");
    let _listener_id = configure_relay(&services, &workspaces, &package).await;
    let selected = workspaces.list().await.unwrap().remove(0);
    let mut workspace = workspaces.get(selected.id).await.unwrap();
    let mut disabled = services.record(&package).unwrap();
    disabled.enabled = false;
    services.insert(disabled);

    application
        .listener_validate(
            workspace.id,
            workspace.revision.get(),
            workspace.listeners[0].clone(),
            Vec::new(),
        )
        .await
        .unwrap();
    workspace = application
        .listener_save(
            workspace.id,
            workspace.revision.get(),
            workspace.listeners[0].clone(),
            Vec::new(),
        )
        .await
        .unwrap();
    assert_eq!(services.compile_calls.load(Ordering::SeqCst), 2);
    assert_eq!(services.describe_calls.load(Ordering::SeqCst), 2);

    let mut incompatible = description_with_blob(package);
    let intercept_proxy_domain::DocumentSchemaNode::Object { properties, .. } =
        &mut incompatible.upstream_schema.root
    else {
        unreachable!()
    };
    properties.insert(
        "amount".into(),
        intercept_proxy_domain::DocumentSchemaNode::String { title: None },
    );
    services.set_description(incompatible.package.clone(), incompatible);
    let current = workspaces.get(workspace.id).await.unwrap();
    application
        .listener_validate(
            current.id,
            current.revision.get(),
            current.listeners[0].clone(),
            Vec::new(),
        )
        .await
        .unwrap();
}

#[tokio::test]
async fn listener_start_rechecks_revision_enabled_and_fresh_compilation() {
    let (application, services, workspaces, runtime) = fixture();
    let package = pkg("iso-8583", "1.0.0");
    let listener_id = configure_relay(&services, &workspaces, &package).await;
    let selected = workspaces.list().await.unwrap().remove(0);
    let workspace = workspaces.get(selected.id).await.unwrap();
    let get_calls_before_stale_start = services.get_calls.load(Ordering::SeqCst);

    let stale = application
        .listener_start(
            workspace.id,
            workspace.revision.get().saturating_sub(1),
            listener_id,
        )
        .await
        .unwrap_err();
    assert_eq!(error_code(&stale), "REVISION_CONFLICT");
    assert_eq!(
        services.get_calls.load(Ordering::SeqCst),
        get_calls_before_stale_start
    );

    let mut disabled = services.record(&package).unwrap();
    disabled.enabled = false;
    services.insert(disabled);
    let error = application
        .listener_start(workspace.id, workspace.revision.get(), listener_id)
        .await
        .unwrap_err();
    assert_eq!(error_code(&error), "PROTOCOL_PACKAGE_DISABLED");
    assert!(runtime.statuses().await.unwrap().is_empty());

    services.insert(record(package.clone(), true));
    services.set_description(package.clone(), description_with_blob(package.clone()));
    services.set_compilation_result(
        package.clone(),
        Ok(ProtocolPackageCompilationReceipt {
            package: package.clone(),
            host_api: 2,
            compatible: true,
        }),
    );
    let error = application
        .listener_start(workspace.id, workspace.revision.get(), listener_id)
        .await
        .unwrap_err();
    assert_eq!(error_code(&error), "PROTOCOL_PACKAGE_API_INCOMPATIBLE");
    assert!(runtime.statuses().await.unwrap().is_empty());
    assert!(!workspaces.get(workspace.id).await.unwrap().listeners[0].enabled);
}

#[tokio::test]
async fn active_scripted_listener_accepts_live_rule_changes_but_freezes_other_configuration() {
    let (application, services, workspaces, _) = fixture();
    let package = pkg("iso-8583", "1.0.0");
    let listener_id = configure_relay(&services, &workspaces, &package).await;
    let selected = workspaces.list().await.unwrap().remove(0);
    let workspace = workspaces.get(selected.id).await.unwrap();
    let rule = application
        .protocol_rule_save(input(
            listener_id,
            package.clone(),
            ProtocolDirection::Upstream,
            0,
        ))
        .await
        .unwrap();
    let workspace = workspaces.get(workspace.id).await.unwrap();

    application
        .listener_start(workspace.id, workspace.revision.get(), listener_id)
        .await
        .unwrap();
    let running = workspaces.get(workspace.id).await.unwrap();
    let mut edited = running.listeners[0].clone();
    edited.name.push_str(" changed");
    assert_eq!(
        error_code(
            &application
                .listener_save(running.id, running.revision.get(), edited, Vec::new(),)
                .await
                .unwrap_err()
        ),
        "LISTENER_RUNTIME_ACTIVE"
    );

    let added = application
        .protocol_rule_save(input(
            listener_id,
            package.clone(),
            ProtocolDirection::Upstream,
            1,
        ))
        .await
        .unwrap();
    let toggled = application
        .protocol_rule_toggle(rule.rule_id(), rule.revision().get(), false)
        .await
        .unwrap();
    assert!(!toggled.enabled());
    application
        .protocol_rule_delete(added.rule_id(), added.revision().get(), true)
        .await
        .unwrap();
    services.set_usages(
        package.clone(),
        vec![usage(
            workspace.id,
            listener_id,
            ListenerRuntimeState::Running,
        )],
    );
    assert_eq!(
        error_code(
            &application
                .protocol_package_disable(package.clone())
                .await
                .unwrap_err()
        ),
        "PROTOCOL_PACKAGE_RUNTIME_IN_USE"
    );

    let running = workspaces.get(workspace.id).await.unwrap();
    application
        .listener_stop(running.id, running.revision.get(), listener_id)
        .await
        .unwrap();
    services.set_usages(
        package.clone(),
        vec![usage(
            workspace.id,
            listener_id,
            ListenerRuntimeState::Stopped,
        )],
    );
    application.protocol_package_disable(package).await.unwrap();
}

#[derive(Debug, Default)]
struct FailFirstRuleReplacementRuntime {
    inner: InMemoryListenerRuntime,
    replacements: AtomicUsize,
}

#[async_trait::async_trait]
impl ListenerRuntimePort for FailFirstRuleReplacementRuntime {
    async fn statuses(&self) -> AppResult<Vec<ListenerStatusViewModel>> {
        self.inner.statuses().await
    }

    async fn start(
        &self,
        workspace: ProxyWorkspace,
        listener: ProxyListener,
    ) -> AppResult<ListenerStatusViewModel> {
        self.inner.start(workspace, listener).await
    }

    async fn stop(&self, listener_id: ListenerId) -> AppResult<ListenerStatusViewModel> {
        self.inner.stop(listener_id).await
    }

    async fn replace_rule_definitions(
        &self,
        workspace: ProxyWorkspace,
        listener_id: ListenerId,
    ) -> AppResult<()> {
        if self.replacements.fetch_add(1, Ordering::SeqCst) == 0 {
            return Err(AppError::new(
                "RULE_RUNTIME_REPLACE_FAILED",
                "injected replacement failure",
            ));
        }
        self.inner
            .replace_rule_definitions(workspace, listener_id)
            .await
    }

    async fn test_upstream_connection(
        &self,
        workspace: ProxyWorkspace,
        listener: ProxyListener,
    ) -> AppResult<ListenerUpstreamConnectionTestViewModel> {
        self.inner
            .test_upstream_connection(workspace, listener)
            .await
    }

    async fn test_upstream_tls(
        &self,
        workspace: ProxyWorkspace,
        listener: ProxyListener,
    ) -> AppResult<ListenerUpstreamTlsTestViewModel> {
        self.inner.test_upstream_tls(workspace, listener).await
    }
}

#[tokio::test]
async fn runtime_replacement_failure_restores_the_previous_persisted_rule_set() {
    let services = Arc::new(FakeProtocolPackageServices::default());
    let workspaces = Arc::new(InMemoryWorkspaceStore::default());
    let runtime = Arc::new(FailFirstRuleReplacementRuntime::default());
    let application = application_with_listener_runtime(
        Arc::clone(&services),
        Arc::clone(&workspaces),
        runtime.clone(),
    );
    let package = pkg("iso-8583", "1.0.0");
    let listener_id = configure_relay(&services, &workspaces, &package).await;
    let before = workspaces
        .get(workspaces.list().await.unwrap()[0].id)
        .await
        .unwrap();

    let error = application
        .protocol_rule_save(input(listener_id, package, ProtocolDirection::Upstream, 0))
        .await
        .unwrap_err();

    assert_eq!(error_code(&error), "RULE_RUNTIME_REPLACE_FAILED");
    let after = workspaces.get(before.id).await.unwrap();
    assert!(after.document_runtime_rules().unwrap().is_empty());
    assert_eq!(after.rule_created_order_high_water, 0);
    assert!(after.revision > before.revision);
    assert_eq!(runtime.replacements.load(Ordering::SeqCst), 2);
}
