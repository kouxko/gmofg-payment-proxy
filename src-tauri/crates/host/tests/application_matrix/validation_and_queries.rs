#[tokio::test]
async fn invalid_product_profile_fails_before_storage_is_opened() {
    let temp = tempfile::tempdir().expect("temporary invalid product host");
    let error = ApplicationHostBuilder::new(
        temp.path(),
        test_platform(),
        Arc::new(InvalidProfile::default()),
    )
    .build()
    .await
    .expect_err("duplicate channel profile must fail");
    assert!(matches!(
        error,
        intercept_proxy_host::HostBuildError::InvalidProductProfile(_)
    ));
    assert!(!temp.path().join("generic-test.sqlite3").exists());
}

#[tokio::test]
async fn production_host_covers_queries_and_settings_without_ui() {
    intercept_proxy_product_api::validate_product_profile(&TestProfile)
        .expect("generic three-channel test profile");
    let temp = tempfile::tempdir().expect("temporary application host");
    let host = ApplicationHostBuilder::new(temp.path(), test_platform(), Arc::new(TestProfile))
        .build()
        .await
        .expect("build UI-neutral host");
    let application = host.application();
    assert!(temp.path().join("generic-test.sqlite3").is_file());
    let bootstrap = application
        .app_bootstrap()
        .await
        .expect("generic three-channel bootstrap");
    assert_eq!(bootstrap.channel_catalog.len(), 1);
    assert_eq!(bootstrap.channel_catalog[0].display_name, "默认代理监听");
    assert!(uuid::Uuid::parse_str(bootstrap.channel_catalog[0].id.as_str()).is_ok());
    let capture = application
        .capture_query(capture_query())
        .await
        .expect("query empty capture");
    assert!(capture.rows.is_empty());
    assert_eq!(capture.page, 1);
    assert_eq!(capture.page_size, 1);

    let sessions = application
        .session_query(session_query())
        .await
        .expect("query empty sessions");
    assert!(sessions.items.is_empty());
    assert_eq!(sessions.total, 0);

    let settings = valid_settings();
    let validation = application
        .settings_validate(settings.clone())
        .await
        .expect("validate settings before certificate setup");
    assert!(validation.valid, "{validation:#?}");
    // 通用化后，证书是否就绪属于各 Listener/Workspace 的局部约束，
    // 全局设置校验不应再用已删除的旧产品证书状态阻止保存。
    assert!(
        validation
            .warnings
            .iter()
            .all(|warning| !warning.contains("证书配置尚未就绪")),
        "global settings unexpectedly retained the legacy certificate warning: {validation:#?}"
    );
    let saved_settings = application
        .settings_save(settings.clone())
        .await
        .expect("save settings through application");
    assert_eq!(saved_settings.revision, 1);

    let confirmation = application
        .settings_reset_defaults(false)
        .await
        .expect_err("reset defaults requires confirmation");
    assert_eq!(confirmation.view_model.code, "CONFIRMATION_REQUIRED");
    let defaults = application
        .settings_reset_defaults(true)
        .await
        .expect("return Rust-owned defaults");
    assert_eq!(
        defaults
            .channels
            .iter()
            .map(|channel| channel.id.as_str())
            .collect::<Vec<_>>(),
        vec!["alpha", "beta", "gamma"]
    );
    assert_eq!(defaults.channels[0].port, 21_001);

    host.shutdown().await.expect("shutdown UI-neutral host");
}

// ARCH-007~009, RULE-001~017, FAULT-001~011, TEST-HOST:
// rule and fault CRUD use the production SQLite repository and domain validation.
