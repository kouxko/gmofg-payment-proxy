use super::protocol_package_portability::{
    description, package, portable_package, scripted_workspace,
};
use super::*;

#[tokio::test]
async fn v3_import_purely_revalidates_exact_reference_and_preserves_full_local_registry() {
    let referenced = package("legacy-v3", "1.0.0");
    let extra = package("local-only", "2.0.0");
    let mut workspace = scripted_workspace(referenced.clone(), false);
    // v3 没有 Socket rule wire，但已经有 Scripted Listener 与显式拓扑。
    workspace.socket_rules.clear();
    workspace.socket_rule_created_order_high_water = 0;
    let bytes = v3_configuration_bytes(workspace);
    let documents = Arc::new(InMemoryWorkspaceDocumentStore::default());
    documents.set_next_import(bytes);
    let store = Arc::new(RecordingConfigurationStore::default());
    let (application, portability) = application_with_workspace_configuration_and_packages(
        Arc::new(FakePorts::default()),
        Arc::new(InMemoryWorkspaceStore::default()),
        documents,
        store.clone(),
    );
    portability.register(
        portable_package(referenced.clone(), false),
        description(referenced),
    );
    // 未引用包即使没有可用编译描述也不能阻断 legacy 导入；registry 与 enabled 原样保留。
    portability
        .application_packages
        .lock()
        .push(portable_package(extra, true));

    application
        .application_configuration_import()
        .await
        .unwrap();

    assert_eq!(
        portability.compiler_validate_calls.load(Ordering::SeqCst),
        0
    );
    assert_eq!(
        portability.compiler_describe_calls.load(Ordering::SeqCst),
        0
    );
    assert_eq!(
        portability.installed_preflight_calls.load(Ordering::SeqCst),
        1
    );
    assert_eq!(portability.preflight_calls.load(Ordering::SeqCst), 0);
    assert_eq!(portability.replace_calls.load(Ordering::SeqCst), 0);
    assert_eq!(portability.legacy_replace_calls.load(Ordering::SeqCst), 1);
    let stored = store.document.lock().clone().unwrap();
    assert_eq!(stored.protocol_packages.len(), 2);
    assert!(stored.protocol_packages.iter().any(|item| !item.enabled));
    assert!(stored.protocol_packages.iter().any(|item| item.enabled));
}

#[tokio::test]
async fn v2_import_has_no_scripted_reference_and_still_preserves_registry_enabled_state() {
    let disabled = package("disabled", "1.0.0");
    let enabled = package("enabled", "2.0.0");
    let documents = Arc::new(InMemoryWorkspaceDocumentStore::default());
    documents.set_next_import(v2_configuration_bytes());
    let store = Arc::new(RecordingConfigurationStore::default());
    let (application, portability) = application_with_workspace_configuration_and_packages(
        Arc::new(FakePorts::default()),
        Arc::new(InMemoryWorkspaceStore::default()),
        documents,
        store.clone(),
    );
    portability.register(
        portable_package(disabled.clone(), false),
        description(disabled),
    );
    portability.register(
        portable_package(enabled.clone(), true),
        description(enabled),
    );

    application
        .application_configuration_import()
        .await
        .unwrap();

    // v2 的 ProxyWorkspaceV2 只能表达旧 HTTP Listener，没有可供编译的 Scripted 引用。
    assert_eq!(
        portability.compiler_validate_calls.load(Ordering::SeqCst),
        0
    );
    assert_eq!(
        portability.compiler_describe_calls.load(Ordering::SeqCst),
        0
    );
    assert_eq!(
        portability.installed_preflight_calls.load(Ordering::SeqCst),
        1
    );
    assert_eq!(portability.replace_calls.load(Ordering::SeqCst), 0);
    assert_eq!(portability.legacy_replace_calls.load(Ordering::SeqCst), 1);
    let stored = store.document.lock().clone().unwrap();
    assert_eq!(stored.protocol_packages.len(), 2);
    assert_eq!(
        stored
            .protocol_packages
            .iter()
            .map(|item| item.enabled)
            .collect::<Vec<_>>(),
        vec![false, true]
    );
}

#[tokio::test]
async fn legacy_failure_after_pure_package_preflight_performs_no_commit_or_compiler_write() {
    let referenced = package("legacy-invalid-settings", "1.0.0");
    let mut workspace = scripted_workspace(referenced.clone(), false);
    workspace.socket_rules.clear();
    workspace.socket_rule_created_order_high_water = 0;
    let documents = Arc::new(InMemoryWorkspaceDocumentStore::default());
    documents.set_next_import(v3_configuration_bytes(workspace));
    let store = Arc::new(RecordingConfigurationStore::default());
    let (application, portability) = application_with_workspace_configuration_and_packages(
        Arc::new(FakePorts::default()),
        Arc::new(InMemoryWorkspaceStore::default()),
        documents,
        store.clone(),
    );
    let mut incompatible = description(referenced.clone());
    incompatible.capabilities.upstream.encode = false;
    portability.register(portable_package(referenced, false), incompatible);

    assert_eq!(
        application
            .application_configuration_import()
            .await
            .unwrap_err()
            .view_model
            .code,
        "PROTOCOL_PACKAGE_CAPABILITY_MISMATCH"
    );
    assert_eq!(
        portability.installed_preflight_calls.load(Ordering::SeqCst),
        1
    );
    assert_eq!(
        portability.compiler_validate_calls.load(Ordering::SeqCst),
        0
    );
    assert_eq!(
        portability.compiler_describe_calls.load(Ordering::SeqCst),
        0
    );
    assert_eq!(portability.legacy_replace_calls.load(Ordering::SeqCst), 0);
    assert!(store.document.lock().is_none());
}

fn v3_configuration_bytes(workspace: ProxyWorkspace) -> Vec<u8> {
    let mut value = serde_json::to_value(ApplicationConfigurationDocument {
        format_version: APPLICATION_CONFIGURATION_FORMAT_VERSION,
        selected_workspace_id: workspace.id,
        workspaces: vec![workspace],
        settings: PortableSettings::from(&SettingsDraft::default()),
        certificate_materials: Vec::new(),
        protocol_packages: Vec::new(),
    })
    .unwrap();
    value["format_version"] = serde_json::json!(APPLICATION_CONFIGURATION_V3_FORMAT_VERSION);
    value.as_object_mut().unwrap().remove("protocol_packages");
    value["workspaces"][0]
        .as_object_mut()
        .unwrap()
        .insert("metadata_extractors".into(), serde_json::json!([]));
    value["workspaces"][0]
        .as_object_mut()
        .unwrap()
        .remove("socket_rules");
    value["workspaces"][0]
        .as_object_mut()
        .unwrap()
        .remove("socket_rule_created_order_high_water");
    serde_json::to_vec(&value).unwrap()
}

fn v2_configuration_bytes() -> Vec<u8> {
    let current = ProxyWorkspace::default();
    let listener = &current.listeners[0];
    let http = listener.http().unwrap();
    let listener = ProxyListenerV2 {
        id: listener.id,
        name: listener.name.clone(),
        enabled: listener.enabled,
        bind_address: listener.bind_address.clone(),
        port: listener.port,
        authentication: http.authentication.clone(),
        allowed_client_cidrs: listener.allowed_client_cidrs.clone(),
        mitm: http.mitm.clone(),
        connect_timeout_ms: listener.connect_timeout_ms,
        read_timeout_ms: listener.read_timeout_ms,
        write_timeout_ms: listener.write_timeout_ms,
        downstream_tls: Some(http.downstream_tls.clone()),
        request_body_codec: http.request_body_codec,
        response_body_codec: http.response_body_codec,
        fixed_server: http.fixed_server.clone(),
    };
    serde_json::to_vec(&serde_json::json!({
        "format_version": APPLICATION_CONFIGURATION_V2_FORMAT_VERSION,
        "selected_workspace_id": current.id,
        "workspaces": [{
            "id": current.id,
            "name": current.name,
            "revision": current.revision,
            "listeners": [listener],
            "metadata_extractors": [],
            "response_assertions": current.response_assertions,
            "rules": current.rules,
            "fault_presets": current.fault_presets,
            "certificate_references": current.certificate_references,
            "android_network_profiles": current.android_network_profiles,
        }],
        "settings": PortableSettings::from(&SettingsDraft::default()),
        "certificate_materials": [],
    }))
    .unwrap()
}
