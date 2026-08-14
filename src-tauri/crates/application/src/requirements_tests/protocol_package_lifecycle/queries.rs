use super::*;

#[tokio::test]
async fn list_groups_versions_by_id_and_detail_uses_the_exact_version() {
    let (application, services, _, _) = fixture();
    let iso_v1 = package("iso-8583", "1.0.0");
    let iso_v2 = package("iso-8583", "2.0.0");
    let tlv = package("tlv", "1.0.0");
    for item in [iso_v2.clone(), tlv.clone(), iso_v1.clone()] {
        services.insert(record(item, false));
    }
    let expected_usage = usage(
        WorkspaceId::new(),
        ListenerId::new(),
        ListenerRuntimeState::Stopped,
    );
    services.set_usages(iso_v1.clone(), vec![expected_usage.clone()]);
    services.set_usages(iso_v2.clone(), Vec::new());

    let groups = application.protocol_package_list().await.unwrap();
    assert_eq!(groups.len(), 2);
    assert_eq!(groups[0].id.as_str(), "iso-8583");
    assert_eq!(
        groups[0]
            .versions
            .iter()
            .map(|item| item.package.version.as_str())
            .collect::<Vec<_>>(),
        ["1.0.0", "2.0.0"]
    );
    assert_eq!(groups[1].id.as_str(), "tlv");

    let detail = application
        .protocol_package_detail(iso_v1.clone())
        .await
        .unwrap();
    assert_eq!(detail.version.package, iso_v1);
    assert_eq!(detail.usages, vec![expected_usage]);
    assert_eq!(services.usage_calls.load(Ordering::SeqCst), 1);

    let missing = application
        .protocol_package_detail(package("iso-8583", "3.0.0"))
        .await
        .unwrap_err();
    assert_eq!(error_code(&missing), "PROTOCOL_PACKAGE_NOT_FOUND");
    assert_eq!(services.usage_calls.load(Ordering::SeqCst), 1);
}

#[tokio::test]
async fn query_failures_return_no_partial_list_or_detail() {
    let (application, services, _, _) = fixture();
    let target = package("iso-8583", "1.0.0");
    services.insert(record(target.clone(), false));
    services.failures.lock().list = Some(AppError::new("STORE_LIST_FAILED", "list"));
    assert_eq!(
        error_code(&application.protocol_package_list().await.unwrap_err()),
        "STORE_LIST_FAILED"
    );

    services.failures.lock().list = None;
    services.failures.lock().usage = Some(AppError::new("USAGE_QUERY_FAILED", "usage"));
    assert_eq!(
        error_code(
            &application
                .protocol_package_detail(target)
                .await
                .unwrap_err()
        ),
        "USAGE_QUERY_FAILED"
    );
}
