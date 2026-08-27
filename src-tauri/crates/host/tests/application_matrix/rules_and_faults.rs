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
    let mut rule = application
        .rule_new_http_draft(listener_id)
        .await
        .expect("create Rust-owned rule draft");
    rule.name = "无 UI 集成规则".into();
    rule.description = "Application facade matrix".into();
    rule.actions = vec![RuleAction::Delay { milliseconds: 5 }];
    let saved_rule = application.rule_save(rule).await.expect("save rule");
    let rule_id = saved_rule.summary.rule_id;
    assert_eq!(application.rule_list().await.expect("list rules").len(), 1);
    assert_eq!(
        application
            .rule_get(rule_id)
            .await
            .expect("get saved rule")
            .summary
            .name,
        "无 UI 集成规则"
    );

    let disabled = application
        .rule_toggle(rule_id, saved_rule.summary.revision, false)
        .await
        .expect("disable rule");
    assert!(!disabled.summary.enabled);
    let copied = application.rule_copy(rule_id).await.expect("copy rule");
    assert_ne!(copied.summary.rule_id, rule_id);
    let deleted = application
        .rule_delete(rule_id, disabled.summary.revision, true)
        .await
        .expect("delete original rule");
    assert!(deleted.success);

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

    host.shutdown().await.expect("shutdown UI-neutral host");
}

// ARCH-007~009, CERT-001~020, TEST-HOST:
// certificate generation and validation use the production protected store.
