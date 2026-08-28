use super::*;

fn fake_settings_view() -> SettingsViewModel {
    let stored = SettingsDraft {
        expected_revision: Some(1),
        ..valid_settings_draft()
    };
    SettingsViewModel {
        stored,
        revision: 1,
        can_write: true,
        disabled_reason: None,
        fixed_tls_version: "TLS 1.2".into(),
        redirects_enabled: false,
        retries_enabled: false,
        payload_policy_text: "Payload 仅保存在内存中。".into(),
    }
}

pub(super) fn fake_certificate_overview() -> CertificateOverviewViewModel {
    CertificateOverviewViewModel {
        revision: 1,
        ready: true,
        status_text: "证书已就绪".into(),
        ui_tone: UiTone::Positive,
        items: vec![CertificateItemViewModel {
            kind: "Proxy 叶子证书".into(),
            subject: "CN=proxy.test".into(),
            usage: "App → Proxy TLS 服务端身份".into(),
            sans: vec!["127.0.0.1".into()],
            valid_from: None,
            valid_until: None,
            sha256_fingerprint: "fingerprint".into(),
            status_text: "有效".into(),
            ui_tone: UiTone::Positive,
        }],
        can_initialize: false,
        can_change: true,
        disabled_reason: None,
    }
}

#[derive(Debug)]
pub(in crate::requirements_tests) struct FakePorts {
    pub(in crate::requirements_tests) rule_validation_calls: AtomicUsize,
    pub(in crate::requirements_tests) rule_save_calls: AtomicUsize,
    pub(in crate::requirements_tests) settings_validations: AtomicUsize,
    pub(in crate::requirements_tests) settings_validation_override:
        parking_lot::Mutex<Option<SettingsValidationViewModel>>,
    pub(in crate::requirements_tests) settings_save_calls: AtomicUsize,
    pub(in crate::requirements_tests) settings_get_calls: AtomicUsize,
    pub(in crate::requirements_tests) certificate_import_calls: AtomicUsize,
    pub(in crate::requirements_tests) certificate_status_calls: AtomicUsize,
    pub(in crate::requirements_tests) certificate_overview_calls: AtomicUsize,
    pub(in crate::requirements_tests) certificate_discard_calls: AtomicUsize,
    pub(in crate::requirements_tests) certificate_restore_calls: AtomicUsize,
    pub(in crate::requirements_tests) certificate_preflight_calls: AtomicUsize,
    pub(in crate::requirements_tests) certificate_generation: parking_lot::Mutex<[u8; 32]>,
    pub(in crate::requirements_tests) block_certificate_discard: AtomicBool,
    pub(in crate::requirements_tests) fail_certificate_discard: AtomicBool,
    pub(in crate::requirements_tests) certificate_discard_entered: tokio::sync::Notify,
    pub(in crate::requirements_tests) continue_certificate_discard: tokio::sync::Notify,
    pub(in crate::requirements_tests) discarded_certificate_references:
        parking_lot::Mutex<BTreeSet<String>>,
    pub(in crate::requirements_tests) settings: parking_lot::Mutex<SettingsViewModel>,
    pub(in crate::requirements_tests) certificate_overview:
        parking_lot::Mutex<CertificateOverviewViewModel>,
}

impl Default for FakePorts {
    fn default() -> Self {
        Self {
            rule_validation_calls: AtomicUsize::new(0),
            rule_save_calls: AtomicUsize::new(0),
            settings_validations: AtomicUsize::new(0),
            settings_validation_override: parking_lot::Mutex::new(None),
            settings_save_calls: AtomicUsize::new(0),
            settings_get_calls: AtomicUsize::new(0),
            certificate_import_calls: AtomicUsize::new(0),
            certificate_status_calls: AtomicUsize::new(0),
            certificate_overview_calls: AtomicUsize::new(0),
            certificate_discard_calls: AtomicUsize::new(0),
            certificate_restore_calls: AtomicUsize::new(0),
            certificate_preflight_calls: AtomicUsize::new(0),
            certificate_generation: parking_lot::Mutex::new([0; 32]),
            block_certificate_discard: AtomicBool::new(false),
            fail_certificate_discard: AtomicBool::new(false),
            certificate_discard_entered: tokio::sync::Notify::new(),
            continue_certificate_discard: tokio::sync::Notify::new(),
            discarded_certificate_references: parking_lot::Mutex::new(BTreeSet::new()),
            settings: parking_lot::Mutex::new(fake_settings_view()),
            certificate_overview: parking_lot::Mutex::new(fake_certificate_overview()),
        }
    }
}

pub(in crate::requirements_tests) fn unused<T>() -> AppResult<T> {
    Err(AppError::new("UNUSED_FAKE_PORT", "测试未使用此端口。"))
}

#[derive(Debug, Default)]
pub(in crate::requirements_tests) struct UnusedExternalPackagePort;

#[async_trait]
impl ExternalPackageApplicationPort for UnusedExternalPackagePort {
    async fn service_status(&self) -> AppResult<ExternalPackageServiceStatusViewModel> {
        Ok(ExternalPackageServiceStatusViewModel {
            websocket_url: "ws://0.0.0.0:8765/packages".into(),
            fixed_path: "/packages".into(),
            online_connection_count: 0,
            state: ExternalPackageServiceStateViewModel::Listening,
            authentication_enabled: false,
        })
    }

    async fn list(&self) -> AppResult<Vec<ProtocolPackageVersionViewModel>> {
        Ok(Vec::new())
    }

    async fn get(
        &self,
        _: &ProtocolPackageRef,
    ) -> AppResult<Option<ProtocolPackageVersionViewModel>> {
        Ok(None)
    }

    async fn describe(
        &self,
        _: &ProtocolPackageRef,
    ) -> AppResult<ProtocolPackageDescriptionViewModel> {
        unused()
    }

    async fn detail(&self, _: &ProtocolPackageRef) -> AppResult<ExternalPackageDetailViewModel> {
        unused()
    }

    async fn set_enabled(&self, _: &ProtocolPackageRef, _: bool) -> AppResult<()> {
        unused()
    }

    async fn disconnect(&self, _: &ProtocolPackageRef) -> AppResult<()> {
        unused()
    }

    async fn delete(&self, _: &ProtocolPackageRef) -> AppResult<()> {
        unused()
    }
}

#[derive(Debug, Default)]
pub(in crate::requirements_tests) struct NoopApplicationConfigurationStore;

#[async_trait]
impl ApplicationConfigurationStorePort for NoopApplicationConfigurationStore {
    async fn replace_all(&self, _: ApplicationConfigurationDocument) -> AppResult<()> {
        Ok(())
    }
}

#[derive(Debug, Default)]
pub(in crate::requirements_tests) struct UnusedProtectedSecretPort;

#[async_trait]
impl ProtectedSecretPort for UnusedProtectedSecretPort {
    async fn store_basic_auth(&self, _: String, _: String) -> AppResult<SecretReference> {
        unused()
    }
}

#[derive(Debug, Default)]
pub(in crate::requirements_tests) struct UnusedAndroidControlPort;

#[async_trait]
impl AndroidControlPort for UnusedAndroidControlPort {
    async fn adb_get(&self) -> AppResult<AndroidAdbViewModel> {
        unused()
    }
    async fn adb_select(&self, _: String) -> AppResult<AndroidAdbViewModel> {
        unused()
    }
    async fn device_list(&self) -> AppResult<Vec<AndroidDeviceViewModel>> {
        unused()
    }
    async fn package_list(
        &self,
        _: AndroidDeviceTarget,
    ) -> AppResult<Vec<AndroidPackageViewModel>> {
        unused()
    }
    async fn package_get(
        &self,
        _: AndroidDeviceTarget,
        _: String,
    ) -> AppResult<AndroidPackageViewModel> {
        unused()
    }
    async fn companion_install(
        &self,
        _: AndroidDeviceTarget,
        _: bool,
    ) -> AppResult<AndroidCompanionInstallViewModel> {
        unused()
    }
    async fn vpn_open_consent(
        &self,
        _: AndroidDeviceTarget,
    ) -> AppResult<AndroidNetworkStatusViewModel> {
        unused()
    }
    async fn network_start(
        &self,
        _: AndroidDeviceTarget,
        _: AndroidNetworkActivation,
    ) -> AppResult<AndroidNetworkStatusViewModel> {
        unused()
    }
    async fn network_apply(
        &self,
        _: AndroidRuntimeTarget,
        _: AndroidNetworkActivation,
    ) -> AppResult<AndroidNetworkStatusViewModel> {
        unused()
    }
    async fn network_runtime_ready(
        &self,
        _: AndroidDeviceTarget,
        _: &AndroidNetworkActivation,
        _: &AndroidNetworkStatusViewModel,
    ) -> AppResult<bool> {
        unused()
    }
    async fn network_stop(
        &self,
        _: AndroidRuntimeTarget,
    ) -> AppResult<AndroidNetworkStatusViewModel> {
        unused()
    }
    async fn emergency_restore(
        &self,
        _: AndroidRuntimeTarget,
    ) -> AppResult<AndroidNetworkStatusViewModel> {
        unused()
    }
    async fn network_status(
        &self,
        _: AndroidDeviceTarget,
    ) -> AppResult<AndroidNetworkStatusViewModel> {
        Err(AppError::new(
            "ANDROID_DEVICE_NOT_SELECTED",
            "测试场景没有选择 Android 设备。",
        ))
    }
    async fn runtime_owners(&self) -> AppResult<Vec<AndroidRuntimeOwnerViewModel>> {
        Ok(Vec::new())
    }
    async fn network_runtime_endpoints(
        &self,
        _: AndroidDeviceTarget,
        _: Option<AndroidNetworkActivation>,
    ) -> AppResult<Vec<AndroidRuntimeEndpointViewModel>> {
        Ok(Vec::new())
    }
}

#[async_trait]
impl CaptureRepositoryPort for FakePorts {
    async fn query(&self, _: CaptureQuery) -> AppResult<CapturePageViewModel> {
        Ok(CapturePageViewModel {
            rows: Vec::new(),
            total: 0,
            page: 1,
            page_size: 5,
            total_pages: 0,
            event_cursor: 0,
            oldest_event_id: None,
            runtime_epoch: None,
            snapshot_required: false,
            empty_message: "暂无抓包记录。".into(),
        })
    }
    async fn get_detail(&self, _: SessionId, _: RuntimeEpoch) -> AppResult<CaptureDetailViewModel> {
        unused()
    }
    async fn clear_view(&self, _: u64) -> AppResult<u64> {
        unused()
    }
}

#[async_trait]
impl SessionQueryPort for FakePorts {
    async fn query(&self, _: SessionQuery) -> AppResult<SessionListViewModel> {
        unused()
    }
    async fn get(&self, _: SessionId) -> AppResult<SessionDetailViewModel> {
        unused()
    }
    async fn clear_completed(&self) -> AppResult<usize> {
        unused()
    }
}

impl BreakpointValidationPort for FakePorts {
    fn format_json(&self, _: BreakpointDraft) -> AppResult<BreakpointDraft> {
        unused()
    }
    fn normalize(&self, _: BreakpointDraft) -> AppResult<BreakpointDraft> {
        unused()
    }
    fn restore_original(&self, _: &BreakpointDetailViewModel) -> AppResult<BreakpointDraft> {
        unused()
    }
    fn validate(
        &self,
        _: &BreakpointDetailViewModel,
        _: &BreakpointDraft,
    ) -> AppResult<BreakpointValidationViewModel> {
        unused()
    }
    fn validate_decision(
        &self,
        _: &BreakpointDetailViewModel,
        _: &BreakpointDecision,
    ) -> AppResult<BreakpointValidationViewModel> {
        unused()
    }
}

#[async_trait]
impl RuleRepositoryPort for FakePorts {
    async fn list(&self) -> AppResult<Vec<RuleSummaryViewModel>> {
        unused()
    }
    async fn get(&self, _: RuleId) -> AppResult<RuleViewModel> {
        unused()
    }
    async fn new_http_draft(&self, channel: ChannelId) -> AppResult<RuleDraft> {
        Ok(RuleDraft {
            rule_id: None,
            expected_revision: None,
            name: "新建规则".into(),
            description: String::new(),
            enabled: true,
            priority: 100,
            channel: Some(channel),
            stage: Some(MessageStage::Request),
            conditions: Vec::new(),
            actions: Vec::new(),
            one_shot: false,
        })
    }
    async fn create_from_session(&self, _: SessionId) -> AppResult<RuleDraft> {
        unused()
    }
    async fn validate(&self, _: &RuleDraft) -> AppResult<RuleValidationViewModel> {
        self.rule_validation_calls.fetch_add(1, Ordering::SeqCst);
        Ok(FieldValidationViewModel {
            valid: true,
            field_errors: BTreeMap::new(),
            warnings: Vec::new(),
        })
    }
    async fn save(&self, _: RuleDraft) -> AppResult<RuleViewModel> {
        self.rule_save_calls.fetch_add(1, Ordering::SeqCst);
        unused()
    }
    async fn copy(&self, _: RuleId) -> AppResult<RuleViewModel> {
        unused()
    }
    async fn delete(&self, _: RuleId, _: u64) -> AppResult<OperationResultViewModel> {
        unused()
    }
    async fn toggle(&self, _: RuleId, _: u64, _: bool) -> AppResult<RuleViewModel> {
        unused()
    }
    async fn import(&self) -> AppResult<OperationResultViewModel> {
        unused()
    }
    async fn export(&self) -> AppResult<OperationResultViewModel> {
        unused()
    }
}

#[async_trait]
impl FaultServicePort for FakePorts {
    async fn templates(&self) -> AppResult<Vec<FaultTemplateViewModel>> {
        unused()
    }
    async fn rule_draft(&self, _: FaultConfigurationDraft) -> AppResult<RuleDefinitionSaveInput> {
        unused()
    }

    fn active_view(&self, _: &RuleDefinition) -> Option<ActiveFaultViewModel> {
        None
    }
}

#[async_trait]
impl CertificateServicePort for FakePorts {
    async fn status(&self) -> AppResult<CertificateOverviewViewModel> {
        self.certificate_status_calls.fetch_add(1, Ordering::SeqCst);
        Ok(self.certificate_overview.lock().clone())
    }

    async fn synchronize_installation_ca(
        &self,
        _: Vec<String>,
    ) -> AppResult<CertificateOverviewViewModel> {
        CertificateServicePort::status(self).await
    }

    async fn overview(&self) -> AppResult<CertificateOverviewViewModel> {
        self.certificate_overview_calls
            .fetch_add(1, Ordering::SeqCst);
        Ok(self.certificate_overview.lock().clone())
    }
    async fn generate_ca(&self, _: Vec<String>) -> AppResult<CertificateOverviewViewModel> {
        unused()
    }
    async fn export_ca(&self) -> AppResult<OperationResultViewModel> {
        unused()
    }
    async fn reissue_leaf(
        &self,
        _: u64,
        _: Vec<String>,
    ) -> AppResult<CertificateOverviewViewModel> {
        unused()
    }
    async fn import_pkcs12(&self, _: String) -> AppResult<CertificateOverviewViewModel> {
        self.certificate_import_calls.fetch_add(1, Ordering::SeqCst);
        Ok(fake_certificate_overview())
    }
    async fn import_upstream_ca(&self) -> AppResult<CertificateOverviewViewModel> {
        unused()
    }
    async fn validate(&self) -> AppResult<CertificateValidationViewModel> {
        Ok(FieldValidationViewModel {
            valid: true,
            field_errors: BTreeMap::new(),
            warnings: Vec::new(),
        })
    }
    async fn reset_ca(&self, _: u64) -> AppResult<CertificateOverviewViewModel> {
        unused()
    }
}

#[async_trait]
impl SettingsRepositoryPort for FakePorts {
    async fn defaults(&self) -> AppResult<SettingsDraft> {
        Ok(valid_settings_draft())
    }

    async fn get(&self) -> AppResult<SettingsViewModel> {
        self.settings_get_calls.fetch_add(1, Ordering::SeqCst);
        Ok(self.settings.lock().clone())
    }
    async fn validate(&self, _: &SettingsDraft) -> AppResult<SettingsValidationViewModel> {
        self.settings_validations.fetch_add(1, Ordering::SeqCst);
        if let Some(validation) = self.settings_validation_override.lock().clone() {
            return Ok(validation);
        }
        Ok(FieldValidationViewModel {
            valid: true,
            field_errors: BTreeMap::new(),
            warnings: Vec::new(),
        })
    }
    async fn save(&self, mut draft: SettingsDraft) -> AppResult<SettingsViewModel> {
        self.settings_save_calls.fetch_add(1, Ordering::SeqCst);
        let mut settings = self.settings.lock();
        settings.revision = settings.revision.saturating_add(1);
        draft.expected_revision = Some(settings.revision);
        settings.stored = draft;
        Ok(settings.clone())
    }
}
