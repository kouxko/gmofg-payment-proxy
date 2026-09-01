use super::*;

pub(in crate::requirements_tests) fn application_with_fake_ports(
    ports: Arc<FakePorts>,
) -> Application {
    application_with_fake_ports_and_listener_runtime(
        ports,
        Arc::new(InMemoryListenerRuntime::default()),
    )
}

pub(in crate::requirements_tests) fn application_with_fake_ports_and_events(
    ports: Arc<FakePorts>,
    events: Arc<EventHub>,
) -> Application {
    let configuration_store = Arc::new(NoopApplicationConfigurationStore);
    let portability = Arc::new(FakeProtocolPackagePortability::new(configuration_store));
    Application::new(
        "Test Product".into(),
        ApplicationDependencies {
            capture: ports.clone(),
            sessions: ports.clone(),
            faults: ports.clone(),
            certificates: ports.clone(),
            settings: ports.clone(),
            listener_certificates: ports,
            workspaces: Arc::new(InMemoryWorkspaceStore::default()),
            listener_runtime: Arc::new(InMemoryListenerRuntime::default()),
            protocol_packages: protocol_package_services(portability),
            events,
            environment_baseline_capture: test_environment_baseline_capture(),
            environment_identity_allocator: test_environment_identity_allocator(),
            environment_apply_lease: test_environment_apply_lease(),
            environment_material_preparer: test_environment_material_preparer(),
            environment_commit: test_environment_commit(),
            environment_validator: test_environment_validator(),
        },
        Arc::new(UnusedAndroidControlPort),
        Arc::new(UnusedProtectedSecretPort),
    )
}

pub(in crate::requirements_tests) fn application_with_fake_ports_and_android(
    ports: Arc<FakePorts>,
    android: Arc<dyn AndroidControlPort>,
) -> Application {
    let configuration_store = Arc::new(NoopApplicationConfigurationStore);
    let portability = Arc::new(FakeProtocolPackagePortability::new(configuration_store));
    Application::new(
        "Test Product".into(),
        ApplicationDependencies {
            capture: ports.clone(),
            sessions: ports.clone(),
            faults: ports.clone(),
            certificates: ports.clone(),
            settings: ports.clone(),
            listener_certificates: ports,
            workspaces: Arc::new(InMemoryWorkspaceStore::default()),
            listener_runtime: Arc::new(InMemoryListenerRuntime::default()),
            protocol_packages: protocol_package_services(portability),
            events: Arc::new(EventHub::default()),
            environment_baseline_capture: test_environment_baseline_capture(),
            environment_identity_allocator: test_environment_identity_allocator(),
            environment_apply_lease: test_environment_apply_lease(),
            environment_material_preparer: test_environment_material_preparer(),
            environment_commit: test_environment_commit(),
            environment_validator: test_environment_validator(),
        },
        android,
        Arc::new(UnusedProtectedSecretPort),
    )
}

pub(in crate::requirements_tests) fn application_with_fake_ports_and_listener_runtime(
    ports: Arc<FakePorts>,
    listener_runtime: Arc<dyn ListenerRuntimePort>,
) -> Application {
    let workspaces = Arc::new(InMemoryWorkspaceStore::default());
    let configuration_store = Arc::new(NoopApplicationConfigurationStore);
    application_with_workspace_configuration_packages_and_runtime(
        ports,
        workspaces,
        configuration_store,
        listener_runtime,
    )
    .0
}

pub(in crate::requirements_tests) fn application_with_workspace_ports(
    ports: Arc<FakePorts>,
    workspaces: Arc<InMemoryWorkspaceStore>,
) -> Application {
    application_with_workspace_configuration_packages_and_runtime(
        ports,
        workspaces,
        Arc::new(NoopApplicationConfigurationStore),
        Arc::new(InMemoryListenerRuntime::default()),
    )
    .0
}

pub(in crate::requirements_tests) fn application_with_workspace_ports_and_listener_runtime(
    ports: Arc<FakePorts>,
    workspaces: Arc<InMemoryWorkspaceStore>,
    listener_runtime: Arc<dyn ListenerRuntimePort>,
) -> Application {
    application_with_workspace_configuration_packages_and_runtime(
        ports,
        workspaces,
        Arc::new(NoopApplicationConfigurationStore),
        listener_runtime,
    )
    .0
}

pub(in crate::requirements_tests) fn application_with_environment_preview_ports(
    ports: Arc<FakePorts>,
    workspaces: Arc<dyn WorkspaceRepositoryPort>,
    baseline_capture: Arc<dyn EnvironmentApplyBaselineCapturePort>,
    identity_allocator: EnvironmentIdentityAllocator,
) -> Application {
    application_with_workspace_configuration_packages_runtime_and_environment(
        ports,
        workspaces,
        Arc::new(NoopApplicationConfigurationStore),
        Arc::new(InMemoryListenerRuntime::default()),
        baseline_capture,
        identity_allocator,
        test_environment_apply_lease(),
        test_environment_material_preparer(),
        test_environment_commit(),
        test_environment_validator(),
    )
    .0
}

pub(in crate::requirements_tests) fn application_with_environment_preview_ports_and_runtime(
    ports: Arc<FakePorts>,
    workspaces: Arc<dyn WorkspaceRepositoryPort>,
    listener_runtime: Arc<dyn ListenerRuntimePort>,
    baseline_capture: Arc<dyn EnvironmentApplyBaselineCapturePort>,
    identity_allocator: EnvironmentIdentityAllocator,
) -> Application {
    application_with_workspace_configuration_packages_runtime_and_environment(
        ports,
        workspaces,
        Arc::new(NoopApplicationConfigurationStore),
        listener_runtime,
        baseline_capture,
        identity_allocator,
        test_environment_apply_lease(),
        test_environment_material_preparer(),
        test_environment_commit(),
        test_environment_validator(),
    )
    .0
}

#[allow(clippy::too_many_arguments)]
pub(in crate::requirements_tests) fn application_with_environment_preview_apply_ports_and_runtime(
    ports: Arc<FakePorts>,
    workspaces: Arc<dyn WorkspaceRepositoryPort>,
    listener_runtime: Arc<dyn ListenerRuntimePort>,
    baseline_capture: Arc<dyn EnvironmentApplyBaselineCapturePort>,
    identity_allocator: EnvironmentIdentityAllocator,
    apply_lease: Arc<dyn EnvironmentApplyLeasePort>,
    material_preparer: Arc<dyn EnvironmentProtectedMaterialPreparePort>,
    commit: Arc<dyn EnvironmentCommitPort>,
    validator: Arc<dyn EnvironmentValidationLayerPort>,
) -> Application {
    application_with_workspace_configuration_packages_runtime_and_environment(
        ports,
        workspaces,
        Arc::new(NoopApplicationConfigurationStore),
        listener_runtime,
        baseline_capture,
        identity_allocator,
        apply_lease,
        material_preparer,
        commit,
        validator,
    )
    .0
}

pub(in crate::requirements_tests) fn application_with_workspace_repository(
    ports: Arc<FakePorts>,
    workspaces: Arc<dyn WorkspaceRepositoryPort>,
) -> Application {
    application_with_workspace_configuration_packages_and_runtime(
        ports,
        workspaces,
        Arc::new(NoopApplicationConfigurationStore),
        Arc::new(InMemoryListenerRuntime::default()),
    )
    .0
}

pub(in crate::requirements_tests) fn application_with_configuration_store(
    ports: Arc<FakePorts>,
    workspaces: Arc<InMemoryWorkspaceStore>,
    configuration_store: Arc<dyn ApplicationConfigurationStorePort>,
) -> Application {
    application_with_workspace_configuration_and_packages(ports, workspaces, configuration_store).0
}

pub(in crate::requirements_tests) fn application_with_workspace_configuration_and_packages(
    ports: Arc<FakePorts>,
    workspaces: Arc<dyn WorkspaceRepositoryPort>,
    configuration_store: Arc<dyn ApplicationConfigurationStorePort>,
) -> (Application, Arc<FakeProtocolPackagePortability>) {
    application_with_workspace_configuration_packages_and_runtime(
        ports,
        workspaces,
        configuration_store,
        Arc::new(InMemoryListenerRuntime::default()),
    )
}

fn application_with_workspace_configuration_packages_and_runtime(
    ports: Arc<FakePorts>,
    workspaces: Arc<dyn WorkspaceRepositoryPort>,
    configuration_store: Arc<dyn ApplicationConfigurationStorePort>,
    listener_runtime: Arc<dyn ListenerRuntimePort>,
) -> (Application, Arc<FakeProtocolPackagePortability>) {
    application_with_workspace_configuration_packages_runtime_and_environment(
        ports,
        workspaces,
        configuration_store,
        listener_runtime,
        test_environment_baseline_capture(),
        test_environment_identity_allocator(),
        test_environment_apply_lease(),
        test_environment_material_preparer(),
        test_environment_commit(),
        test_environment_validator(),
    )
}

#[allow(clippy::too_many_arguments)]
fn application_with_workspace_configuration_packages_runtime_and_environment(
    ports: Arc<FakePorts>,
    workspaces: Arc<dyn WorkspaceRepositoryPort>,
    configuration_store: Arc<dyn ApplicationConfigurationStorePort>,
    listener_runtime: Arc<dyn ListenerRuntimePort>,
    environment_baseline_capture: Arc<dyn EnvironmentApplyBaselineCapturePort>,
    environment_identity_allocator: EnvironmentIdentityAllocator,
    environment_apply_lease: Arc<dyn EnvironmentApplyLeasePort>,
    environment_material_preparer: Arc<dyn EnvironmentProtectedMaterialPreparePort>,
    environment_commit: Arc<dyn EnvironmentCommitPort>,
    environment_validator: Arc<dyn EnvironmentValidationLayerPort>,
) -> (Application, Arc<FakeProtocolPackagePortability>) {
    let portability = Arc::new(FakeProtocolPackagePortability::new(configuration_store));
    let protocol_packages = protocol_package_services(Arc::clone(&portability));
    let application = Application::new(
        "Test Product".into(),
        ApplicationDependencies {
            capture: ports.clone(),
            sessions: ports.clone(),
            faults: ports.clone(),
            certificates: ports.clone(),
            settings: ports.clone(),
            listener_certificates: ports,
            workspaces,
            listener_runtime,
            protocol_packages,
            events: Arc::new(EventHub::default()),
            environment_baseline_capture,
            environment_identity_allocator,
            environment_apply_lease,
            environment_material_preparer,
            environment_commit,
            environment_validator,
        },
        Arc::new(UnusedAndroidControlPort),
        Arc::new(UnusedProtectedSecretPort),
    );
    (application, portability)
}

pub(in crate::requirements_tests) fn unused_protocol_package_services()
-> ProtocolPackageApplicationServices {
    let configuration_store = Arc::new(NoopApplicationConfigurationStore);
    protocol_package_services(Arc::new(FakeProtocolPackagePortability::new(
        configuration_store,
    )))
}

pub(in crate::requirements_tests) fn protocol_package_services(
    portability: Arc<FakeProtocolPackagePortability>,
) -> ProtocolPackageApplicationServices {
    ProtocolPackageApplicationServices {
        importer: portability.clone(),
        builtin: portability.clone(),
        usage_query: portability.clone(),
        external: portability,
    }
}
