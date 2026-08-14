use super::*;

#[tokio::test]
async fn list_groups_versions_by_id_and_detail_uses_the_exact_version() {
    let (application, services, _, _) = fixture();
    let iso_v1 = package("iso-8583", "1.0.0");
    let iso_v2 = package("iso-8583", "2.0.0");
    let iso_v10 = package("iso-8583", "10.0.0");
    let tlv = package("tlv", "1.0.0");
    for item in [iso_v10, iso_v2.clone(), tlv.clone(), iso_v1.clone()] {
        services.insert(record(item, false));
    }
    let expected_usage = usage(
        WorkspaceId::new(),
        ListenerId::new(),
        ListenerRuntimeState::Stopped,
    );
    services.set_usages(iso_v1.clone(), vec![expected_usage.clone()]);
    services.set_usages(iso_v2.clone(), Vec::new());
    let expected_description = description();
    services.set_description(iso_v1.clone(), expected_description.clone());

    let groups = application.protocol_package_list().await.unwrap();
    assert_eq!(groups.len(), 2);
    assert_eq!(groups[0].id.as_str(), "iso-8583");
    assert_eq!(
        groups[0]
            .versions
            .iter()
            .map(|item| item.package.version.as_str())
            .collect::<Vec<_>>(),
        ["1.0.0", "2.0.0", "10.0.0"]
    );
    assert_eq!(groups[1].id.as_str(), "tlv");
    assert_eq!(groups[0].reference_count, 1);
    assert_eq!(groups[0].active_reference_count, 0);
    assert_eq!(groups[1].reference_count, 0);
    assert_eq!(services.usage_count_calls.load(Ordering::SeqCst), 1);

    let detail = application
        .protocol_package_detail(iso_v1.clone())
        .await
        .unwrap();
    assert_eq!(detail.version.package, iso_v1);
    assert_eq!(detail.capabilities, expected_description.capabilities);
    assert_eq!(detail.schema, expected_description.schema);
    assert_eq!(
        detail
            .schema
            .fields
            .iter()
            .map(|field| field.name.as_str())
            .collect::<Vec<_>>(),
        ["trace_id", "amount", "approved"]
    );
    assert_eq!(detail.usages, vec![expected_usage]);
    assert_eq!(services.describe_calls.load(Ordering::SeqCst), 1);
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
    services.failures.lock().describe = Some(AppError::new("DESCRIBE_FAILED", "describe"));
    assert_eq!(
        error_code(
            &application
                .protocol_package_detail(target.clone())
                .await
                .unwrap_err()
        ),
        "DESCRIBE_FAILED"
    );
    assert_eq!(services.usage_calls.load(Ordering::SeqCst), 0);

    services.failures.lock().describe = None;
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

#[tokio::test]
async fn usage_query_requires_an_installed_exact_version_and_preserves_runtime_states() {
    let (application, services, _, _) = fixture();
    let target = package("iso-8583", "1.0.0");
    services.insert(record(target.clone(), false));
    let expected = [
        ListenerRuntimeState::Stopped,
        ListenerRuntimeState::Starting,
        ListenerRuntimeState::Running,
        ListenerRuntimeState::Stopping,
        ListenerRuntimeState::Faulted,
    ]
    .into_iter()
    .map(|state| usage(WorkspaceId::new(), ListenerId::new(), state))
    .collect::<Vec<_>>();
    services.set_usages(target.clone(), expected.clone());

    assert_eq!(
        application
            .protocol_package_usage(target.clone())
            .await
            .unwrap(),
        expected
    );
    let missing = application
        .protocol_package_usage(package("iso-8583", "9.0.0"))
        .await
        .unwrap_err();
    assert_eq!(error_code(&missing), "PROTOCOL_PACKAGE_NOT_FOUND");
    assert_eq!(services.usage_calls.load(Ordering::SeqCst), 1);
}

#[tokio::test]
async fn native_import_cancellation_success_and_errors_are_forwarded_without_partial_dto() {
    let (application, services, _, _) = fixture();
    let target = package("iso-8583", "1.0.0");
    let version = record(target, false);
    let description = description();
    let token = ProtocolPackageImportToken::from_uuid(Uuid::new_v4());
    let preview = ProtocolPackageImportPreviewViewModel {
        token: Some(token),
        disposition: crate::ProtocolPackageImportDispositionViewModel::New,
        package: version.package.clone(),
        name: version.name.clone(),
        host_api: version.host_api,
        capabilities: description.capabilities,
        schema: description.schema.clone(),
    };
    let imported = ProtocolPackageImportViewModel {
        outcome: ProtocolPackageImportOutcomeViewModel::Installed,
        version,
        capabilities: description.capabilities,
        schema: description.schema,
    };
    services.push_import_response(Ok(None));
    services.push_import_response(Ok(Some(preview.clone())));
    services.push_import_response(Err(AppError::new("SCRIPT_SYNTAX_INVALID", "invalid")));
    services.push_import_commit_response(Ok(imported.clone()));

    assert_eq!(application.protocol_package_import().await.unwrap(), None);
    assert_eq!(
        application.protocol_package_import().await.unwrap(),
        Some(preview)
    );
    assert_eq!(
        application
            .protocol_package_import_commit(token)
            .await
            .unwrap(),
        imported
    );
    assert_eq!(
        error_code(&application.protocol_package_import().await.unwrap_err()),
        "SCRIPT_SYNTAX_INVALID"
    );
    assert_eq!(services.import_calls.load(Ordering::SeqCst), 3);
}

#[tokio::test]
async fn detail_serializes_only_the_approved_no_source_wire_shape() {
    let (application, services, _, _) = fixture();
    let target = package("iso-8583", "1.0.0");
    services.insert(record(target.clone(), false));
    let detail = application.protocol_package_detail(target).await.unwrap();
    let value = serde_json::to_value(detail).unwrap();

    assert_eq!(
        value
            .as_object()
            .unwrap()
            .keys()
            .cloned()
            .collect::<BTreeSet<_>>(),
        ["capabilities", "schema", "usages", "version"]
            .into_iter()
            .map(str::to_owned)
            .collect()
    );
    assert_eq!(
        value["schema"]["fields"]
            .as_array()
            .unwrap()
            .iter()
            .map(|field| field["name"].as_str().unwrap())
            .collect::<Vec<_>>(),
        ["trace_id", "amount", "approved"]
    );
    assert_no_forbidden_protocol_package_keys(&value);
}

fn assert_no_forbidden_protocol_package_keys(value: &serde_json::Value) {
    const FORBIDDEN: &[&str] = &[
        "source",
        "script",
        "script_content",
        "ast",
        "absolute_path",
        "path",
        "zip",
        "zip_bytes",
        "files",
        "contents",
    ];
    match value {
        serde_json::Value::Object(object) => {
            for (key, nested) in object {
                assert!(!FORBIDDEN.contains(&key.as_str()), "forbidden key: {key}");
                assert_no_forbidden_protocol_package_keys(nested);
            }
        }
        serde_json::Value::Array(items) => {
            for item in items {
                assert_no_forbidden_protocol_package_keys(item);
            }
        }
        _ => {}
    }
}
