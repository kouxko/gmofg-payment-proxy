use std::{collections::BTreeMap, net::TcpListener, path::PathBuf, sync::Arc};

use async_trait::async_trait;
use chrono::Utc;
use intercept_proxy_application::{
    AppResult, BreakpointCoordinator, BreakpointDecision, BreakpointDecisionKind,
    BreakpointDetailViewModel, BreakpointDraft, BreakpointOutcome, BreakpointState,
    BreakpointSummaryViewModel, CaptureQuery, CaptureSort, ChannelId, ChannelSettingsDraft,
    ConnectionHealthState, ConnectionHealthViewModel, FaultConfigurationDraft,
    MessageContentViewModel, MessageStage, PageRequest, ProxyState, ProxyStatusViewModel,
    ProxySupervisorPort, RuleAction, SessionQuery, SessionSort, SettingsDraft, SortDirection,
    UiTone,
};
use intercept_proxy_host::{ApplicationHostBuilder, HostPlatformServices};
use intercept_proxy_infrastructure::{
    InfrastructureError, NativeFileDialog, SecretProtector, adapters::FileSelection,
};
use intercept_proxy_product_api::InterceptProxyProfile;
use intercept_proxy_product_api::{
    BodyCodec, CertificateLabels, ClassifiedRequest, ProductCertificatePolicy, ProductChannel,
    ProductError, ProductFaultTemplate, ProductLabels, ProductMessageContext, ProductProfile,
    ProductStorageNamespace, RequestClassifier,
};
use parking_lot::Mutex;
use uuid::Uuid;

#[derive(Debug)]
struct NoFileDialog;

impl NativeFileDialog for NoFileDialog {
    fn choose_open_file(&self, _purpose: &str) -> AppResult<Option<PathBuf>> {
        Ok(None)
    }

    fn choose_save_file(
        &self,
        _purpose: &str,
        _suggested_file_name: &str,
    ) -> AppResult<Option<FileSelection>> {
        Ok(None)
    }
}

#[derive(Debug)]
struct TestSecretProtector;

impl SecretProtector for TestSecretProtector {
    fn protect(&self, plaintext: &[u8]) -> Result<Vec<u8>, InfrastructureError> {
        Ok(plaintext.iter().map(|byte| byte ^ 0xa5).collect())
    }

    fn unprotect(&self, ciphertext: &[u8]) -> Result<Vec<u8>, InfrastructureError> {
        self.protect(ciphertext)
    }
}

#[derive(Debug, Default)]
struct TestProfile;

#[derive(Debug, Default)]
struct Utf8Codec;

#[derive(Debug, Default)]
struct EmptyClassifier;

#[derive(Debug, Default)]
struct InvalidProfile(TestProfile);

const TEST_CHANNELS: &[ProductChannel] = &[
    ProductChannel {
        id: "alpha",
        display_name: "Alpha",
        enabled_by_default: true,
        listen_port: 21_001,
        upstream_url: "https://alpha.example.test",
    },
    ProductChannel {
        id: "beta",
        display_name: "Beta",
        enabled_by_default: true,
        listen_port: 21_002,
        upstream_url: "https://beta.example.test",
    },
    ProductChannel {
        id: "gamma",
        display_name: "Gamma",
        enabled_by_default: false,
        listen_port: 21_003,
        upstream_url: "https://gamma.example.test",
    },
];

const INVALID_CHANNELS: &[ProductChannel] = &[
    ProductChannel {
        id: "alpha",
        display_name: "Alpha",
        enabled_by_default: true,
        listen_port: 22_001,
        upstream_url: "https://alpha.example.test",
    },
    ProductChannel {
        id: "alpha",
        display_name: "Duplicate",
        enabled_by_default: true,
        listen_port: 22_002,
        upstream_url: "https://duplicate.example.test",
    },
];

const TEST_FAULTS: &[ProductFaultTemplate] = &[ProductFaultTemplate {
    id: "request_delay",
    name: "Test delay",
    stage_text: "Request",
    behavior_text: "Delay before forwarding",
    affected_party_text: "Test client",
    default_channel_id: "alpha",
    risk_text: "Low",
}];

impl BodyCodec for Utf8Codec {
    fn id(&self) -> &'static str {
        "utf-8"
    }

    fn name(&self) -> &'static str {
        "UTF-8"
    }

    fn decode(&self, bytes: &[u8]) -> Result<String, ProductError> {
        String::from_utf8(bytes.to_vec())
            .map_err(|error| ProductError::new("BODY_DECODE_FAILED", error.to_string()))
    }

    fn encode(&self, text: &str) -> Result<Vec<u8>, ProductError> {
        Ok(text.as_bytes().to_vec())
    }
}

impl RequestClassifier for EmptyClassifier {
    fn classify(&self, _: ProductMessageContext<'_>) -> ClassifiedRequest {
        ClassifiedRequest::default()
    }
}

impl ProductCertificatePolicy for TestProfile {
    fn bundled_upstream_ca_pem(&self) -> Option<&'static [u8]> {
        None
    }

    fn labels(&self) -> CertificateLabels {
        CertificateLabels {
            root_name: "Test root",
            root_usage: "Test",
            leaf_name: "Test leaf",
            leaf_usage: "Test",
            client_identity_name: "Test identity",
            client_identity_usage: "Test",
            upstream_name: "Test upstream",
            upstream_bundled_usage: "Test",
            upstream_override_usage: "Test",
            ready_status: "Ready",
            incomplete_status: "Incomplete",
            already_exists_message: "Exists",
            export_cancelled_message: "Cancelled",
            export_success_message: "Exported",
        }
    }
}

impl ProductProfile for TestProfile {
    fn id(&self) -> &'static str {
        "generic-test"
    }

    fn name(&self) -> &'static str {
        "Generic Test"
    }

    fn channels(&self) -> &'static [ProductChannel] {
        TEST_CHANNELS
    }

    fn storage(&self) -> ProductStorageNamespace {
        ProductStorageNamespace {
            database_file_name: "generic-test.sqlite3",
            secret_service: "com.example.generic-test",
            secret_account: "master-key",
            secret_envelope_magic: b"TSTK1",
            secret_aad: b"generic-test/envelope/v1",
        }
    }

    fn labels(&self) -> ProductLabels {
        ProductLabels {
            client_name: "Test Client",
            upstream_name: "Test Upstream",
            fault_rule_name_prefix: "Test fault · ",
        }
    }

    fn fault_templates(&self) -> &'static [ProductFaultTemplate] {
        TEST_FAULTS
    }

    fn request_classifier(&self) -> Arc<dyn RequestClassifier> {
        Arc::new(EmptyClassifier)
    }

    fn certificates(&self) -> &dyn ProductCertificatePolicy {
        self
    }

    fn body_codec(&self) -> Arc<dyn BodyCodec> {
        Arc::new(Utf8Codec)
    }
}

impl ProductProfile for InvalidProfile {
    fn id(&self) -> &'static str {
        "invalid-test"
    }

    fn name(&self) -> &'static str {
        "Invalid Test"
    }

    fn channels(&self) -> &'static [ProductChannel] {
        INVALID_CHANNELS
    }

    fn storage(&self) -> ProductStorageNamespace {
        self.0.storage()
    }

    fn labels(&self) -> ProductLabels {
        ProductProfile::labels(&self.0)
    }

    fn fault_templates(&self) -> &'static [ProductFaultTemplate] {
        TEST_FAULTS
    }

    fn request_classifier(&self) -> Arc<dyn RequestClassifier> {
        self.0.request_classifier()
    }

    fn certificates(&self) -> &dyn ProductCertificatePolicy {
        &self.0
    }

    fn body_codec(&self) -> Arc<dyn BodyCodec> {
        self.0.body_codec()
    }
}

#[derive(Debug)]
struct LifecycleProxy {
    state: Mutex<ProxyState>,
    epoch: Uuid,
}

impl LifecycleProxy {
    fn new(epoch: Uuid) -> Self {
        Self {
            state: Mutex::new(ProxyState::Stopped),
            epoch,
        }
    }
}

#[async_trait]
impl ProxySupervisorPort for LifecycleProxy {
    async fn status(&self) -> AppResult<ProxyStatusViewModel> {
        Ok(proxy_status(*self.state.lock(), self.epoch))
    }

    async fn start(&self, _effective_settings: SettingsDraft) -> AppResult<ProxyStatusViewModel> {
        *self.state.lock() = ProxyState::Running;
        Ok(proxy_status(ProxyState::Running, self.epoch))
    }

    async fn stop(&self) -> AppResult<ProxyStatusViewModel> {
        *self.state.lock() = ProxyState::Stopped;
        Ok(proxy_status(ProxyState::Stopped, self.epoch))
    }
}

fn test_platform() -> HostPlatformServices {
    HostPlatformServices::new(Arc::new(TestSecretProtector), Arc::new(NoFileDialog))
}

fn unused_local_port() -> u16 {
    TcpListener::bind(("127.0.0.1", 0))
        .expect("bind an ephemeral local port")
        .local_addr()
        .expect("read local address")
        .port()
}

fn valid_settings() -> SettingsDraft {
    let mut ports = Vec::new();
    while ports.len() < 3 {
        let port = unused_local_port();
        if !ports.contains(&port) {
            ports.push(port);
        }
    }
    let mut settings = SettingsDraft {
        bind_address: "127.0.0.1".into(),
        channels: TEST_CHANNELS
            .iter()
            .zip(ports)
            .map(|(channel, port)| ChannelSettingsDraft {
                id: ChannelId::new(channel.id).unwrap(),
                display_name: channel.display_name.into(),
                enabled: true,
                port,
                upstream_url: channel.upstream_url.into(),
            })
            .collect(),
        leaf_sans: vec!["127.0.0.1".into()],
        ..SettingsDraft::default()
    };
    settings.channels[2].enabled = false;
    settings
}

fn capture_query() -> CaptureQuery {
    CaptureQuery {
        keyword: Some("  ".into()),
        terminal_ip: None,
        channel: None,
        stage: None,
        result: None,
        rule_id: None,
        after_event_id: None,
        sort: CaptureSort::OccurredAt,
        direction: SortDirection::Desc,
        page: PageRequest {
            page: 0,
            page_size: 0,
        },
    }
}

fn session_query() -> SessionQuery {
    SessionQuery {
        keyword: Some("  ".into()),
        terminal_ip: None,
        channel: None,
        result: None,
        rule_id: None,
        started_from: None,
        started_to: None,
        sort: SessionSort::StartedAt,
        direction: SortDirection::Desc,
        page: PageRequest {
            page: 0,
            page_size: 0,
        },
    }
}

// ARCH-007~009, SETTINGS-001~016, CAPTURE-001~010, SESSION-001~010, TEST-HOST:
// real production adapters are driven exclusively through Application.
