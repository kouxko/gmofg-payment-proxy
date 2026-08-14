use super::*;

pub(in crate::requirements_tests) fn application_with_fake_ports(
    ports: Arc<FakePorts>,
) -> Application {
    application_with_fake_ports_and_listener_runtime(
        ports,
        Arc::new(InMemoryListenerRuntime::default()),
    )
}

pub(in crate::requirements_tests) fn application_with_fake_ports_and_listener_runtime(
    ports: Arc<FakePorts>,
    listener_runtime: Arc<dyn ListenerRuntimePort>,
) -> Application {
    let workspaces = Arc::new(InMemoryWorkspaceStore::default());
    let workspace_documents = Arc::new(InMemoryWorkspaceDocumentStore::default());
    let configuration_store = Arc::new(UnavailableApplicationConfigurationStore);
    application_with_workspace_configuration_packages_and_runtime(
        ports,
        workspaces,
        workspace_documents,
        configuration_store,
        listener_runtime,
    )
    .0
}

pub(in crate::requirements_tests) fn application_with_workspace_ports(
    ports: Arc<FakePorts>,
    workspaces: Arc<InMemoryWorkspaceStore>,
    workspace_documents: Arc<InMemoryWorkspaceDocumentStore>,
) -> Application {
    application_with_workspace_configuration_packages_and_runtime(
        ports,
        workspaces,
        workspace_documents,
        Arc::new(UnavailableApplicationConfigurationStore),
        Arc::new(InMemoryListenerRuntime::default()),
    )
    .0
}

pub(in crate::requirements_tests) fn application_with_configuration_store(
    ports: Arc<FakePorts>,
    workspaces: Arc<InMemoryWorkspaceStore>,
    workspace_documents: Arc<InMemoryWorkspaceDocumentStore>,
    configuration_store: Arc<dyn ApplicationConfigurationStorePort>,
) -> Application {
    application_with_workspace_configuration_and_packages(
        ports,
        workspaces,
        workspace_documents,
        configuration_store,
    )
    .0
}

pub(in crate::requirements_tests) fn application_with_workspace_configuration_and_packages(
    ports: Arc<FakePorts>,
    workspaces: Arc<InMemoryWorkspaceStore>,
    workspace_documents: Arc<InMemoryWorkspaceDocumentStore>,
    configuration_store: Arc<dyn ApplicationConfigurationStorePort>,
) -> (Application, Arc<FakeProtocolPackagePortability>) {
    application_with_workspace_configuration_packages_and_runtime(
        ports,
        workspaces,
        workspace_documents,
        configuration_store,
        Arc::new(InMemoryListenerRuntime::default()),
    )
}

fn application_with_workspace_configuration_packages_and_runtime(
    ports: Arc<FakePorts>,
    workspaces: Arc<InMemoryWorkspaceStore>,
    workspace_documents: Arc<InMemoryWorkspaceDocumentStore>,
    configuration_store: Arc<dyn ApplicationConfigurationStorePort>,
    listener_runtime: Arc<dyn ListenerRuntimePort>,
) -> (Application, Arc<FakeProtocolPackagePortability>) {
    let workspace_port: Arc<dyn WorkspaceRepositoryPort> = workspaces.clone();
    let portability = Arc::new(FakeProtocolPackagePortability::new(
        workspace_port,
        configuration_store,
    ));
    let mut protocol_packages = ProtocolPackageApplicationServices::unavailable();
    protocol_packages.compiler = portability.clone();
    protocol_packages.portability = portability.clone();
    let application = Application::new_with_platform_services(
        "Test Product".into(),
        ApplicationDependencies {
            proxy: ports.clone(),
            capture: ports.clone(),
            sessions: ports.clone(),
            breakpoints: Arc::new(BreakpointCoordinator::default()),
            breakpoint_validation: ports.clone(),
            rules: ports.clone(),
            faults: ports.clone(),
            certificates: ports.clone(),
            settings: ports.clone(),
            listener_certificates: ports,
            workspaces,
            workspace_documents,
            listener_runtime,
            protocol_packages,
            events: Arc::new(EventHub::default()),
        },
        Arc::new(UnavailableAndroidControlPort),
        Arc::new(UnavailableProtectedSecretPort),
    );
    (application, portability)
}
