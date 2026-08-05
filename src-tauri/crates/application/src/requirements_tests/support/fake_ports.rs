use super::*;

fn fake_settings_view() -> SettingsViewModel {
    let stored = SettingsDraft {
        expected_revision: Some(1),
        ..valid_settings_draft()
    };
    SettingsViewModel {
        stored: stored.clone(),
        effective: Some(stored),
        pending_changes: false,
        requires_restart: false,
        restart_reason: None,
        revision: 1,
        can_write: true,
        disabled_reason: None,
        fixed_tls_version: "TLS 1.2".into(),
        redirects_enabled: false,
        retries_enabled: false,
        payload_policy_text: "Payload 仅保存在内存中。".into(),
    }
}

fn fake_certificate_overview() -> CertificateOverviewViewModel {
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
    pub(in crate::requirements_tests) settings_validations: AtomicUsize,
    pub(in crate::requirements_tests) proxy_state: parking_lot::Mutex<ProxyState>,
    pub(in crate::requirements_tests) start_results:
        parking_lot::Mutex<VecDeque<AppResult<ProxyStatusViewModel>>>,
    pub(in crate::requirements_tests) start_calls: AtomicUsize,
    pub(in crate::requirements_tests) stop_calls: AtomicUsize,
    pub(in crate::requirements_tests) block_start: AtomicBool,
    pub(in crate::requirements_tests) start_entered: tokio::sync::Notify,
    pub(in crate::requirements_tests) continue_start: tokio::sync::Notify,
    pub(in crate::requirements_tests) settings_save_calls: AtomicUsize,
    pub(in crate::requirements_tests) certificate_import_calls: AtomicUsize,
    pub(in crate::requirements_tests) certificate_status_calls: AtomicUsize,
    pub(in crate::requirements_tests) certificate_overview_calls: AtomicUsize,
    pub(in crate::requirements_tests) certificate_discard_calls: AtomicUsize,
    pub(in crate::requirements_tests) block_certificate_discard: AtomicBool,
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
            settings_validations: AtomicUsize::new(0),
            proxy_state: parking_lot::Mutex::new(ProxyState::Stopped),
            start_results: parking_lot::Mutex::new(VecDeque::new()),
            start_calls: AtomicUsize::new(0),
            stop_calls: AtomicUsize::new(0),
            block_start: AtomicBool::new(false),
            start_entered: tokio::sync::Notify::new(),
            continue_start: tokio::sync::Notify::new(),
            settings_save_calls: AtomicUsize::new(0),
            certificate_import_calls: AtomicUsize::new(0),
            certificate_status_calls: AtomicUsize::new(0),
            certificate_overview_calls: AtomicUsize::new(0),
            certificate_discard_calls: AtomicUsize::new(0),
            block_certificate_discard: AtomicBool::new(false),
            certificate_discard_entered: tokio::sync::Notify::new(),
            continue_certificate_discard: tokio::sync::Notify::new(),
            discarded_certificate_references: parking_lot::Mutex::new(BTreeSet::new()),
            settings: parking_lot::Mutex::new(fake_settings_view()),
            certificate_overview: parking_lot::Mutex::new(fake_certificate_overview()),
        }
    }
}

#[async_trait]
impl ListenerCertificateImportPort for FakePorts {
    async fn import_downstream_server_identity(
        &self,
        _label: String,
    ) -> AppResult<Option<ListenerCertificateImportViewModel>> {
        Ok(None)
    }

    async fn import_downstream_client_trust(
        &self,
        _label: String,
    ) -> AppResult<Option<ListenerCertificateImportViewModel>> {
        Ok(None)
    }

    async fn import_upstream_client_identity(
        &self,
        _label: String,
        _password: String,
    ) -> AppResult<Option<ListenerCertificateImportViewModel>> {
        Ok(None)
    }

    async fn import_upstream_server_trust(
        &self,
        _label: String,
    ) -> AppResult<Option<ListenerCertificateImportViewModel>> {
        Ok(None)
    }

    async fn inspect(
        &self,
        reference: CertificateReference,
    ) -> AppResult<CertificateItemViewModel> {
        if self
            .discarded_certificate_references
            .lock()
            .contains(&reference.reference)
        {
            return Err(AppError::new(
                "LISTENER_CERTIFICATE_MATERIAL_UNAVAILABLE",
                "托管证书材料已被清理。",
            ));
        }
        Ok(fake_certificate_overview().items.remove(0))
    }

    async fn discard(&self, reference: CertificateReference) -> AppResult<()> {
        self.certificate_discard_calls
            .fetch_add(1, Ordering::SeqCst);
        if self.block_certificate_discard.load(Ordering::SeqCst) {
            self.certificate_discard_entered.notify_one();
            self.continue_certificate_discard.notified().await;
        }
        self.discarded_certificate_references
            .lock()
            .insert(reference.reference);
        Ok(())
    }
}

pub(in crate::requirements_tests) fn unused<T>() -> AppResult<T> {
    Err(AppError::new("UNUSED_FAKE_PORT", "测试未使用此端口。"))
}

#[async_trait]
impl ProxySupervisorPort for FakePorts {
    async fn status(&self) -> AppResult<ProxyStatusViewModel> {
        Ok(proxy_status(*self.proxy_state.lock()))
    }
    async fn start(&self, _: SettingsDraft) -> AppResult<ProxyStatusViewModel> {
        self.start_calls.fetch_add(1, Ordering::SeqCst);
        if self.block_start.load(Ordering::SeqCst) {
            self.start_entered.notify_one();
            self.continue_start.notified().await;
        }
        if let Some(result) = self.start_results.lock().pop_front() {
            if let Ok(status) = &result {
                *self.proxy_state.lock() = status.state;
            }
            return result;
        }
        *self.proxy_state.lock() = ProxyState::Running;
        Ok(proxy_status(ProxyState::Running))
    }
    async fn stop(&self) -> AppResult<ProxyStatusViewModel> {
        self.stop_calls.fetch_add(1, Ordering::SeqCst);
        *self.proxy_state.lock() = ProxyState::Stopped;
        Ok(proxy_status(ProxyState::Stopped))
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
    async fn query(&self, _: SessionQuery) -> AppResult<SessionPageViewModel> {
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
    async fn new_draft(&self) -> AppResult<RuleDraft> {
        Ok(RuleDraft {
            rule_id: None,
            expected_revision: None,
            name: "新建规则".into(),
            description: String::new(),
            enabled: true,
            priority: 100,
            channel: None,
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
        unused()
    }
    async fn save(&self, _: RuleDraft) -> AppResult<RuleViewModel> {
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
    async fn configure(&self, _: FaultConfigurationDraft) -> AppResult<ActiveFaultViewModel> {
        unused()
    }
    async fn active(&self) -> AppResult<Vec<ActiveFaultViewModel>> {
        unused()
    }
    async fn stop(&self, _: RuleId, _: u64) -> AppResult<ActiveFaultViewModel> {
        unused()
    }
}

#[async_trait]
impl CertificateServicePort for FakePorts {
    async fn status(&self) -> AppResult<CertificateOverviewViewModel> {
        self.certificate_status_calls.fetch_add(1, Ordering::SeqCst);
        Ok(self.certificate_overview.lock().clone())
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
        Ok(self.settings.lock().clone())
    }
    async fn validate(&self, _: &SettingsDraft) -> AppResult<SettingsValidationViewModel> {
        self.settings_validations.fetch_add(1, Ordering::SeqCst);
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
        settings.pending_changes = settings
            .effective
            .as_ref()
            .is_some_and(|effective| effective != &settings.stored);
        settings.requires_restart = settings.pending_changes;
        Ok(settings.clone())
    }
    async fn restore(&self, settings: SettingsViewModel) -> AppResult<SettingsViewModel> {
        *self.settings.lock() = settings.clone();
        Ok(settings)
    }
    async fn apply_effective(&self, effective: SettingsDraft) -> AppResult<SettingsViewModel> {
        let mut settings = self.settings.lock();
        settings.effective = Some(effective);
        settings.pending_changes = false;
        settings.requires_restart = false;
        Ok(settings.clone())
    }
    async fn clear_effective(&self) -> AppResult<SettingsViewModel> {
        let mut settings = self.settings.lock();
        settings.effective = None;
        settings.pending_changes = false;
        settings.requires_restart = false;
        Ok(settings.clone())
    }
}

#[async_trait]
impl FileExportPort for FakePorts {
    async fn export_session(
        &self,
        _: SessionDetailViewModel,
        _: bool,
    ) -> AppResult<OperationResultViewModel> {
        unused()
    }
}

pub(in crate::requirements_tests) fn proxy_status(state: ProxyState) -> ProxyStatusViewModel {
    let (state_text, ui_tone) = state.display_zh();
    ProxyStatusViewModel {
        state,
        state_text: state_text.into(),
        ui_tone,
        runtime_epoch: (state == ProxyState::Running).then(|| Uuid::from_u128(20)),
        revision: 1,
        channels: Vec::new(),
        app_to_proxy_health: ConnectionHealthViewModel {
            state: ConnectionHealthState::Unavailable,
            state_text: "未监听".into(),
            detail: "测试状态".into(),
            ui_tone: UiTone::Neutral,
        },
        proxy_to_server_health: ConnectionHealthViewModel {
            state: ConnectionHealthState::Unavailable,
            state_text: "尚未连接".into(),
            detail: "测试状态".into(),
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
        can_restart: false,
        restart_disabled_reason: None,
        fault_reason: None,
    }
}
