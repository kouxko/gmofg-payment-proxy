use std::{sync::Arc, time::Duration};

use futures_util::SinkExt;
use intercept_proxy_application::{
    AndroidAdbViewModel, AndroidCompanionInstallViewModel, AndroidControlPort, AndroidDeviceTarget,
    AndroidDeviceViewModel, AndroidNetworkActivation, AndroidNetworkStatusViewModel,
    AndroidPackageViewModel, AndroidRuntimeEndpointViewModel, AndroidRuntimeOwnerMode,
    AndroidRuntimeOwnerSource, AndroidRuntimeOwnerState, AndroidRuntimeOwnerTransitionReason,
    AndroidRuntimeOwnerViewModel, AndroidRuntimeTarget, AppResult,
    EnvironmentApplyBaselineCapturePort, EnvironmentApplyBaselineCaptureRequest,
    EnvironmentCommitTarget, ExternalPackageApplicationPort, HttpBodyProcessing,
    ListenerRuntimePort, ProtocolPackageId, ProtocolPackageRef, ProtocolPackageVersion,
};
use intercept_proxy_domain::{ListenerDataPlane, ProxyWorkspace};
use intercept_proxy_package_contract::{PackageManifest, PackageRegisterNotification};
use intercept_proxy_runtime::NoopPipelinePorts;
use tokio::io::DuplexStream;
use tokio_tungstenite::{
    WebSocketStream,
    tungstenite::{Message, protocol::Role},
};

use super::*;
use crate::{InfrastructureError, SecretProtector, SqliteExecutor, SqliteStore};

#[derive(Debug)]
struct TestProtector;

impl SecretProtector for TestProtector {
    fn protect(&self, plaintext: &[u8]) -> Result<Vec<u8>, InfrastructureError> {
        Ok(plaintext.to_vec())
    }

    fn unprotect(&self, ciphertext: &[u8]) -> Result<Vec<u8>, InfrastructureError> {
        Ok(ciphertext.to_vec())
    }
}

struct RuntimeFixture {
    store: Arc<SqliteStore>,
    listener: Arc<ListenerRuntimeAdapter>,
    external_packages: Arc<ExternalPackageRegistryAdapter>,
    runtime: EnvironmentApplyRuntimeAdapter,
}

#[derive(Debug, Default)]
struct MutableAndroidOwner {
    owner: std::sync::Mutex<Option<AndroidRuntimeOwnerViewModel>>,
}

#[async_trait::async_trait]
impl AndroidControlPort for MutableAndroidOwner {
    async fn adb_get(&self) -> AppResult<AndroidAdbViewModel> {
        unreachable!()
    }
    async fn adb_select(&self, _: String) -> AppResult<AndroidAdbViewModel> {
        unreachable!()
    }
    async fn device_list(&self) -> AppResult<Vec<AndroidDeviceViewModel>> {
        unreachable!()
    }
    async fn package_list(
        &self,
        _: AndroidDeviceTarget,
    ) -> AppResult<Vec<AndroidPackageViewModel>> {
        unreachable!()
    }
    async fn package_get(
        &self,
        _: AndroidDeviceTarget,
        _: String,
    ) -> AppResult<AndroidPackageViewModel> {
        unreachable!()
    }
    async fn companion_install(
        &self,
        _: AndroidDeviceTarget,
        _: bool,
    ) -> AppResult<AndroidCompanionInstallViewModel> {
        unreachable!()
    }
    async fn vpn_open_consent(
        &self,
        _: AndroidDeviceTarget,
    ) -> AppResult<AndroidNetworkStatusViewModel> {
        unreachable!()
    }
    async fn network_start(
        &self,
        _: AndroidDeviceTarget,
        _: AndroidNetworkActivation,
    ) -> AppResult<AndroidNetworkStatusViewModel> {
        unreachable!()
    }
    async fn network_apply(
        &self,
        _: AndroidRuntimeTarget,
        _: AndroidNetworkActivation,
    ) -> AppResult<AndroidNetworkStatusViewModel> {
        unreachable!()
    }
    async fn network_runtime_ready(
        &self,
        _: AndroidDeviceTarget,
        _: &AndroidNetworkActivation,
        _: &AndroidNetworkStatusViewModel,
    ) -> AppResult<bool> {
        unreachable!()
    }
    async fn network_stop(
        &self,
        _: AndroidRuntimeTarget,
    ) -> AppResult<AndroidNetworkStatusViewModel> {
        unreachable!()
    }
    async fn emergency_restore(
        &self,
        _: AndroidRuntimeTarget,
    ) -> AppResult<AndroidNetworkStatusViewModel> {
        unreachable!()
    }
    async fn network_status(
        &self,
        _: AndroidDeviceTarget,
    ) -> AppResult<AndroidNetworkStatusViewModel> {
        unreachable!()
    }
    async fn runtime_owners(&self) -> AppResult<Vec<AndroidRuntimeOwnerViewModel>> {
        Ok(self.owner.lock().unwrap().clone().into_iter().collect())
    }
    async fn network_runtime_endpoints(
        &self,
        _: AndroidDeviceTarget,
        _: Option<AndroidNetworkActivation>,
    ) -> AppResult<Vec<AndroidRuntimeEndpointViewModel>> {
        unreachable!()
    }
}

async fn runtime_fixture() -> RuntimeFixture {
    runtime_fixture_with_builtin(None).await
}

async fn runtime_fixture_with_builtin(archive: Option<Arc<[u8]>>) -> RuntimeFixture {
    let store = Arc::new(SqliteStore::in_memory().unwrap());
    let packages = ProtocolPackageRepositoryAdapter::with_default_limits(store.clone());
    let packages = match archive {
        Some(archive) => packages.with_builtin_archive(archive),
        None => packages,
    };
    packages.ensure_builtin_seeded_async().await.unwrap();
    let packages = Arc::new(packages);
    let secrets = Arc::new(ProtectedSecretAdapter::new(
        store.clone(),
        Arc::new(TestProtector),
    ));
    let gates = Arc::new(EnvironmentApplyResourceGateRegistry::default());
    let external_packages = Arc::new(
        ExternalPackageRegistryAdapter::new(store.clone())
            .with_environment_apply_resource_gates(gates.clone()),
    );
    let listener = Arc::new(
        ListenerRuntimeAdapter::new(store.clone(), secrets, packages.clone())
            .with_environment_apply_resource_gates(gates.clone()),
    );
    listener.set_pipeline_ports(Arc::new(NoopPipelinePorts));
    let android = Arc::new(
        AndroidAdbAdapter::new(None, store.clone())
            .await
            .unwrap()
            .with_environment_apply_resource_gates(gates.clone()),
    );
    let runtime = EnvironmentApplyRuntimeAdapter::new(
        listener.clone(),
        android,
        packages,
        external_packages.clone(),
        SqliteExecutor::new(store.clone()),
        gates,
    );
    RuntimeFixture {
        store,
        listener,
        external_packages,
        runtime,
    }
}

#[path = "revision16_integration/internal_package_baseline.rs"]
mod internal_package_baseline;

#[tokio::test(flavor = "current_thread")]
async fn listener_start_stop_hidden_aba_advances_without_intermediate_observation() {
    let fixture = runtime_fixture().await;
    let reservation = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = reservation.local_addr().unwrap();
    drop(reservation);
    let mut workspace = ProxyWorkspace::default();
    workspace.listeners[0].bind_address = address.ip().to_string();
    workspace.listeners[0].port = address.port();
    let listener = workspace.listeners[0].clone();
    let before = fixture
        .runtime
        .observe_generations(workspace.id.as_uuid())
        .await
        .unwrap()
        .listener;

    fixture
        .listener
        .start(workspace.clone(), listener.clone())
        .await
        .unwrap();
    fixture.listener.stop(listener.id).await.unwrap();
    let after_aba = fixture
        .runtime
        .observe_generations(workspace.id.as_uuid())
        .await
        .unwrap()
        .listener;

    assert!(
        before < after_aba,
        "start-stop must publish mutations and retain a tombstone even when no observer saw running"
    );
}

#[tokio::test(flavor = "current_thread")]
async fn android_none_present_none_retains_a_monotonic_tombstone_generation() {
    let fixture = runtime_fixture().await;
    let android = Arc::new(MutableAndroidOwner::default());
    let packages = Arc::new(ProtocolPackageRepositoryAdapter::with_default_limits(
        fixture.store.clone(),
    ));
    let runtime = EnvironmentApplyRuntimeAdapter::new(
        fixture.listener,
        android.clone(),
        packages,
        fixture.external_packages,
        SqliteExecutor::new(fixture.store),
        Arc::new(EnvironmentApplyResourceGateRegistry::default()),
    );
    let workspace_id = uuid::Uuid::new_v4();
    let before = runtime
        .observe_generations(workspace_id)
        .await
        .unwrap()
        .android;
    *android.owner.lock().unwrap() = Some(AndroidRuntimeOwnerViewModel {
        serial: "DEVICE-G038".into(),
        epoch: uuid::Uuid::new_v4(),
        mode: AndroidRuntimeOwnerMode::AdbReverse,
        profile_id: "profile-g038".into(),
        state: AndroidRuntimeOwnerState::Active,
        source: AndroidRuntimeOwnerSource::Start,
        transition_reason: AndroidRuntimeOwnerTransitionReason::ActivationConfirmed,
        updated_at: chrono::Utc::now(),
    });
    let present = runtime
        .observe_generations(workspace_id)
        .await
        .unwrap()
        .android;
    *android.owner.lock().unwrap() = None;
    let cleared = runtime
        .observe_generations(workspace_id)
        .await
        .unwrap()
        .android;

    assert!(before < present);
    assert!(
        present < cleared,
        "clearing the owner must retain a tombstone and advance rather than hash back to zero"
    );
}

type Peer = WebSocketStream<DuplexStream>;

fn external_registration() -> PackageManifest {
    serde_json::from_value(serde_json::json!({
        "api": 1,
        "kind": "socket",
        "package": {
            "id": "revision16-external", "name": "Revision 16", "version": "1.10.0",
            "description": "external baseline projection"
        },
        "document": {
            "upstream": {"schema": {"type": "object", "title": "Up", "properties": {"mti":{"type":"string","title":"MTI"}}}},
            "downstream": {"schema": {"type": "object", "title": "Down", "properties": {"code":{"type":"string","title":"Code"}}}}
        }
    }))
    .unwrap()
}

async fn connected_client(
    registration: &PackageManifest,
    generation: u64,
) -> (PackageTransportClient, Peer) {
    let (actor_io, peer_io) = tokio::io::duplex(2 * 1024 * 1024);
    let actor = WebSocketStream::from_raw_socket(actor_io, Role::Server, None).await;
    let mut peer = WebSocketStream::from_raw_socket(peer_io, Role::Client, None).await;
    let config = PackageTransportConfig::new(
        Duration::from_secs(30),
        Duration::from_secs(10),
        Duration::from_secs(30),
        1024 * 1024,
        1024 * 1024,
        1024 * 1024,
        128 * 1024,
    );
    let connecting = tokio::spawn(PackageTransportClient::connect(actor, generation, config));
    peer.send(Message::Text(
        serde_json::to_string(&PackageRegisterNotification::new(registration.clone()))
            .unwrap()
            .into(),
    ))
    .await
    .unwrap();
    let (_, client) = connecting.await.unwrap().unwrap();
    (client, peer)
}

fn external_package_ref() -> ProtocolPackageRef {
    ProtocolPackageRef {
        id: ProtocolPackageId::new("revision16-external").unwrap(),
        version: ProtocolPackageVersion::new("1.10.0").unwrap(),
    }
}

#[tokio::test(flavor = "current_thread")]
async fn external_online_offline_online_hidden_aba_advances_without_intermediate_observation() {
    let fixture = runtime_fixture().await;
    let registry = fixture.external_packages.clone();
    let registration = external_registration();
    let package = external_package_ref();
    let fingerprint = external_package_registration_fingerprint(&registration).unwrap();
    let before = fixture
        .runtime
        .observe_generations(uuid::Uuid::new_v4())
        .await
        .unwrap()
        .package;
    let (first, _peer) = connected_client(&registration, 71).await;
    registry
        .accept_registration(&registration, fingerprint, first)
        .await
        .unwrap();
    registry.disconnect(&package).await.unwrap();
    let (second, _peer) = connected_client(&registration, 72).await;
    registry
        .accept_registration(&registration, fingerprint, second)
        .await
        .unwrap();
    let after_aba = fixture
        .runtime
        .observe_generations(uuid::Uuid::new_v4())
        .await
        .unwrap()
        .package;

    assert!(
        before < after_aba,
        "online-offline-online must publish every mutation even without intermediate observation"
    );
    registry.disconnect(&package).await.unwrap();
}

#[tokio::test(flavor = "current_thread")]
async fn baseline_capture_reads_external_online_projection_for_exact_package() {
    let fixture = runtime_fixture().await;
    let registry = fixture.external_packages.clone();
    let registration = external_registration();
    let package = external_package_ref();
    let fingerprint = external_package_registration_fingerprint(&registration).unwrap();
    let (client, _peer) = connected_client(&registration, 73).await;
    registry
        .accept_registration(&registration, fingerprint, client)
        .await
        .unwrap();
    registry.set_enabled(&package, true).await.unwrap();
    let mut candidate = ProxyWorkspace::default();
    let ListenerDataPlane::Http(settings) = &mut candidate.listeners[0].data_plane else {
        panic!("default Listener is HTTP")
    };
    settings.body_processing = HttpBodyProcessing::Protocol {
        package: package.clone(),
    };
    let baseline = fixture
        .runtime
        .capture(EnvironmentApplyBaselineCaptureRequest {
            target: EnvironmentCommitTarget::New {
                workspace_id: candidate.id.as_uuid(),
                display_name: candidate.name.clone(),
            },
            persisted_workspace: None,
            candidate_workspace: candidate,
            schema_version: 1,
            validation_engine_version:
                intercept_proxy_application::ENVIRONMENT_VALIDATION_ENGINE_VERSION,
        })
        .await
        .expect("external projection must be visible to baseline capture");

    assert_eq!(baseline.exact_packages().len(), 1);
    assert_eq!(baseline.exact_packages()[0].package_ref(), &package);
    assert!(baseline.exact_packages()[0].enabled());
    assert!(baseline.exact_packages()[0].online());
    registry.disconnect(&package).await.unwrap();
}

#[tokio::test(flavor = "current_thread")]
async fn service_status_publication_advances_the_exact_package_projection_epoch() {
    let fixture = runtime_fixture().await;
    let registry = fixture.external_packages.clone();
    let registration = external_registration();
    let package = external_package_ref();
    let fingerprint = external_package_registration_fingerprint(&registration).unwrap();
    let (client, _peer) = connected_client(&registration, 74).await;
    registry
        .accept_registration(&registration, fingerprint, client)
        .await
        .unwrap();
    registry.set_enabled(&package, true).await.unwrap();
    let mut candidate = ProxyWorkspace::default();
    let ListenerDataPlane::Http(settings) = &mut candidate.listeners[0].data_plane else {
        panic!("default Listener is HTTP")
    };
    settings.body_processing = HttpBodyProcessing::Protocol {
        package: package.clone(),
    };
    let request = |candidate: ProxyWorkspace| EnvironmentApplyBaselineCaptureRequest {
        target: EnvironmentCommitTarget::New {
            workspace_id: candidate.id.as_uuid(),
            display_name: candidate.name.clone(),
        },
        persisted_workspace: None,
        candidate_workspace: candidate,
        schema_version: 1,
        validation_engine_version:
            intercept_proxy_application::ENVIRONMENT_VALIDATION_ENGINE_VERSION,
    };
    let before = fixture
        .runtime
        .capture(request(candidate.clone()))
        .await
        .unwrap();

    registry
        .mark_service_listening("ws://127.0.0.1:9000/packages")
        .await;
    let after = fixture.runtime.capture(request(candidate)).await.unwrap();

    assert!(before.exact_packages()[0].service_epoch() < after.exact_packages()[0].service_epoch());
    registry.disconnect(&package).await.unwrap();
}
