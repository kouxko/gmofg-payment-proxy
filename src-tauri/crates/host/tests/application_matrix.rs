//! UI 无关 `ApplicationHost` 的产品适配矩阵测试。
//!
//! 这里用测试 `ProductProfile` 和平台服务从组合根建立真实 `Application`，证明核心可被
//! Tauri 之外的入口复用。它验证 Command/use case 语义，但不打开 WebView，也不声称
//! 已完成 Android 真机或 GMO-FG 上游验收。

use std::{collections::BTreeMap, net::TcpListener, path::PathBuf, sync::Arc};

use async_trait::async_trait;
use chrono::Utc;
use gmofg_proxy_application::{
    AppResult, BreakpointCoordinator, BreakpointDecision, BreakpointDecisionKind,
    BreakpointDetailViewModel, BreakpointDraft, BreakpointOutcome, BreakpointState,
    BreakpointSummaryViewModel, CaptureQuery, CaptureSort, ChannelId, ChannelSettingsDraft,
    ConnectionHealthState, ConnectionHealthViewModel, FaultConfigurationDraft,
    MessageContentViewModel, MessageStage, PageRequest, ProxyState, ProxyStatusViewModel,
    ProxySupervisorPort, RuleAction, SessionQuery, SessionSort, SettingsDraft, SortDirection,
    UiTone,
};
use gmofg_proxy_host::{ApplicationHostBuilder, HostPlatformServices};
use gmofg_proxy_infrastructure::{
    InfrastructureError, NativeFileDialog, SecretProtector, adapters::FileSelection,
};
use gmofg_proxy_product_api::{
    BodyCodec, CertificateLabels, ClassifiedRequest, ProductCertificatePolicy, ProductChannel,
    ProductError, ProductFaultTemplate, ProductLabels, ProductMessageContext, ProductProfile,
    ProductStorageNamespace, RequestClassifier,
};
use gmofg_proxy_product_payment::PaymentProductProfile;
use parking_lot::Mutex;
use uuid::Uuid;

#[derive(Debug)]
struct NoFileDialog;

impl NativeFileDialog for NoFileDialog {
    fn choose_open_file(&self, _purpose: &str) -> AppResult<Option<PathBuf>> {
        Ok(None)
    }

    fn choose_save_file(&self, _purpose: &str) -> AppResult<Option<FileSelection>> {
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
    fn public_root_ca_pem(&self) -> &'static [u8] {
        b""
    }

    fn embedded_test_authority(
        &self,
    ) -> Option<gmofg_proxy_product_api::EmbeddedTestCertificateAuthority> {
        None
    }

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
#[tokio::test]
async fn invalid_product_profile_fails_before_storage_is_opened() {
    let temp = tempfile::tempdir().expect("temporary invalid product host");
    let error = ApplicationHostBuilder::new(
        temp.path(),
        test_platform(),
        Arc::new(InvalidProfile::default()),
    )
    .build()
    .await
    .expect_err("duplicate channel profile must fail");
    assert!(matches!(
        error,
        gmofg_proxy_host::HostBuildError::InvalidProductProfile(_)
    ));
    assert!(!temp.path().join("generic-test.sqlite3").exists());
}

#[tokio::test]
async fn production_host_covers_queries_and_settings_without_ui() {
    gmofg_proxy_product_api::validate_product_profile(&TestProfile)
        .expect("non-Payment three-channel profile");
    let temp = tempfile::tempdir().expect("temporary application host");
    let host = ApplicationHostBuilder::new(temp.path(), test_platform(), Arc::new(TestProfile))
        .build()
        .await
        .expect("build UI-neutral host");
    let application = host.application();
    assert!(temp.path().join("generic-test.sqlite3").is_file());
    let bootstrap = application
        .app_bootstrap()
        .await
        .expect("generic three-channel bootstrap");
    assert_eq!(
        bootstrap
            .channel_catalog
            .iter()
            .map(|channel| (channel.id.as_str(), channel.display_name.as_str()))
            .collect::<Vec<_>>(),
        vec![("alpha", "Alpha"), ("beta", "Beta"), ("gamma", "Gamma")]
    );
    let status = application
        .proxy_get_status()
        .await
        .expect("three-channel status");
    assert_eq!(
        status
            .channels
            .iter()
            .map(|channel| (channel.id.as_str(), channel.display_name.as_str()))
            .collect::<Vec<_>>(),
        vec![("alpha", "Alpha"), ("beta", "Beta"), ("gamma", "Gamma")]
    );

    let capture = application
        .capture_query(capture_query())
        .await
        .expect("query empty capture");
    assert!(capture.rows.is_empty());
    assert_eq!(capture.page, 1);
    assert_eq!(capture.page_size, 1);

    let sessions = application
        .session_query(session_query())
        .await
        .expect("query empty sessions");
    assert!(sessions.items.is_empty());
    assert_eq!(sessions.page, 1);
    assert_eq!(sessions.page_size, 1);

    let settings = valid_settings();
    let validation = application
        .settings_validate(settings.clone())
        .await
        .expect("validate settings before certificate setup");
    assert!(validation.valid);
    assert!(
        validation
            .warnings
            .iter()
            .any(|warning| warning.contains("证书材料尚未配置"))
    );
    let saved_settings = application
        .settings_save(settings.clone())
        .await
        .expect("save settings through application");
    assert_eq!(saved_settings.revision, 1);

    let confirmation = application
        .settings_reset_defaults(false)
        .await
        .expect_err("reset defaults requires confirmation");
    assert_eq!(confirmation.view_model.code, "CONFIRMATION_REQUIRED");
    let defaults = application
        .settings_reset_defaults(true)
        .await
        .expect("return Rust-owned defaults");
    assert_eq!(
        defaults
            .channels
            .iter()
            .map(|channel| channel.id.as_str())
            .collect::<Vec<_>>(),
        vec!["alpha", "beta", "gamma"]
    );
    assert_eq!(defaults.channels[0].port, 21_001);

    host.shutdown().await.expect("shutdown UI-neutral host");
}

// ARCH-007~009, RULE-001~017, FAULT-001~011, TEST-HOST:
// rule and fault CRUD use the production SQLite repository and domain validation.
#[tokio::test]
async fn production_host_covers_rule_and_fault_lifecycle_without_ui() {
    let temp = tempfile::tempdir().expect("temporary rule host");
    let host = ApplicationHostBuilder::new(temp.path(), test_platform(), Arc::new(TestProfile))
        .build()
        .await
        .expect("build UI-neutral host");
    let application = host.application();

    let mut rule = application
        .rule_new_draft()
        .await
        .expect("create Rust-owned rule draft");
    rule.name = "无 UI 集成规则".into();
    rule.description = "Application facade matrix".into();
    rule.actions = vec![RuleAction::Delay { milliseconds: 5 }];
    let saved_rule = application.rule_save(rule).await.expect("save rule");
    let rule_id = saved_rule.summary.rule_id;
    assert_eq!(application.rule_list().await.expect("list rules").len(), 1);
    assert_eq!(
        application
            .rule_get(rule_id)
            .await
            .expect("get saved rule")
            .summary
            .name,
        "无 UI 集成规则"
    );

    let disabled = application
        .rule_toggle(rule_id, saved_rule.summary.revision, false)
        .await
        .expect("disable rule");
    assert!(!disabled.summary.enabled);
    let copied = application.rule_copy(rule_id).await.expect("copy rule");
    assert_ne!(copied.summary.rule_id, rule_id);
    let deleted = application
        .rule_delete(rule_id, disabled.summary.revision, true)
        .await
        .expect("delete original rule");
    assert!(deleted.success);

    let templates = application
        .fault_template_list()
        .await
        .expect("list fault templates");
    assert_eq!(templates.len(), 1, "product catalog filters capabilities");
    assert_eq!(templates[0].name, "Test delay");
    let template = templates
        .iter()
        .find(|template| template.template_id == "request_delay")
        .expect("request delay template");
    let active_fault = application
        .fault_configure(FaultConfigurationDraft {
            template_id: template.template_id.clone(),
            existing_rule_id: None,
            expected_revision: None,
            channel: Some(template.default_channel.clone()),
            terminal: Some("10.0.0.8".into()),
            target: Some("/".into()),
            nth_hit: Some(template.default_nth_hit),
            one_shot: template.default_one_shot,
            priority: template.default_priority,
            parameters: template.default_parameters.clone(),
        })
        .await
        .expect("configure fault through real rule repository");
    assert!(active_fault.enabled);
    assert_eq!(
        application
            .fault_active_list()
            .await
            .expect("list active faults")
            .len(),
        1
    );
    let stopped_fault = application
        .fault_stop(active_fault.rule_id, active_fault.revision, true)
        .await
        .expect("stop active fault");
    assert!(!stopped_fault.enabled);

    host.shutdown().await.expect("shutdown UI-neutral host");
}

// ARCH-007~009, CERT-001~020, TEST-HOST:
// certificate generation and validation use the production protected store.
#[tokio::test]
async fn production_host_covers_certificate_overview_and_validation_without_ui() {
    let temp = tempfile::tempdir().expect("temporary certificate host");
    let host = ApplicationHostBuilder::new(
        temp.path(),
        test_platform(),
        Arc::new(PaymentProductProfile::isolated_test_tool()),
    )
    .build()
    .await
    .expect("build UI-neutral host");
    let application = host.application();
    let bootstrap = application
        .app_bootstrap()
        .await
        .expect("Payment two-channel bootstrap");
    assert_eq!(
        bootstrap
            .channel_catalog
            .iter()
            .map(|channel| (channel.id.as_str(), channel.display_name.as_str()))
            .collect::<Vec<_>>(),
        vec![("transaction", "交易"), ("dll", "DLL")]
    );

    let empty_certificates = application
        .certificate_overview()
        .await
        .expect("query initial certificate overview");
    assert!(!empty_certificates.ready);
    assert!(empty_certificates.can_initialize);
    assert_eq!(empty_certificates.items.len(), 1);
    assert!(
        empty_certificates.items[0]
            .usage
            .contains("内置 Payment server.crt")
    );

    let generated = application
        .certificate_generate_ca(vec![" 127.0.0.1 ".into(), "127.0.0.1".into()])
        .await
        .expect("generate CA and leaf through real certificate adapter");
    assert!(!generated.can_initialize);
    assert_eq!(generated.items.len(), 3);
    let certificate_validation = application
        .certificate_validate()
        .await
        .expect("validate incomplete certificate set");
    assert!(!certificate_validation.valid);
    assert!(
        certificate_validation
            .field_errors
            .contains_key("shared_pkcs12")
    );
    assert!(
        !certificate_validation
            .field_errors
            .contains_key("upstream_ca")
    );

    host.shutdown().await.expect("shutdown UI-neutral host");
}

// STATE-001~016, BREAKPOINT-001~016, TEST-STATE, TEST-BREAKPOINT, TEST-HOST:
// only the network supervisor is replaced; Application and breakpoint logic are real.
#[tokio::test]
async fn injected_supervisor_exercises_lifecycle_and_breakpoint_state_without_ui() {
    let epoch = Uuid::from_u128(0x0027_4007_2778);
    let proxy = Arc::new(LifecycleProxy::new(epoch));
    let breakpoints = Arc::new(BreakpointCoordinator::default());
    let temp = tempfile::tempdir().expect("temporary lifecycle host");
    let host = ApplicationHostBuilder::new(temp.path(), test_platform(), Arc::new(TestProfile))
        .with_proxy_supervisor(proxy)
        .with_breakpoint_coordinator(Arc::clone(&breakpoints))
        .build()
        .await
        .expect("build host with deterministic supervisor");
    let application = host.application();

    assert_eq!(
        application
            .proxy_get_status()
            .await
            .expect("initial status")
            .state,
        ProxyState::Stopped
    );
    assert_eq!(
        application.proxy_start().await.expect("start proxy").state,
        ProxyState::Running
    );
    assert_eq!(
        application
            .proxy_restart()
            .await
            .expect("restart proxy")
            .state,
        ProxyState::Running
    );

    let breakpoint_id = Uuid::from_u128(0x48);
    let ticket = breakpoints
        .register(breakpoint_detail(breakpoint_id, epoch))
        .expect("register a pending breakpoint");
    let original = application
        .breakpoint_get(breakpoint_id, epoch)
        .expect("get breakpoint");
    let formatted = application
        .breakpoint_format_json(BreakpointDraft {
            breakpoint_id,
            expected_revision: original.summary.revision,
            message: original.effective.clone(),
        })
        .expect("format JSON in Rust");
    let validation = application
        .breakpoint_validate(&formatted, epoch)
        .expect("validate breakpoint draft");
    assert!(validation.valid);

    let resolved = application
        .breakpoint_resolve(
            epoch,
            BreakpointDecision {
                breakpoint_id,
                expected_revision: original.summary.revision,
                kind: BreakpointDecisionKind::ForwardOriginal,
                message: None,
                delay_ms: None,
                http_status: None,
                content_length_delta: None,
                truncate_at: None,
            },
        )
        .await
        .expect("resolve breakpoint through application");
    assert_eq!(resolved.state, BreakpointState::Resolved);
    assert!(matches!(
        ticket.outcome.await.expect("breakpoint outcome"),
        BreakpointOutcome::Decision(_)
    ));

    assert_eq!(
        application.proxy_stop().await.expect("stop proxy").state,
        ProxyState::Stopped
    );
    host.shutdown().await.expect("shutdown deterministic host");
}

fn message_content() -> MessageContentViewModel {
    let body = br#"{"TransactionType":"0001","RequestID":"R"}"#.to_vec();
    MessageContentViewModel {
        http_status: None,
        start_line_bytes: b"POST / HTTP/1.1".to_vec(),
        raw_headers: Vec::new(),
        headers: BTreeMap::from([
            ("content-type".into(), vec!["application/json".into()]),
            ("content-length".into(), vec![body.len().to_string()]),
        ]),
        body_text: Some(String::from_utf8(body.clone()).expect("ASCII JSON")),
        body_bytes: body,
        json: None,
        content_length: 42,
    }
}

fn breakpoint_detail(breakpoint_id: Uuid, epoch: Uuid) -> BreakpointDetailViewModel {
    BreakpointDetailViewModel {
        summary: BreakpointSummaryViewModel {
            breakpoint_id,
            session_id: Uuid::from_u128(1),
            runtime_epoch: epoch,
            stage: MessageStage::Request,
            title: "请求断点".into(),
            terminal_ip: "10.0.34.94".into(),
            channel: ChannelId::new("beta").unwrap(),
            channel_text: "Beta".into(),
            method: "POST".into(),
            target: "/".into(),
            waiting_since: Utc::now(),
            certificate_fingerprint_suffix: "D4:8".into(),
            state: BreakpointState::Pending,
            state_text: "等待处理".into(),
            ui_tone: UiTone::Warning,
            revision: 1,
        },
        original: message_content(),
        effective: message_content(),
        can_resolve: true,
        resolve_disabled_reason: None,
        available_actions: Vec::new(),
    }
}

fn proxy_status(state: ProxyState, epoch: Uuid) -> ProxyStatusViewModel {
    let (state_text, ui_tone) = state.display_zh();
    ProxyStatusViewModel {
        state,
        state_text: state_text.into(),
        ui_tone,
        runtime_epoch: (state == ProxyState::Running).then_some(epoch),
        revision: 1,
        channels: Vec::new(),
        app_to_proxy_health: ConnectionHealthViewModel {
            state: ConnectionHealthState::Unavailable,
            state_text: "测试替身".into(),
            detail: "无 UI 生命周期测试".into(),
            ui_tone: UiTone::Neutral,
        },
        proxy_to_server_health: ConnectionHealthViewModel {
            state: ConnectionHealthState::Unavailable,
            state_text: "测试替身".into(),
            detail: "无 UI 生命周期测试".into(),
            ui_tone: UiTone::Neutral,
        },
        active_sessions: 0,
        pending_breakpoints: 0,
        logical_memory_bytes: 0,
        logical_memory_text: "0 B".into(),
        memory_capacity_bytes: 256 * 1024 * 1024,
        memory_capacity_text: "256.0 MiB".into(),
        memory_usage_percent: 0,
        session_capacity: 500,
        default_timeout_seconds: 70,
        can_start: state == ProxyState::Stopped,
        start_disabled_reason: None,
        can_stop: state == ProxyState::Running,
        stop_disabled_reason: None,
        can_restart: state == ProxyState::Running,
        restart_disabled_reason: None,
        fault_reason: None,
    }
}
