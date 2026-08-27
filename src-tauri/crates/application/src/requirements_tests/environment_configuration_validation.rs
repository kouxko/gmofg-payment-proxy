//! G035 RED: Revision 16 layered technical validation contracts.

use std::{
    fs,
    future::pending,
    path::Path,
    sync::{Arc, Mutex},
    time::Duration,
};

use async_trait::async_trait;
use tokio_util::sync::CancellationToken;

use crate::{
    AppError, AppResult, EnvironmentCandidateValidator, EnvironmentStatusCode,
    EnvironmentValidationLayer, EnvironmentValidationLayerPort, EnvironmentValidationLayerRequest,
    EnvironmentValidationStatus,
};

const FULL_SHAPE: &[u8] = include_bytes!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../src/mcp/tests/fixtures/environment_configuration_candidate_v1/full-shape.json"
));

const ORDER: [EnvironmentValidationLayer; 7] = [
    EnvironmentValidationLayer::Schema,
    EnvironmentValidationLayer::Domain,
    EnvironmentValidationLayer::Material,
    EnvironmentValidationLayer::PackageProjection,
    EnvironmentValidationLayer::DnsTcpPort,
    EnvironmentValidationLayer::TlsMtls,
    EnvironmentValidationLayer::PreviewBaseline,
];

#[derive(Clone, Copy)]
enum Behavior {
    Pass,
    Fail(EnvironmentValidationLayer, &'static str),
    Block(EnvironmentValidationLayer),
}

struct RecordingValidationPort {
    behavior: Behavior,
    calls: Mutex<Vec<EnvironmentValidationLayer>>,
    package_refs: Mutex<Vec<(String, String)>>,
    dns_targets: Mutex<Vec<(String, u16)>>,
    tls_targets: Mutex<Vec<(String, u16, Option<String>)>>,
    installation_root_selectors: Mutex<Vec<Option<String>>>,
}

impl RecordingValidationPort {
    fn new(behavior: Behavior) -> Self {
        Self {
            behavior,
            calls: Mutex::new(Vec::new()),
            package_refs: Mutex::new(Vec::new()),
            dns_targets: Mutex::new(Vec::new()),
            tls_targets: Mutex::new(Vec::new()),
            installation_root_selectors: Mutex::new(Vec::new()),
        }
    }

    fn calls(&self) -> Vec<EnvironmentValidationLayer> {
        self.calls.lock().unwrap().clone()
    }
}

#[async_trait]
impl EnvironmentValidationLayerPort for RecordingValidationPort {
    async fn validate_layer(
        &self,
        request: EnvironmentValidationLayerRequest<'_>,
    ) -> AppResult<EnvironmentValidationStatus> {
        let layer = request.layer();
        self.calls.lock().unwrap().push(layer);
        self.package_refs
            .lock()
            .unwrap()
            .extend(request.exact_package_refs().iter().map(|package| {
                (
                    package.id.as_str().to_owned(),
                    package.version.as_str().to_owned(),
                )
            }));
        self.dns_targets.lock().unwrap().extend(
            request
                .dns_tcp_targets()
                .iter()
                .map(|target| (target.host().to_owned(), target.port())),
        );
        self.tls_targets
            .lock()
            .unwrap()
            .extend(request.tls_mtls_targets().iter().map(|target| {
                (
                    target.host().to_owned(),
                    target.port(),
                    target.server_name().map(str::to_owned),
                )
            }));
        self.installation_root_selectors
            .lock()
            .unwrap()
            .push(request.installation_root_selector().map(str::to_owned));

        match self.behavior {
            Behavior::Fail(failed, code) if failed == layer => {
                Err(AppError::new(code, "deterministic validation failure"))
            }
            Behavior::Block(blocked) if blocked == layer => pending().await,
            Behavior::Pass | Behavior::Fail(_, _) | Behavior::Block(_) => {
                Ok(EnvironmentValidationStatus::Passed)
            }
        }
    }
}

fn validator(
    port: Arc<RecordingValidationPort>,
) -> EnvironmentCandidateValidator<RecordingValidationPort> {
    EnvironmentCandidateValidator::new(port).with_total_deadline(Duration::from_secs(30))
}

#[tokio::test]
async fn validates_all_layers_in_revision16_dependency_order() {
    let port = Arc::new(RecordingValidationPort::new(Behavior::Pass));
    let report = validator(Arc::clone(&port))
        .validate(FULL_SHAPE, CancellationToken::new())
        .await;

    assert_eq!(port.calls(), ORDER);
    assert_eq!(report.layers().len(), ORDER.len());
    for (result, expected_layer) in report.layers().iter().zip(ORDER) {
        assert_eq!(result.layer(), expected_layer);
        assert_eq!(result.status(), EnvironmentValidationStatus::Passed);
        assert_eq!(result.code(), None);
        assert_eq!(result.reason(), None);
    }
}

#[tokio::test(start_paused = true)]
async fn total_deadline_cancels_current_layer_and_skips_downstream_layers() {
    let port = Arc::new(RecordingValidationPort::new(Behavior::Block(
        EnvironmentValidationLayer::Material,
    )));
    let task = tokio::spawn({
        let port = Arc::clone(&port);
        async move {
            validator(port)
                .validate(FULL_SHAPE, CancellationToken::new())
                .await
        }
    });
    while port.calls().last() != Some(&EnvironmentValidationLayer::Material) {
        tokio::task::yield_now().await;
    }
    tokio::time::advance(Duration::from_secs(30)).await;
    let report = task.await.unwrap();

    assert_eq!(
        report.status_code(),
        Some(EnvironmentStatusCode::McpCreateDeadlineExceeded)
    );
    assert_eq!(
        report.layers()[0].status(),
        EnvironmentValidationStatus::Passed
    );
    assert_eq!(
        report.layers()[1].status(),
        EnvironmentValidationStatus::Passed
    );
    assert_eq!(
        report.layers()[2].status(),
        EnvironmentValidationStatus::Cancelled
    );
    assert_eq!(
        report.layers()[2].reason(),
        Some("create_deadline_exceeded")
    );
    for result in &report.layers()[3..] {
        assert_eq!(
            result.status(),
            EnvironmentValidationStatus::SkippedDependency
        );
        assert_eq!(result.reason(), Some("create_deadline_exceeded"));
    }
}

#[tokio::test]
async fn request_cancellation_stops_the_inflight_layer_without_starting_dependents() {
    let port = Arc::new(RecordingValidationPort::new(Behavior::Block(
        EnvironmentValidationLayer::PackageProjection,
    )));
    let cancellation = CancellationToken::new();
    let task = tokio::spawn({
        let port = Arc::clone(&port);
        let cancellation = cancellation.clone();
        async move { validator(port).validate(FULL_SHAPE, cancellation).await }
    });
    while port.calls().last() != Some(&EnvironmentValidationLayer::PackageProjection) {
        tokio::task::yield_now().await;
    }
    cancellation.cancel();
    let report = task.await.unwrap();

    assert_eq!(
        report.status_code(),
        Some(EnvironmentStatusCode::CandidateCancelled)
    );
    assert_eq!(port.calls(), ORDER[..=3]);
    assert_eq!(
        report.layers()[3].status(),
        EnvironmentValidationStatus::Cancelled
    );
    for result in &report.layers()[4..] {
        assert_eq!(
            result.status(),
            EnvironmentValidationStatus::SkippedDependency
        );
    }
}

#[tokio::test]
async fn material_parse_failure_uses_exact_code_and_skips_later_layers() {
    let port = Arc::new(RecordingValidationPort::new(Behavior::Fail(
        EnvironmentValidationLayer::Material,
        "CERTIFICATE_PARSE_FAILED",
    )));
    let report = validator(Arc::clone(&port))
        .validate(FULL_SHAPE, CancellationToken::new())
        .await;

    assert_eq!(port.calls(), ORDER[..=2]);
    assert_eq!(
        report.layers()[2].status(),
        EnvironmentValidationStatus::Failed
    );
    assert_eq!(
        report.layers()[2].code(),
        Some(EnvironmentStatusCode::CertificateParseFailed)
    );
    for result in &report.layers()[3..] {
        assert_eq!(
            result.status(),
            EnvironmentValidationStatus::SkippedDependency
        );
    }
}

#[tokio::test]
async fn disabled_exact_package_uses_the_registered_create_validation_code() {
    let port = Arc::new(RecordingValidationPort::new(Behavior::Fail(
        EnvironmentValidationLayer::PackageProjection,
        "PROTOCOL_PACKAGE_DISABLED",
    )));
    let report = validator(port)
        .validate(FULL_SHAPE, CancellationToken::new())
        .await;

    assert_eq!(
        report.status_code(),
        Some(EnvironmentStatusCode::ProtocolPackageDisabled)
    );
}

#[tokio::test]
async fn offline_external_package_uses_the_registered_create_validation_code() {
    let port = Arc::new(RecordingValidationPort::new(Behavior::Fail(
        EnvironmentValidationLayer::PackageProjection,
        "EXTERNAL_PACKAGE_OFFLINE",
    )));
    let report = validator(port)
        .validate(FULL_SHAPE, CancellationToken::new())
        .await;

    assert_eq!(
        report.status_code(),
        Some(EnvironmentStatusCode::ExternalPackageOffline)
    );
}

#[tokio::test]
async fn package_projection_receives_only_exact_typed_refs() {
    let port = Arc::new(RecordingValidationPort::new(Behavior::Pass));
    validator(Arc::clone(&port))
        .validate(FULL_SHAPE, CancellationToken::new())
        .await;

    assert_eq!(
        *port.package_refs.lock().unwrap(),
        vec![("au-eftex".to_owned(), "1.1.0".to_owned())]
    );
}

#[tokio::test]
async fn dns_tcp_precedes_tls_mtls_and_tls_uses_installation_root_selector() {
    let port = Arc::new(RecordingValidationPort::new(Behavior::Pass));
    validator(Arc::clone(&port))
        .validate(FULL_SHAPE, CancellationToken::new())
        .await;

    let calls = port.calls();
    let dns = calls
        .iter()
        .position(|layer| *layer == EnvironmentValidationLayer::DnsTcpPort)
        .unwrap();
    let tls = calls
        .iter()
        .position(|layer| *layer == EnvironmentValidationLayer::TlsMtls)
        .unwrap();
    assert!(dns < tls);
    assert!(
        port.dns_targets
            .lock()
            .unwrap()
            .contains(&("pay.example.test".into(), 443))
    );
    assert!(
        port.tls_targets
            .lock()
            .unwrap()
            .iter()
            .any(|target| target.0 == "pay.example.test" && target.1 == 443)
    );
    assert!(
        port.installation_root_selectors
            .lock()
            .unwrap()
            .iter()
            .any(|selector| selector.as_deref() == Some("installation:root-ca"))
    );
}

#[tokio::test]
async fn public_validation_report_never_contains_candidate_private_values() {
    let port = Arc::new(RecordingValidationPort::new(Behavior::Pass));
    let report = validator(port)
        .validate(FULL_SHAPE, CancellationToken::new())
        .await;
    let serialized = serde_json::to_string(&report).unwrap();

    for private_value in [
        "-----BEGIN PRIVATE KEY-----",
        "-----BEGIN CERTIFICATE-----",
        "fixture-password",
        "operator",
    ] {
        assert!(!serialized.contains(private_value));
    }
}

#[test]
fn validation_sources_forbid_business_or_package_execution_paths() {
    fn read_rust_sources(root: &Path) -> String {
        let mut source = String::new();
        let mut paths = fs::read_dir(root)
            .unwrap()
            .map(|entry| entry.unwrap().path())
            .collect::<Vec<_>>();
        paths.sort();
        for path in paths {
            if path.is_dir() {
                source.push_str(&read_rust_sources(&path));
            } else if path.extension().is_some_and(|extension| extension == "rs") {
                source.push_str(&fs::read_to_string(path).unwrap());
            }
        }
        source
    }

    let root =
        Path::new(env!("CARGO_MANIFEST_DIR")).join("src/environment_configuration/validation");
    let source = if root.is_dir() {
        read_rust_sources(&root)
    } else {
        fs::read_to_string(root.with_extension("rs")).unwrap_or_default()
    };

    for forbidden in [
        "package_rpc",
        "health_probe",
        "business_payload",
        "application_bytes",
        "socket_frame",
        "http_business_body",
        ".decode(",
        ".encode(",
        "mac_cipher",
    ] {
        assert!(
            !source.contains(forbidden),
            "validation must not use `{forbidden}`"
        );
    }
}

#[path = "environment_configuration_validation/domain_contract_red.rs"]
mod domain_contract_red;
#[path = "environment_configuration_validation/existing_active_listener_red.rs"]
mod existing_active_listener_red;
#[path = "environment_configuration_validation/facade_race_red.rs"]
mod facade_race_red;
#[path = "environment_configuration_validation/http_rule_domain_red.rs"]
mod http_rule_domain_red;
#[path = "environment_configuration_validation/limits_red.rs"]
mod limits_red;
#[path = "environment_configuration_validation/packaged_resource_projection.rs"]
mod packaged_resource_projection;
#[path = "environment_configuration_validation/preview_contract_red.rs"]
mod preview_contract_red;
#[path = "environment_configuration_validation/preview_security_red.rs"]
mod preview_security_red;
#[path = "environment_configuration_validation/review_red.rs"]
mod review_red;
#[path = "environment_configuration_validation/runner_worker_red.rs"]
mod runner_worker_red;
#[path = "environment_configuration_validation/selector_parse_red.rs"]
mod selector_parse_red;
#[path = "environment_configuration_validation/selector_projection_red.rs"]
mod selector_projection_red;
