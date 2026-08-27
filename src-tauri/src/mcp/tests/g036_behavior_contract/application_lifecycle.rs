use std::sync::{
    Arc,
    atomic::{AtomicUsize, Ordering},
};

use async_trait::async_trait;
use intercept_proxy_application::{
    AppError, AppResult, EnvironmentApplyBaselineCapturePort,
    EnvironmentApplyBaselineCaptureRequest, EnvironmentApplyGenerations, EnvironmentApplyLease,
    EnvironmentApplyLeasePort, EnvironmentApplyLeaseRequest,
    EnvironmentConfigurationApplicationServices, EnvironmentIdentityAllocator,
    EnvironmentMaterialInventoryBaseline, EnvironmentPreparedMaterials,
    EnvironmentProtectedMaterialPreparePort, EnvironmentValidatedApplyBaseline,
    EnvironmentValidatedApplyBaselineCollector, EnvironmentValidationLayerPort,
    EnvironmentValidationLayerRequest, EnvironmentValidationStatus, ExchangeObservationPage,
    ExchangeObservationQueries, ExchangeObservationQuery, ExchangeObservationQueryPort,
    ExchangeObservationRecord, StagedProtectedMaterialHandle,
};
use intercept_proxy_host::{ApplicationHost, ApplicationHostBuilder, HostPlatformServices};
use intercept_proxy_infrastructure::{
    FileSelection, InfrastructureError, NativeFileDialog, SecretProtector,
};
use intercept_proxy_product_api::InterceptProxyProfile;
use serde_json::{Value, json};
use tempfile::TempDir;
use tokio::sync::Notify;
use tokio_util::sync::CancellationToken;

use super::*;
use crate::{
    mcp::{ApplicationBackend, ReadOnlyMcpBackend},
    runtime_logs::RuntimeLogStore,
};

mod production_apply;

static APPLICATION_HOST_LOCK: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());

#[tokio::test(flavor = "current_thread")]
async fn real_application_create_disconnect_cleans_active_and_private_candidate_state() {
    let _guard = APPLICATION_HOST_LOCK.lock().await;
    let controls = Arc::new(Controls::blocked_validation());
    let fixture = LifecycleFixture::new(Arc::clone(&controls)).await;
    let mut stream = write_tool_call_without_reading(
        fixture.server.local_addr(),
        "environment_candidate_create",
        tool_call(
            201,
            "environment_candidate_create",
            &json!({"candidate":minimal_candidate("Disconnect Candidate")}),
        ),
    )
    .await;
    controls.validation_entered.notified().await;
    stream.shutdown().await.expect("close create connection");
    drop(stream);

    tokio::time::timeout(Duration::from_secs(1), async {
        loop {
            let metrics = fixture.application.environment_candidate_metrics();
            if metrics.active_candidates() == 0 && metrics.private_candidate_bytes() == 0 {
                assert!(metrics.retained_terminal_candidates() <= 1);
                break;
            }
            tokio::task::yield_now().await;
        }
    })
    .await
    .expect("disconnect cancellation cleans real Application registry");
    fixture.shutdown().await;
}

#[tokio::test(flavor = "current_thread", start_paused = true)]
async fn real_application_owns_create_deadline_and_cleans_private_candidate_state() {
    let _guard = APPLICATION_HOST_LOCK.lock().await;
    let controls = Arc::new(Controls::blocked_validation());
    let fixture = LifecycleFixture::new(Arc::clone(&controls)).await;
    let address = fixture.server.local_addr();
    let request = tokio::spawn(async move {
        post(
            address,
            "tools/call",
            Some("environment_candidate_create"),
            tool_call(
                206,
                "environment_candidate_create",
                &json!({"candidate":minimal_candidate("Deadline Candidate")}),
            ),
        )
        .await
    });
    controls.validation_entered.notified().await;
    tokio::time::advance(Duration::from_secs(30)).await;
    let response = request.await.expect("deadline response task");

    assert_eq!(response["result"]["isError"], false, "{response}");
    let result = &response["result"]["structuredContent"];
    assert_eq!(result["status"], "validation_failed", "{result}");
    assert_eq!(
        result["errors"][0]["code"], "MCP_CREATE_DEADLINE_EXCEEDED",
        "{result}"
    );
    let metrics = fixture.application.environment_candidate_metrics();
    assert_eq!(metrics.active_candidates(), 0);
    assert_eq!(metrics.private_candidate_bytes(), 0);
    assert!(metrics.retained_terminal_candidates() <= 1);
    fixture.shutdown().await;
}

#[tokio::test(flavor = "current_thread")]
async fn real_application_apply_ack_disconnect_survives_and_worker_wins_to_terminal_cleanup() {
    let _guard = APPLICATION_HOST_LOCK.lock().await;
    let controls = Arc::new(Controls::passing_validation());
    let fixture = LifecycleFixture::new(Arc::clone(&controls)).await;
    let created = fixture.create("Worker Wins").await;
    assert_eq!(created["status"], "preview_ready", "{created}");
    let candidate_id = created["candidate_id"].as_str().expect("candidate id");
    let confirmation_token = created["confirmation_token"].as_str().expect("token");

    let ack = fixture
        .call(
            202,
            "environment_candidate_apply",
            json!({"candidate_id":candidate_id,"confirmation_token":confirmation_token}),
        )
        .await;
    assert_eq!(ack["status"], "apply_queued", "{ack}");
    controls.prepare_entered.notified().await;

    let status = fixture
        .call(
            203,
            "environment_candidate_status",
            json!({"candidate_id":candidate_id}),
        )
        .await;
    assert_eq!(status["status"], "apply_in_progress", "{status}");
    let cancelled = fixture
        .call(
            204,
            "environment_candidate_cancel",
            json!({"candidate_id":candidate_id}),
        )
        .await;
    assert_eq!(
        cancelled["status"], "apply_in_progress_not_cancellable",
        "{cancelled}"
    );

    controls.prepare_release.notify_waiters();
    let terminal = fixture.wait_for_terminal(candidate_id).await;
    assert_eq!(terminal["status"], "failed_before_commit", "{terminal}");
    assert_eq!(controls.prepare_calls.load(Ordering::SeqCst), 1);
    assert_eq!(controls.commit_calls.load(Ordering::SeqCst), 0);
    let metrics = fixture.application.environment_candidate_metrics();
    assert_eq!(metrics.active_candidates(), 0);
    assert_eq!(metrics.active_apply_tasks(), 0);
    assert_eq!(metrics.private_candidate_bytes(), 0);
    fixture.shutdown().await;
}

#[tokio::test(flavor = "current_thread")]
async fn real_application_cancel_before_worker_claim_prevents_prepare_and_commit() {
    let _guard = APPLICATION_HOST_LOCK.lock().await;
    let controls = Arc::new(Controls::passing_validation());
    let fixture = LifecycleFixture::new(Arc::clone(&controls)).await;
    let created = fixture.create("Cancel Wins").await;
    let candidate_id = created["candidate_id"].as_str().expect("candidate id");
    let confirmation_token = created["confirmation_token"].as_str().expect("token");
    let context = McpCallContext {
        request_cancellation: CancellationToken::new(),
        transport_capabilities: fixture.server.transport_capabilities(),
    };
    let ack = fixture
        .backend
        .call_tool_with_context(
            "environment_candidate_apply",
            json!({"candidate_id":candidate_id,"confirmation_token":confirmation_token}),
            context,
        )
        .await
        .expect("real backend queues apply");
    assert_eq!(ack["status"], "apply_queued", "{ack}");
    let cancelled = fixture.application.environment_candidate_cancel(
        &intercept_proxy_application::EnvironmentCandidateId::new(candidate_id.to_owned())
            .expect("candidate id parses"),
    );
    assert_eq!(
        serde_json::to_value(cancelled).unwrap()["status"],
        "cancelled"
    );
    tokio::task::yield_now().await;
    assert_eq!(controls.prepare_calls.load(Ordering::SeqCst), 0);
    assert_eq!(controls.commit_calls.load(Ordering::SeqCst), 0);
    controls.prepare_release.notify_waiters();
    fixture.shutdown().await;
}

struct LifecycleFixture {
    _directory: TempDir,
    host: ApplicationHost,
    application: Arc<intercept_proxy_application::Application>,
    backend: Arc<ApplicationBackend>,
    server: ReadOnlyMcpServer,
}

impl LifecycleFixture {
    async fn new(controls: Arc<Controls>) -> Self {
        let directory = TempDir::new().expect("temporary Host data directory");
        let host = ApplicationHostBuilder::new(
            directory.path(),
            HostPlatformServices::new(Arc::new(TestSecrets), Arc::new(NoopDialog)),
            Arc::new(InterceptProxyProfile),
        )
        .with_environment_configuration_services(EnvironmentConfigurationApplicationServices {
            baseline_capture: controls.clone(),
            identity_allocator: EnvironmentIdentityAllocator::random(),
            apply_lease: controls.clone(),
            material_preparer: controls.clone(),
            commit: controls.clone(),
            validator: controls,
        })
        .build()
        .await
        .expect("build real Application Host");
        let application = host.application();
        let backend = Arc::new(ApplicationBackend::new(
            Arc::clone(&application),
            Arc::new(RuntimeLogStore::memory(32)),
            ExchangeObservationQueries::new(Arc::new(NoopObservations)),
        ));
        let server = start_test_server(backend.clone())
            .await
            .expect("start real Application MCP server");
        Self {
            _directory: directory,
            host,
            application,
            backend,
            server,
        }
    }

    async fn create(&self, name: &str) -> Value {
        self.call(
            200,
            "environment_candidate_create",
            json!({"candidate":minimal_candidate(name)}),
        )
        .await
    }

    async fn call(&self, id: usize, name: &str, arguments: Value) -> Value {
        let response = post(
            self.server.local_addr(),
            "tools/call",
            Some(name),
            tool_call(id, name, &arguments),
        )
        .await;
        assert_eq!(response["result"]["isError"], false, "{response}");
        response["result"]["structuredContent"].clone()
    }

    async fn wait_for_terminal(&self, candidate_id: &str) -> Value {
        tokio::time::timeout(Duration::from_secs(1), async {
            loop {
                let status = self
                    .call(
                        205,
                        "environment_candidate_status",
                        json!({"candidate_id":candidate_id}),
                    )
                    .await;
                if status["status"] == "failed_before_commit" {
                    return status;
                }
                tokio::task::yield_now().await;
            }
        })
        .await
        .expect("worker reaches terminal state")
    }

    async fn shutdown(self) {
        self.server.shutdown().await;
        self.host.shutdown().await.expect("shutdown real Host");
    }
}

struct Controls {
    block_validation: bool,
    validation_entered: Notify,
    prepare_entered: Notify,
    prepare_release: Notify,
    prepare_calls: AtomicUsize,
    commit_calls: AtomicUsize,
}

impl Controls {
    fn blocked_validation() -> Self {
        Self::new(true)
    }

    fn passing_validation() -> Self {
        Self::new(false)
    }

    fn new(block_validation: bool) -> Self {
        Self {
            block_validation,
            validation_entered: Notify::new(),
            prepare_entered: Notify::new(),
            prepare_release: Notify::new(),
            prepare_calls: AtomicUsize::new(0),
            commit_calls: AtomicUsize::new(0),
        }
    }
}

#[async_trait]
impl EnvironmentValidationLayerPort for Controls {
    async fn validate_layer(
        &self,
        _request: EnvironmentValidationLayerRequest<'_>,
    ) -> AppResult<EnvironmentValidationStatus> {
        if self.block_validation {
            self.validation_entered.notify_one();
            std::future::pending().await
        } else {
            Ok(EnvironmentValidationStatus::Passed)
        }
    }
}

#[async_trait]
impl EnvironmentApplyBaselineCapturePort for Controls {
    async fn capture(
        &self,
        request: EnvironmentApplyBaselineCaptureRequest,
    ) -> AppResult<EnvironmentValidatedApplyBaseline> {
        let workspace_id = match request.target {
            intercept_proxy_application::EnvironmentCommitTarget::New { workspace_id, .. }
            | intercept_proxy_application::EnvironmentCommitTarget::Existing {
                workspace_id, ..
            } => workspace_id,
        };
        EnvironmentValidatedApplyBaselineCollector::collect(
            workspace_id,
            EnvironmentApplyGenerations {
                application_mutation: 1,
                ..EnvironmentApplyGenerations::default()
            },
            request.persisted_workspace_structural_hash(),
            Vec::new(),
            Vec::new(),
            Vec::new(),
            vec![EnvironmentMaterialInventoryBaseline::observed(
                "empty-fixture".into(),
                [1; 32],
            )],
        )
    }
}

#[async_trait]
impl EnvironmentApplyLeasePort for Controls {
    async fn acquire(
        &self,
        _request: EnvironmentApplyLeaseRequest,
    ) -> AppResult<EnvironmentApplyLease> {
        Ok(EnvironmentApplyLease::acquired(
            EnvironmentApplyGenerations {
                application_mutation: 1,
                ..EnvironmentApplyGenerations::default()
            },
        ))
    }
}

#[async_trait]
impl EnvironmentProtectedMaterialPreparePort for Controls {
    async fn prepare(
        &self,
        _staged: StagedProtectedMaterialHandle,
    ) -> AppResult<EnvironmentPreparedMaterials> {
        self.prepare_calls.fetch_add(1, Ordering::SeqCst);
        self.prepare_entered.notify_one();
        self.prepare_release.notified().await;
        Err(AppError::new(
            "PROTECTED_MATERIAL_PREPARE_FAILED",
            "controlled prepare failure",
        ))
    }
}

#[async_trait]
impl intercept_proxy_application::EnvironmentCommitPort for Controls {
    async fn commit(
        &self,
        _request: intercept_proxy_application::EnvironmentCommitRequest,
    ) -> Result<
        intercept_proxy_application::EnvironmentCommitResult,
        intercept_proxy_application::EnvironmentCommitFailure,
    > {
        self.commit_calls.fetch_add(1, Ordering::SeqCst);
        unreachable!("controlled preparer fails before commit")
    }
}

fn minimal_candidate(name: &str) -> Value {
    json!({
        "schema_version":1,
        "target":{"mode":"new","name":name},
        "workspace":{"listeners":[],"http_rules":[],"protocol_rules":[],"android_network_profiles":[]},
        "materials":{"certificates":[],"secrets":[]}
    })
}

#[derive(Debug)]
struct NoopObservations;

impl ExchangeObservationQueryPort for NoopObservations {
    fn query(&self, _query: &ExchangeObservationQuery) -> ExchangeObservationPage {
        ExchangeObservationPage {
            rows: Vec::new(),
            page: 1,
            page_size: 1,
            total: 0,
            evicted_records: 0,
            dropped_events: 0,
            ignored_events: 0,
        }
    }

    fn get(&self, _exchange_id: &str) -> Option<ExchangeObservationRecord> {
        None
    }
}

#[derive(Debug)]
struct NoopDialog;

impl NativeFileDialog for NoopDialog {
    fn choose_open_file(&self, _purpose: &str) -> AppResult<Option<std::path::PathBuf>> {
        Ok(None)
    }

    fn choose_save_file(&self, _purpose: &str, _name: &str) -> AppResult<Option<FileSelection>> {
        Ok(None)
    }
}

#[derive(Debug)]
struct TestSecrets;

impl SecretProtector for TestSecrets {
    fn protect(&self, plaintext: &[u8]) -> Result<Vec<u8>, InfrastructureError> {
        Ok(plaintext.to_vec())
    }

    fn unprotect(&self, ciphertext: &[u8]) -> Result<Vec<u8>, InfrastructureError> {
        Ok(ciphertext.to_vec())
    }
}
