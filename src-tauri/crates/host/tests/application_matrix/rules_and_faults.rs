#[tokio::test]
async fn production_host_covers_rule_and_fault_lifecycle_without_ui() {
    let temp = tempfile::tempdir().expect("temporary rule host");
    let host = ApplicationHostBuilder::new(temp.path(), test_platform(), Arc::new(TestProfile))
        .build()
        .await
        .expect("build UI-neutral host");
    let application = host.application();
    let bootstrap = application
        .app_bootstrap()
        .await
        .expect("load selected Workspace listener catalog");
    let workspace_channel = bootstrap
        .channel_catalog
        .first()
        .expect("default Workspace listener")
        .id
        .clone();

    let listener_id = intercept_proxy_application::ListenerId::from_uuid(
        uuid::Uuid::parse_str(workspace_channel.as_str()).expect("listener UUID channel"),
    );
    exercise_unified_rule_lifecycle(&application, listener_id).await;

    let templates = application
        .fault_template_list()
        .await
        .expect("list fault templates");
    assert_eq!(templates.len(), 1, "product catalog filters capabilities");
    assert_eq!(templates[0].name, "Test delay");
    let template = templates
        .iter()
        .find(|template| template.template_id == "request_delay")
        .expect("request delay template");
    assert_eq!(template.default_channel, workspace_channel);
    let active_fault = application
        .fault_configure(FaultConfigurationDraft {
            template_id: template.template_id.clone(),
            existing_rule_id: None,
            expected_revision: None,
            channel: Some(template.default_channel.clone()),
            terminal: Some("10.0.0.8".into()),
            target: Some("/".into()),
            nth_hit: Some(template.default_nth_hit),
            one_shot: template.default_one_shot,
            priority: template.default_priority,
            parameters: template.default_parameters.clone(),
        })
        .await
        .expect("configure fault through real rule repository");
    assert!(active_fault.enabled);
    let unified_faults = application
        .rule_definition_list()
        .await
        .expect("list unified rules after configuring fault");
    let unified_fault = unified_faults
        .iter()
        .find(|rule| rule.rule_id().as_uuid() == active_fault.rule_id)
        .expect("fault is persisted through the unified rule collection");
    assert_eq!(unified_fault.revision().get(), active_fault.revision);
    assert!(unified_fault.enabled());
    assert_eq!(
        application
            .fault_active_list()
            .await
            .expect("list active faults")
            .len(),
        1
    );
    let stopped_fault = application
        .fault_stop(active_fault.rule_id, active_fault.revision, true)
        .await
        .expect("stop active fault");
    assert!(!stopped_fault.enabled);
    let stopped_unified_faults = application
        .rule_definition_list()
        .await
        .expect("list unified rules after stopping fault");
    let stopped_unified_fault = stopped_unified_faults
        .iter()
        .find(|rule| rule.rule_id().as_uuid() == stopped_fault.rule_id)
        .expect("stopped fault remains the same unified rule");
    assert!(!stopped_unified_fault.enabled());
    assert_eq!(
        stopped_unified_fault.revision().get(),
        stopped_fault.revision
    );

    host.shutdown().await.expect("shutdown UI-neutral host");
}

async fn exercise_unified_rule_lifecycle(
    application: &intercept_proxy_application::Application,
    listener_id: intercept_proxy_application::ListenerId,
) {
    let editor_context = application
        .rule_editor_context(listener_id)
        .await
        .expect("load Rust-owned unified rule context");
    let RuleEditorContentContext::Http { stages } = editor_context.content else {
        panic!("HTTP rule context expected");
    };
    let mut input = stages
        .into_iter()
        .find(|stage| stage.stage == RuleStage::ProxyToUpstream)
        .expect("proxy-to-upstream rule stage")
        .new_rule_draft;
    input.draft.name = "无 UI 集成规则".into();
    let RuleContent::Http(http_content) = &mut input.draft.content else {
        panic!("HTTP rule draft expected");
    };
    http_content.description = "Application facade matrix".into();
    http_content.condition = intercept_proxy_application::ConditionTree::Leaf(
        application
            .rule_definition_condition_draft(
                intercept_proxy_application::RuleConditionKind::NthHit,
                MessageStage::Request,
            )
            .expect("Rust-owned NthHit condition"),
    );
    http_content.actions = vec![intercept_proxy_application::UnifiedAction::from(application
        .rule_definition_action_draft(RuleActionKind::Delay, MessageStage::Request)
        .expect("Rust-owned delay action"))];
    let saved_rule = application
        .rule_definition_save(input)
        .await
        .expect("save unified rule");
    let rule_id = saved_rule.rule_id();
    assert_eq!(
        application
            .rule_definition_list()
            .await
            .expect("list rules")
            .len(),
        1
    );
    assert_eq!(
        application
            .rule_definition_get(rule_id)
            .await
            .expect("get saved rule")
            .name(),
        "无 UI 集成规则"
    );

    let disabled = application
        .rule_definition_toggle(rule_id, saved_rule.revision(), false)
        .await
        .expect("disable rule");
    assert!(!disabled.enabled());
    let copied = application
        .rule_definition_copy(rule_id)
        .await
        .expect("copy rule");
    assert_ne!(copied.rule_id(), rule_id);
    let deleted = application
        .rule_definition_delete(rule_id, disabled.revision(), true)
        .await
        .expect("delete original rule");
    assert!(deleted.success);
}

// ARCH-007~009, CERT-001~020, TEST-HOST:
// certificate generation and validation use the production protected store.
