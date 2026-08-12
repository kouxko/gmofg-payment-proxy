use super::*;

pub(in crate::requirements_tests) fn application_with_fake_ports(
    ports: Arc<FakePorts>,
) -> Application {
    application_with_workspace_ports(
        ports,
        Arc::new(InMemoryWorkspaceStore::default()),
        Arc::new(InMemoryWorkspaceDocumentStore::default()),
    )
}

pub(in crate::requirements_tests) fn application_with_workspace_ports(
    ports: Arc<FakePorts>,
    workspaces: Arc<InMemoryWorkspaceStore>,
    workspace_documents: Arc<InMemoryWorkspaceDocumentStore>,
) -> Application {
    Application::new(
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
            listener_runtime: Arc::new(InMemoryListenerRuntime::default()),
            events: Arc::new(EventHub::default()),
        },
    )
}

pub(in crate::requirements_tests) fn application_with_configuration_store(
    ports: Arc<FakePorts>,
    workspaces: Arc<InMemoryWorkspaceStore>,
    workspace_documents: Arc<InMemoryWorkspaceDocumentStore>,
    configuration_store: Arc<dyn ApplicationConfigurationStorePort>,
) -> Application {
    Application::new_with_platform_services(
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
            listener_runtime: Arc::new(InMemoryListenerRuntime::default()),
            events: Arc::new(EventHub::default()),
        },
        Arc::new(UnavailableAndroidControlPort),
        Arc::new(UnavailableProtectedSecretPort),
        configuration_store,
    )
}
