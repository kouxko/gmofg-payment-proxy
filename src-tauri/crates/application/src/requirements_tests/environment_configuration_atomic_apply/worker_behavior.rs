use std::{
    future::Future,
    pin::Pin,
    sync::{
        Arc, Mutex,
        atomic::{AtomicUsize, Ordering},
    },
    task::{Context, Poll, Waker},
};

use async_trait::async_trait;
use intercept_proxy_domain::ProxyWorkspace;
use serde_json::Value;
use tokio::sync::Notify;
use uuid::Uuid;

use crate::environment_configuration::EnvironmentApplyWorker;
use crate::{
    AppError, AppResult, EnvironmentAffectedListenerBaseline, EnvironmentApplyGenerations,
    EnvironmentApplyLease, EnvironmentApplyLeasePort, EnvironmentApplyLeaseRequest,
    EnvironmentCandidateEpoch, EnvironmentCandidatePublicSnapshot, EnvironmentCandidateRegistry,
    EnvironmentCommitFailure, EnvironmentCommitPort, EnvironmentCommitRequest,
    EnvironmentCommitResult, EnvironmentCommitRollbackOutcome, EnvironmentCommitTarget,
    EnvironmentConfirmationToken, EnvironmentExactPackageBaseline,
    EnvironmentMaterialInventoryBaseline, EnvironmentPreparedMaterials,
    EnvironmentProtectedMaterialPreparePort, EnvironmentValidatedApplyBaseline,
    StagedProtectedMaterialHandle, parse_environment_configuration_candidate_v1,
};

mod baseline;

const FULL_SHAPE: &[u8] = include_bytes!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../src/mcp/tests/fixtures/environment_configuration_candidate_v1/full-shape.json"
));
const EXPECTED_PREVIEW: &[u8] = include_bytes!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../src/mcp/tests/fixtures/environment_configuration_candidate_v1/expected-preview.json"
));

enum LeaseOutcome {
    Acquired(EnvironmentApplyGenerations),
    PackageStale(EnvironmentApplyGenerations),
    GenerationMismatch(EnvironmentApplyGenerations),
    Unavailable,
}

struct FakeLease {
    outcome: Mutex<Option<LeaseOutcome>>,
    requests: Mutex<Vec<EnvironmentApplyLeaseRequest>>,
    called: Notify,
}

impl FakeLease {
    fn new(outcome: LeaseOutcome) -> Self {
        Self {
            outcome: Mutex::new(Some(outcome)),
            requests: Mutex::new(Vec::new()),
            called: Notify::new(),
        }
    }
}

#[async_trait]
impl EnvironmentApplyLeasePort for FakeLease {
    async fn acquire(
        &self,
        request: EnvironmentApplyLeaseRequest,
    ) -> AppResult<EnvironmentApplyLease> {
        self.requests.lock().unwrap().push(request);
        self.called.notify_one();
        match self.outcome.lock().unwrap().take().expect("one acquire") {
            LeaseOutcome::Acquired(observed) => Ok(EnvironmentApplyLease::acquired(observed)),
            LeaseOutcome::PackageStale(observed) => {
                Ok(EnvironmentApplyLease::package_stale(observed))
            }
            LeaseOutcome::GenerationMismatch(observed) => {
                Ok(EnvironmentApplyLease::generation_mismatch(observed))
            }
            LeaseOutcome::Unavailable => Err(AppError::new(
                "APPLY_LEASE_UNAVAILABLE",
                "lease unavailable",
            )),
        }
    }
}

enum PrepareOutcome {
    Success,
    Failure,
}

struct FakePrepare {
    outcome: PrepareOutcome,
    calls: AtomicUsize,
}

#[async_trait]
impl EnvironmentProtectedMaterialPreparePort for FakePrepare {
    async fn prepare(
        &self,
        _staged: StagedProtectedMaterialHandle,
    ) -> AppResult<EnvironmentPreparedMaterials> {
        self.calls.fetch_add(1, Ordering::SeqCst);
        match self.outcome {
            PrepareOutcome::Success => {
                Ok(EnvironmentPreparedMaterials::without_materials_for_test(
                    EnvironmentCommitTarget::New {
                        workspace_id: Uuid::from_u128(0x38),
                        display_name: "G038".into(),
                    },
                    ProxyWorkspace::default(),
                ))
            }
            PrepareOutcome::Failure => Err(AppError::new(
                "PROTECTED_MATERIAL_PREPARE_FAILED",
                "prepare failed",
            )),
        }
    }
}

enum CommitOutcome {
    Success,
    BeforeTransaction(&'static str),
    RolledBack(EnvironmentCommitRollbackOutcome),
    BlockingSuccess {
        started: Arc<Notify>,
        release: Arc<Notify>,
    },
}

struct FakeCommit {
    outcome: CommitOutcome,
    calls: AtomicUsize,
    baselines: Mutex<Vec<EnvironmentApplyGenerations>>,
}

#[async_trait]
impl EnvironmentCommitPort for FakeCommit {
    async fn commit(
        &self,
        request: EnvironmentCommitRequest,
    ) -> Result<EnvironmentCommitResult, EnvironmentCommitFailure> {
        self.calls.fetch_add(1, Ordering::SeqCst);
        self.baselines
            .lock()
            .unwrap()
            .push(request.baseline().clone());
        match &self.outcome {
            CommitOutcome::Success => Ok(EnvironmentCommitResult {
                workspace_id: Uuid::from_u128(0x38),
                revision: 1,
                selected_workspace_id: Some(Uuid::from_u128(0x38)),
                reused_materials: 0,
                inserted_materials: 0,
            }),
            CommitOutcome::BeforeTransaction(code) => Err(
                EnvironmentCommitFailure::before_transaction(AppError::new(*code, "commit failed")),
            ),
            CommitOutcome::RolledBack(outcome) => Err(EnvironmentCommitFailure::rolled_back(
                AppError::new("COMMIT_ROLLED_BACK", "commit failed"),
                *outcome,
            )),
            CommitOutcome::BlockingSuccess { started, release } => {
                started.notify_one();
                release.notified().await;
                Ok(EnvironmentCommitResult {
                    workspace_id: Uuid::from_u128(0x38),
                    revision: 1,
                    selected_workspace_id: Some(Uuid::from_u128(0x38)),
                    reused_materials: 0,
                    inserted_materials: 0,
                })
            }
        }
    }
}

fn token(create: &crate::EnvironmentCandidateCreateResult) -> EnvironmentConfirmationToken {
    let value = serde_json::to_value(create).expect("create serializes");
    EnvironmentConfirmationToken::new(
        value["confirmation_token"]
            .as_str()
            .expect("token")
            .to_owned(),
    )
    .expect("token parses")
}

pub(super) fn queued_registry_from_candidate(
    candidate: crate::EnvironmentConfigurationCandidateV1,
) -> (EnvironmentCandidateRegistry, crate::EnvironmentCandidateId) {
    let registry = EnvironmentCandidateRegistry::default();
    let inserted = registry
        .insert_validating(candidate, EnvironmentCandidateEpoch::new(91))
        .expect("insert");
    let snapshot = EnvironmentCandidatePublicSnapshot::from_validated_json(EXPECTED_PREVIEW)
        .expect("snapshot");
    let baseline = EnvironmentValidatedApplyBaseline::validated(
        EnvironmentApplyGenerations {
            application_mutation: 38,
            package: 39,
            ..EnvironmentApplyGenerations::default()
        },
        [38; 32],
        vec![EnvironmentAffectedListenerBaseline::observed(
            Uuid::from_u128(0x38),
            Some(Uuid::from_u128(0x39)),
            0,
        )],
        Vec::new(),
        vec![EnvironmentExactPackageBaseline::observed(
            crate::ProtocolPackageRef {
                id: crate::ProtocolPackageId::new("au-eftex").unwrap(),
                version: crate::ProtocolPackageVersion::new("1.1.0").unwrap(),
            },
            Uuid::from_u128(0x40),
            true,
            true,
        )],
        vec![EnvironmentMaterialInventoryBaseline::frozen(
            "fixture-certificate".into(),
            [0x41; 32],
        )],
    );
    let workspace = ProxyWorkspace::default();
    workspace.validate().expect("commit aggregate validates");
    registry
        .attach_validated_apply_baseline(inserted.candidate_id(), baseline)
        .expect("baseline attached");
    let ready = registry
        .complete_preview_ready(inserted.candidate_id(), snapshot, workspace)
        .expect("ready");
    registry
        .queue_apply(ready.candidate_id(), &token(&ready))
        .expect("queued");
    (registry, ready.candidate_id().clone())
}

fn queued_registry() -> (EnvironmentCandidateRegistry, crate::EnvironmentCandidateId) {
    queued_registry_from_candidate(
        parse_environment_configuration_candidate_v1(FULL_SHAPE).expect("candidate"),
    )
}

fn status(registry: &EnvironmentCandidateRegistry, id: &crate::EnvironmentCandidateId) -> Value {
    serde_json::to_value(registry.status(id)).expect("status serializes")
}

async fn run_worker(
    registry: EnvironmentCandidateRegistry,
    lease: Arc<FakeLease>,
    prepare: Arc<FakePrepare>,
    commit: Arc<FakeCommit>,
) {
    EnvironmentApplyWorker::new(registry.clone(), lease.clone(), prepare, commit).spawn_once();
    lease.called.notified().await;
    registry.begin_shutdown().await;
}

fn poll_once<F: Future>(future: Pin<&mut F>) -> Poll<F::Output> {
    future.poll(&mut Context::from_waker(Waker::noop()))
}

#[tokio::test]
async fn unavailable_lease_terminalizes_before_prepare_or_commit() {
    let (registry, id) = queued_registry();
    let lease = Arc::new(FakeLease::new(LeaseOutcome::Unavailable));
    let prepare = Arc::new(FakePrepare {
        outcome: PrepareOutcome::Success,
        calls: AtomicUsize::new(0),
    });
    let commit = Arc::new(FakeCommit {
        outcome: CommitOutcome::Success,
        calls: AtomicUsize::new(0),
        baselines: Mutex::new(Vec::new()),
    });

    run_worker(registry.clone(), lease, prepare.clone(), commit.clone()).await;

    assert_eq!(prepare.calls.load(Ordering::SeqCst), 0);
    assert_eq!(commit.calls.load(Ordering::SeqCst), 0);
    assert_eq!(
        status(&registry, &id)["terminal_result"]["status_code"],
        "APPLY_LEASE_UNAVAILABLE"
    );
}

#[tokio::test]
async fn package_stale_terminalizes_without_prepare_or_commit() {
    let (registry, id) = queued_registry();
    let lease = Arc::new(FakeLease::new(LeaseOutcome::PackageStale(
        EnvironmentApplyGenerations::default(),
    )));
    let prepare = Arc::new(FakePrepare {
        outcome: PrepareOutcome::Success,
        calls: AtomicUsize::new(0),
    });
    let commit = Arc::new(FakeCommit {
        outcome: CommitOutcome::Success,
        calls: AtomicUsize::new(0),
        baselines: Mutex::new(Vec::new()),
    });

    run_worker(registry.clone(), lease, prepare.clone(), commit.clone()).await;

    assert_eq!(prepare.calls.load(Ordering::SeqCst), 0);
    assert_eq!(commit.calls.load(Ordering::SeqCst), 0);
    assert_eq!(status(&registry, &id)["status"], "stale");
    assert_eq!(
        status(&registry, &id)["terminal_result"]["status_code"],
        "CANDIDATE_STALE"
    );
}

#[tokio::test]
async fn generation_mismatch_is_failed_before_commit() {
    let (registry, id) = queued_registry();
    let lease = Arc::new(FakeLease::new(LeaseOutcome::GenerationMismatch(
        EnvironmentApplyGenerations::default(),
    )));
    let prepare = Arc::new(FakePrepare {
        outcome: PrepareOutcome::Success,
        calls: AtomicUsize::new(0),
    });
    let commit = Arc::new(FakeCommit {
        outcome: CommitOutcome::Success,
        calls: AtomicUsize::new(0),
        baselines: Mutex::new(Vec::new()),
    });

    run_worker(registry.clone(), lease, prepare, commit).await;

    assert_eq!(status(&registry, &id)["status"], "failed_before_commit");
    assert_eq!(
        status(&registry, &id)["terminal_result"]["status_code"],
        "APPLY_LEASE_MISMATCH"
    );
}

#[tokio::test]
async fn prepare_failure_terminalizes_before_commit() {
    let (registry, id) = queued_registry();
    let lease = Arc::new(FakeLease::new(LeaseOutcome::Acquired(
        EnvironmentApplyGenerations::default(),
    )));
    let prepare = Arc::new(FakePrepare {
        outcome: PrepareOutcome::Failure,
        calls: AtomicUsize::new(0),
    });
    let commit = Arc::new(FakeCommit {
        outcome: CommitOutcome::Success,
        calls: AtomicUsize::new(0),
        baselines: Mutex::new(Vec::new()),
    });

    run_worker(registry.clone(), lease, prepare, commit.clone()).await;

    assert_eq!(commit.calls.load(Ordering::SeqCst), 0);
    assert_eq!(status(&registry, &id)["status"], "failed_before_commit");
    assert_eq!(
        status(&registry, &id)["terminal_result"]["status_code"],
        "PROTECTED_MATERIAL_PREPARE_FAILED"
    );
}

#[tokio::test]
async fn commit_baseline_mismatch_is_a_rolled_back_terminal() {
    let (registry, id) = queued_registry();
    let lease = Arc::new(FakeLease::new(LeaseOutcome::Acquired(
        EnvironmentApplyGenerations::default(),
    )));
    let prepare = Arc::new(FakePrepare {
        outcome: PrepareOutcome::Success,
        calls: AtomicUsize::new(0),
    });
    let commit = Arc::new(FakeCommit {
        outcome: CommitOutcome::RolledBack(EnvironmentCommitRollbackOutcome::BaselineMismatch),
        calls: AtomicUsize::new(0),
        baselines: Mutex::new(Vec::new()),
    });

    run_worker(registry.clone(), lease, prepare, commit).await;

    assert_eq!(status(&registry, &id)["status"], "rolled_back");
    assert_eq!(
        status(&registry, &id)["terminal_result"]["status_code"],
        "COMMIT_BASELINE_MISMATCH"
    );
}

#[tokio::test]
async fn commit_failure_before_transaction_is_failed_before_commit() {
    let (registry, id) = queued_registry();
    let lease = Arc::new(FakeLease::new(LeaseOutcome::Acquired(
        EnvironmentApplyGenerations::default(),
    )));
    let prepare = Arc::new(FakePrepare {
        outcome: PrepareOutcome::Success,
        calls: AtomicUsize::new(0),
    });
    let commit = Arc::new(FakeCommit {
        outcome: CommitOutcome::BeforeTransaction("COMMIT_FAILED"),
        calls: AtomicUsize::new(0),
        baselines: Mutex::new(Vec::new()),
    });

    run_worker(registry.clone(), lease, prepare, commit).await;

    assert_eq!(status(&registry, &id)["status"], "failed_before_commit");
    assert_eq!(
        status(&registry, &id)["terminal_result"]["status_code"],
        "COMMIT_FAILED"
    );
}

#[tokio::test]
async fn successful_commit_is_the_only_committed_terminal_path() {
    let (registry, id) = queued_registry();
    let lease = Arc::new(FakeLease::new(LeaseOutcome::Acquired(
        EnvironmentApplyGenerations::default(),
    )));
    let prepare = Arc::new(FakePrepare {
        outcome: PrepareOutcome::Success,
        calls: AtomicUsize::new(0),
    });
    let commit = Arc::new(FakeCommit {
        outcome: CommitOutcome::Success,
        calls: AtomicUsize::new(0),
        baselines: Mutex::new(Vec::new()),
    });

    run_worker(registry.clone(), lease, prepare, commit).await;

    let terminal = status(&registry, &id)["terminal_result"].clone();
    assert_eq!(terminal["result"], "committed");
    assert_eq!(terminal["workspace_id"], Uuid::from_u128(0x38).to_string());
    assert_eq!(terminal["revision"], 1);
    assert!(terminal["status_code"].is_null());
}

#[tokio::test]
async fn caller_release_does_not_cancel_owned_work_and_shutdown_waits_for_commit() {
    let (registry, id) = queued_registry();
    let lease = Arc::new(FakeLease::new(LeaseOutcome::Acquired(
        EnvironmentApplyGenerations::default(),
    )));
    let prepare = Arc::new(FakePrepare {
        outcome: PrepareOutcome::Success,
        calls: AtomicUsize::new(0),
    });
    let started = Arc::new(Notify::new());
    let release = Arc::new(Notify::new());
    let commit = Arc::new(FakeCommit {
        outcome: CommitOutcome::BlockingSuccess {
            started: started.clone(),
            release: release.clone(),
        },
        calls: AtomicUsize::new(0),
        baselines: Mutex::new(Vec::new()),
    });

    EnvironmentApplyWorker::new(registry.clone(), lease, prepare, commit).spawn_once();
    started.notified().await;
    let mut shutdown = Box::pin(registry.begin_shutdown());
    assert!(poll_once(shutdown.as_mut()).is_pending());

    release.notify_one();
    shutdown.await;

    assert_eq!(status(&registry, &id)["status"], "committed");
}
