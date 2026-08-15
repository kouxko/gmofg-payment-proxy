use super::*;

#[tokio::test]
async fn listener_catalog_only_returns_enabled_valid_current_descriptions_in_stable_order() {
    let (application, services, _, _) = fixture();
    let first = package("alpha", "1.0.0");
    let second = package("alpha", "2.0.0");
    let disabled = package("beta", "1.0.0");
    let invalid = package("gamma", "1.0.0");
    let wrong_description = package("omega", "1.0.0");

    for item in [second.clone(), wrong_description.clone(), first.clone()] {
        services.insert(record(item, true));
    }
    services.insert(record(disabled, false));
    let mut invalid_record = record(invalid, true);
    invalid_record.validation = ProtocolPackageValidationViewModel::Invalid {
        code: "SCRIPT_SYNTAX_INVALID".into(),
    };
    services.insert(invalid_record);
    services.set_description(first.clone(), description(first.clone()));
    services.set_description(second.clone(), description(second.clone()));
    services.set_description(wrong_description, description(package("other", "1.0.0")));

    let catalog = application
        .listener_protocol_package_catalog()
        .await
        .unwrap();

    assert_eq!(catalog.installed_version_count, 5);
    assert_eq!(catalog.unavailable_version_count, 3);
    assert_eq!(
        catalog
            .options
            .iter()
            .map(|option| option.package.clone())
            .collect::<Vec<_>>(),
        [first.clone(), second.clone()]
    );
    assert_eq!(catalog.options[0].name, format!("alpha {}", first.version));
    assert_eq!(catalog.options[0].schema.id, "payments");
    assert!(catalog.options[0].capabilities.upstream.encode);
    assert_eq!(services.describe_calls.load(Ordering::SeqCst), 0);
    assert_eq!(services.compile_calls.load(Ordering::SeqCst), 0);
    assert_eq!(services.installed_preflight_calls.load(Ordering::SeqCst), 3);
}

#[tokio::test]
async fn listener_catalog_fails_closed_for_store_error_and_hides_compiler_errors() {
    let (application, services, _, _) = fixture();
    services.failures.lock().list = Some(AppError::new("STORE_LIST_FAILED", "list"));
    assert_eq!(
        error_code(
            &application
                .listener_protocol_package_catalog()
                .await
                .unwrap_err()
        ),
        "STORE_LIST_FAILED"
    );

    services.failures.lock().list = None;
    let target = package("iso-8583", "1.0.0");
    services.insert(record(target, true));
    services.failures.lock().installed_preflight = Some(AppError::new(
        "SENSITIVE_COMPILER_FAILURE",
        "must not cross the catalog boundary",
    ));
    let catalog = application
        .listener_protocol_package_catalog()
        .await
        .unwrap();
    assert!(catalog.options.is_empty());
    assert_eq!(catalog.installed_version_count, 1);
    assert_eq!(catalog.unavailable_version_count, 1);
}

#[tokio::test]
async fn listener_catalog_holds_the_mutation_gate_through_fresh_preflight() {
    let (application, services, _, _) = fixture();
    let application = Arc::new(application);
    let target = package("iso-8583", "1.0.0");
    services.insert(record(target.clone(), true));
    services.set_description(target.clone(), description(target.clone()));
    services
        .block_installed_preflight
        .store(true, Ordering::SeqCst);

    let catalog_application = application.clone();
    let catalog = tokio::spawn(async move {
        catalog_application
            .listener_protocol_package_catalog()
            .await
    });
    services.installed_preflight_entered.notified().await;

    let disable_application = application.clone();
    let disable_target = target.clone();
    let mut disable = tokio::spawn(async move {
        disable_application
            .protocol_package_disable(disable_target)
            .await
    });
    assert!(
        tokio::time::timeout(std::time::Duration::from_millis(50), &mut disable)
            .await
            .is_err(),
        "目录快照完成前，启停写操作必须等待同一个 mutation gate"
    );

    services.continue_installed_preflight.notify_one();
    assert_eq!(catalog.await.unwrap().unwrap().options.len(), 1);
    assert!(!disable.await.unwrap().unwrap().enabled);
}

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
    let expected_description = description(iso_v1.clone());
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
async fn detail_rejects_a_compiler_description_for_another_exact_package() {
    let (application, services, _, _) = fixture();
    let requested = package("iso-8583", "1.0.0");
    services.insert(record(requested.clone(), true));
    services.set_description(requested.clone(), description(package("iso-8583", "2.0.0")));

    let error = application
        .protocol_package_detail(requested)
        .await
        .expect_err("串用另一版本的编译描述必须 fail-closed");

    assert_eq!(
        error_code(&error),
        "PROTOCOL_PACKAGE_DESCRIPTION_IDENTITY_MISMATCH"
    );
    assert_eq!(services.usage_calls.load(Ordering::SeqCst), 0);
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
    let description = description(target.clone());
    let version = record(target, false);
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
