#[tokio::test]
async fn production_host_covers_certificate_overview_and_validation_without_ui() {
    let temp = tempfile::tempdir().expect("temporary certificate host");
    let host = ApplicationHostBuilder::new(
        temp.path(),
        test_platform(),
        Arc::new(InterceptProxyProfile),
    )
    .build()
    .await
    .expect("build UI-neutral host");
    let application = host.application();
    let bootstrap = application
        .app_bootstrap()
        .await
        .expect("generic bootstrap");
    assert_eq!(bootstrap.channel_catalog.len(), 1);
    assert_eq!(bootstrap.channel_catalog[0].display_name, "默认代理监听");
    assert!(uuid::Uuid::parse_str(bootstrap.channel_catalog[0].id.as_str()).is_ok());

    let templates = application
        .fault_template_list()
        .await
        .expect("generic fault templates");
    assert_eq!(
        templates.len(),
        intercept_proxy_product_api::STANDARD_FAULT_CAPABILITY_IDS.len()
    );
    assert!(templates.iter().all(|template| {
        template.default_channel == bootstrap.channel_catalog[0].id
            && !template.name.trim().is_empty()
    }));

    let generated = application
        .certificate_overview()
        .await
        .expect("query initial certificate overview");
    assert!(generated.ready);
    assert!(!generated.can_initialize);
    assert_eq!(generated.items.len(), 2);
    let certificate_validation = application
        .certificate_validate()
        .await
        .expect("validate generated certificate set");
    assert!(certificate_validation.valid);
    assert!(certificate_validation.field_errors.is_empty());

    host.shutdown().await.expect("shutdown UI-neutral host");
}

// STATE-001~016, TEST-STATE, TEST-HOST:
// only the network supervisor is replaced; Application logic is real.
