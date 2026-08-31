use std::{
    collections::{HashMap, HashSet},
    sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    },
};

use async_trait::async_trait;

use super::super::*;

#[derive(Debug, Default)]
pub(super) struct FakeExternalPackages {
    pub(super) records:
        parking_lot::Mutex<HashMap<ProtocolPackageRef, ProtocolPackageVersionViewModel>>,
    pub(super) descriptions:
        parking_lot::Mutex<HashMap<ProtocolPackageRef, ProtocolPackageDescriptionViewModel>>,
    pub(super) local_packages: parking_lot::Mutex<HashSet<ProtocolPackageRef>>,
    pub(super) disconnect_calls: AtomicUsize,
    pub(super) delete_calls: AtomicUsize,
    pub(super) set_enabled_calls: AtomicUsize,
    pub(super) restart_calls: AtomicUsize,
}

#[async_trait]
impl ExternalPackageApplicationPort for FakeExternalPackages {
    async fn service_status(&self) -> AppResult<ExternalPackageServiceStatusViewModel> {
        Ok(ExternalPackageServiceStatusViewModel {
            websocket_url: "ws://0.0.0.0:8765/packages".into(),
            fixed_path: "/packages".into(),
            online_connection_count: self
                .records
                .lock()
                .values()
                .filter(|record| record.source.external_online() == Some(true))
                .count(),
            state: ExternalPackageServiceStateViewModel::Listening,
            authentication_enabled: false,
        })
    }

    async fn list(&self) -> AppResult<Vec<ProtocolPackageVersionViewModel>> {
        Ok(self.records.lock().values().cloned().collect())
    }

    async fn get(
        &self,
        package: &ProtocolPackageRef,
    ) -> AppResult<Option<ProtocolPackageVersionViewModel>> {
        Ok(self.records.lock().get(package).cloned())
    }

    async fn describe(
        &self,
        package: &ProtocolPackageRef,
    ) -> AppResult<ProtocolPackageDescriptionViewModel> {
        self.descriptions
            .lock()
            .get(package)
            .cloned()
            .ok_or_else(|| AppError::new("EXTERNAL_PACKAGE_NOT_FOUND", "外部软件包不存在。"))
    }

    async fn detail(
        &self,
        package: &ProtocolPackageRef,
    ) -> AppResult<ExternalPackageDetailViewModel> {
        if !self.records.lock().contains_key(package) {
            return Err(AppError::new(
                "EXTERNAL_PACKAGE_NOT_FOUND",
                "外部软件包不存在。",
            ));
        }
        Ok(external_detail(
            self.local_packages.lock().contains(package),
        ))
    }

    async fn set_enabled(&self, package: &ProtocolPackageRef, enabled: bool) -> AppResult<()> {
        self.set_enabled_calls.fetch_add(1, Ordering::SeqCst);
        self.records
            .lock()
            .get_mut(package)
            .ok_or_else(|| AppError::new("EXTERNAL_PACKAGE_NOT_FOUND", "外部软件包不存在。"))?
            .enabled = enabled;
        Ok(())
    }

    async fn disconnect(&self, _: &ProtocolPackageRef) -> AppResult<()> {
        self.disconnect_calls.fetch_add(1, Ordering::SeqCst);
        Ok(())
    }

    async fn restart(&self, package: &ProtocolPackageRef) -> AppResult<()> {
        self.restart_calls.fetch_add(1, Ordering::SeqCst);
        let mut records = self.records.lock();
        let record = records
            .get_mut(package)
            .ok_or_else(|| AppError::new("EXTERNAL_PACKAGE_NOT_FOUND", "外部软件包不存在。"))?;
        record.source = ProtocolPackageSourceViewModel::External { online: true };
        Ok(())
    }

    async fn delete(&self, package: &ProtocolPackageRef) -> AppResult<()> {
        self.delete_calls.fetch_add(1, Ordering::SeqCst);
        self.records.lock().remove(package);
        Ok(())
    }

    async fn application_backup_baseline(
        &self,
    ) -> AppResult<Vec<ApplicationBackupProtocolPackageBaseline>> {
        unused()
    }
    async fn export_application_packages(
        &self,
    ) -> AppResult<Vec<PortableApplicationProtocolPackage>> {
        unused()
    }
    async fn preflight_application_packages(
        &self,
        _: &[PortableApplicationProtocolPackage],
    ) -> AppResult<Vec<ProtocolPackageDescriptionViewModel>> {
        unused()
    }
    async fn preflight_installed_packages(
        &self,
        _: &[ProtocolPackageRef],
    ) -> AppResult<Vec<ProtocolPackageDescriptionViewModel>> {
        unused()
    }
    async fn replace_application_bundle(
        &self,
        _: Vec<PortableApplicationProtocolPackage>,
        _: ApplicationConfigurationDocument,
    ) -> AppResult<()> {
        unused()
    }
    async fn reset_application_bundle(&self, _: ApplicationConfigurationDocument) -> AppResult<()> {
        unused()
    }
}

fn external_detail(local_process: bool) -> ExternalPackageDetailViewModel {
    let methods = ExternalPackageDirectionMethodsViewModel {
        frame: "hooks.upstream.frame".into(),
        decode: "hooks.upstream.decode".into(),
        encode: "hooks.upstream.encode".into(),
        display: "document.upstream.display".into(),
    };
    ExternalPackageDetailViewModel {
        local_process,
        remote_address: Some("127.0.0.1:9000".into()),
        connection_id: None,
        first_connected_at: Utc.timestamp_opt(1_700_000_000, 0).single().unwrap(),
        last_connected_at: Utc.timestamp_opt(1_700_000_000, 0).single().unwrap(),
        registration_fingerprint_sha256: "00".repeat(32),
        upstream_methods: methods.clone(),
        downstream_methods: methods,
        recent_error: None,
    }
}

pub(super) fn package(id: &str, version: &str) -> ProtocolPackageRef {
    ProtocolPackageRef {
        id: ProtocolPackageId::new(id).unwrap(),
        version: ProtocolPackageVersion::new(version).unwrap(),
    }
}

pub(super) fn external_record(
    package: ProtocolPackageRef,
    enabled: bool,
    online: bool,
) -> ProtocolPackageVersionViewModel {
    ProtocolPackageVersionViewModel {
        package,
        name: "External ISO8583".into(),
        host_api: 1,
        kind: ProtocolPackageKindViewModel::Socket,
        source: ProtocolPackageSourceViewModel::External { online },
        enabled,
        validation: ProtocolPackageValidationViewModel::Valid,
        installed_at: Utc.timestamp_opt(1_700_000_000, 0).single().unwrap(),
    }
}

pub(super) fn description(package: ProtocolPackageRef) -> ProtocolPackageDescriptionViewModel {
    let schema = ProtocolPackageSchemaViewModel {
        root: intercept_proxy_domain::DocumentSchemaNode::Object {
            title: Some("External".into()),
            properties: std::collections::BTreeMap::from([(
                "amount".into(),
                intercept_proxy_domain::DocumentSchemaNode::Number {
                    title: Some("Amount".into()),
                },
            )]),
        },
    };
    ProtocolPackageDescriptionViewModel {
        package,
        kind: ProtocolPackageKindViewModel::Socket,
        capabilities: ProtocolPackageCapabilitiesViewModel {
            upstream: ProtocolPackageDirectionCapabilitiesViewModel {
                frame: true,
                decode: true,
                encode: true,
            },
            downstream: ProtocolPackageDirectionCapabilitiesViewModel {
                frame: true,
                decode: true,
                encode: true,
            },
            display: true,
        },
        upstream_schema: Some(schema.clone()),
        downstream_schema: Some(schema),
    }
}

pub(super) fn fixture(
    external: Arc<FakeExternalPackages>,
    usage: Arc<dyn ProtocolPackageUsageQueryPort>,
    runtime: Arc<dyn ListenerRuntimePort>,
) -> Application {
    fixture_with_workspaces(
        external,
        usage,
        runtime,
        Arc::new(InMemoryWorkspaceStore::default()),
    )
}

pub(super) fn fixture_with_workspaces(
    external: Arc<FakeExternalPackages>,
    usage: Arc<dyn ProtocolPackageUsageQueryPort>,
    runtime: Arc<dyn ListenerRuntimePort>,
    workspaces: Arc<InMemoryWorkspaceStore>,
) -> Application {
    let ports = Arc::new(FakePorts::default());
    let mut protocol_packages = unused_protocol_package_services();
    protocol_packages.external = external;
    protocol_packages.usage_query = usage;
    Application::new(
        "External package lifecycle test".into(),
        ApplicationDependencies {
            capture: ports.clone(),
            sessions: ports.clone(),
            breakpoints: Arc::new(BreakpointCoordinator::default()),
            breakpoint_validation: ports.clone(),
            rules: ports.clone(),
            faults: ports.clone(),
            certificates: ports.clone(),
            settings: ports.clone(),
            workspaces,
            listener_runtime: runtime,
            listener_certificates: ports,
            protocol_packages,
            events: Arc::new(EventHub::default()),
            environment_baseline_capture:
                crate::requirements_tests::test_environment_baseline_capture(),
            environment_identity_allocator:
                crate::requirements_tests::test_environment_identity_allocator(),
            environment_apply_lease: crate::requirements_tests::test_environment_apply_lease(),
            environment_material_preparer:
                crate::requirements_tests::test_environment_material_preparer(),
            environment_commit: crate::requirements_tests::test_environment_commit(),
            environment_validator: crate::requirements_tests::test_environment_validator(),
        },
        Arc::new(UnusedAndroidControlPort),
        Arc::new(UnusedProtectedSecretPort),
    )
}

#[derive(Debug, Default)]
pub(super) struct EmptyUsage;

#[async_trait]
impl ProtocolPackageUsageQueryPort for EmptyUsage {
    async fn usages(
        &self,
        _: &ProtocolPackageRef,
    ) -> AppResult<Vec<ProtocolPackageUsageViewModel>> {
        Ok(Vec::new())
    }

    async fn usage_counts(&self) -> AppResult<Vec<ProtocolPackageUsageCount>> {
        Ok(Vec::new())
    }
}

#[derive(Debug)]
pub(super) struct FixedUsage(pub(super) Vec<ProtocolPackageUsageViewModel>);

#[async_trait]
impl ProtocolPackageUsageQueryPort for FixedUsage {
    async fn usages(
        &self,
        _: &ProtocolPackageRef,
    ) -> AppResult<Vec<ProtocolPackageUsageViewModel>> {
        Ok(self.0.clone())
    }

    async fn usage_counts(&self) -> AppResult<Vec<ProtocolPackageUsageCount>> {
        Ok(Vec::new())
    }
}

#[derive(Debug, Default)]
pub(super) struct TrackingRuntime {
    pub(super) stopped: parking_lot::Mutex<Vec<ListenerId>>,
}

#[async_trait]
impl ListenerRuntimePort for TrackingRuntime {
    async fn statuses(&self) -> AppResult<Vec<ListenerStatusViewModel>> {
        Ok(Vec::new())
    }

    async fn start(
        &self,
        _: ProxyWorkspace,
        _: ProxyListener,
    ) -> AppResult<ListenerStatusViewModel> {
        unused()
    }

    async fn stop(&self, listener_id: ListenerId) -> AppResult<ListenerStatusViewModel> {
        self.stopped.lock().push(listener_id);
        Ok(ListenerStatusViewModel {
            listener_id,
            runtime_epoch: None,
            state: ListenerRuntimeState::Stopped,
            state_text: "已停止".into(),
            ui_tone: UiTone::Neutral,
            listen_address: String::new(),
            fault_reason: None,
            can_start: true,
            can_stop: false,
            active_connections: 0,
            client_to_server_bytes: 0,
            server_to_client_bytes: 0,
            retained_diagnostic_evictions: 0,
        })
    }

    async fn replace_rule_definitions(&self, _: ProxyWorkspace, _: ListenerId) -> AppResult<()> {
        unused()
    }

    async fn test_upstream_connection(
        &self,
        _: ProxyWorkspace,
        _: ProxyListener,
    ) -> AppResult<ListenerUpstreamConnectionTestViewModel> {
        unused()
    }

    async fn test_upstream_tls(
        &self,
        _: ProxyWorkspace,
        _: ProxyListener,
    ) -> AppResult<ListenerUpstreamTlsTestViewModel> {
        unused()
    }
}

pub(super) fn usage(
    listener_id: ListenerId,
    runtime_state: ListenerRuntimeState,
) -> ProtocolPackageUsageViewModel {
    ProtocolPackageUsageViewModel {
        workspace_id: WorkspaceId::new(),
        workspace_name: "Workspace".into(),
        listener_id,
        listener_name: "Listener".into(),
        listener_enabled: runtime_state != ListenerRuntimeState::Stopped,
        runtime_state,
    }
}
