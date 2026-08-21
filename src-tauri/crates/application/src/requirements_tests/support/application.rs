use super::*;

pub(in crate::requirements_tests) fn application_with_fake_ports(
    ports: Arc<FakePorts>,
) -> Application {
    application_with_fake_ports_and_listener_runtime(
        ports,
        Arc::new(InMemoryListenerRuntime::default()),
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
            breakpoints: Arc::new(BreakpointCoordinator::default()),
            breakpoint_validation: ports.clone(),
            rules: ports.clone(),
            faults: ports.clone(),
            certificates: ports.clone(),
            settings: ports.clone(),
            listener_certificates: ports,
            workspaces: Arc::new(InMemoryWorkspaceStore::default()),
            listener_runtime: Arc::new(InMemoryListenerRuntime::default()),
            protocol_packages: protocol_package_services(portability),
            events: Arc::new(EventHub::default()),
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

pub(in crate::requirements_tests) fn application_with_configuration_store(
    ports: Arc<FakePorts>,
    workspaces: Arc<InMemoryWorkspaceStore>,
    configuration_store: Arc<dyn ApplicationConfigurationStorePort>,
) -> Application {
    application_with_workspace_configuration_and_packages(ports, workspaces, configuration_store).0
}

pub(in crate::requirements_tests) fn application_with_workspace_configuration_and_packages(
    ports: Arc<FakePorts>,
    workspaces: Arc<InMemoryWorkspaceStore>,
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
    workspaces: Arc<InMemoryWorkspaceStore>,
    configuration_store: Arc<dyn ApplicationConfigurationStorePort>,
    listener_runtime: Arc<dyn ListenerRuntimePort>,
) -> (Application, Arc<FakeProtocolPackagePortability>) {
    let portability = Arc::new(FakeProtocolPackagePortability::new(configuration_store));
    let protocol_packages = protocol_package_services(Arc::clone(&portability));
    let application = Application::new(
        "Test Product".into(),
        ApplicationDependencies {
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
            listener_runtime,
            protocol_packages,
            events: Arc::new(EventHub::default()),
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
        store: portability.clone(),
        compiler: portability.clone(),
        importer: portability.clone(),
        builtin: portability.clone(),
        usage_query: portability.clone(),
        portability,
        external: Arc::new(UnusedExternalPackagePort),
    }
}
