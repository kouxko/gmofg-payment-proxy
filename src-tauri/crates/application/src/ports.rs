//! 应用层端口（依赖倒置边界）。
//!
//! 用例只面向这些 trait 编程，代理、仓储、证书和文件等具体实现由 Host 注入。测试可
//! 使用内存实现，TUI/CLI 也无需启动 Tauri 或 `WebView`。

use async_trait::async_trait;

use crate::{
    ActiveFaultViewModel, AppResult, BreakpointDecision, BreakpointDetailViewModel,
    BreakpointDraft, BreakpointValidationViewModel, CaptureDetailViewModel, CapturePageViewModel,
    CaptureQuery, CertificateOverviewViewModel, CertificateValidationViewModel,
    FaultConfigurationDraft, FaultTemplateViewModel, OperationResultViewModel,
    ProxyStatusViewModel, RuleDraft, RuleId, RuleSummaryViewModel, RuleValidationViewModel,
    RuleViewModel, RuntimeEpoch, SessionDetailViewModel, SessionId, SessionPageViewModel,
    SessionQuery, SettingsDraft, SettingsValidationViewModel, SettingsViewModel,
};

#[async_trait]
/// 启停和查询代理运行时的端口。
///
/// application 只发送已经校验的配置，不知道监听器、Tokio 任务或 TLS 的具体实现。
pub trait ProxySupervisorPort: Send + Sync + std::fmt::Debug {
    async fn status(&self) -> AppResult<ProxyStatusViewModel>;
    async fn start(&self, effective_settings: SettingsDraft) -> AppResult<ProxyStatusViewModel>;
    async fn stop(&self) -> AppResult<ProxyStatusViewModel>;
}

#[async_trait]
/// 抓包列表与详情的只读数据端口。
pub trait CaptureRepositoryPort: Send + Sync + std::fmt::Debug {
    async fn query(&self, query: CaptureQuery) -> AppResult<CapturePageViewModel>;
    async fn get_detail(
        &self,
        session_id: SessionId,
        runtime_epoch: RuntimeEpoch,
    ) -> AppResult<CaptureDetailViewModel>;
    async fn clear_view(&self, current_cursor: u64) -> AppResult<u64>;
}

#[async_trait]
/// 规则持久化端口。
///
/// 实现必须保留 revision 并发校验，不能因为换成 TUI/CLI 就绕过规则校验。
pub trait RuleRepositoryPort: Send + Sync + std::fmt::Debug {
    async fn list(&self) -> AppResult<Vec<RuleSummaryViewModel>>;
    async fn get(&self, rule_id: RuleId) -> AppResult<RuleViewModel>;
    async fn new_draft(&self) -> AppResult<RuleDraft>;
    async fn create_from_session(&self, session_id: SessionId) -> AppResult<RuleDraft>;
    async fn validate(&self, draft: &RuleDraft) -> AppResult<RuleValidationViewModel>;
    async fn save(&self, draft: RuleDraft) -> AppResult<RuleViewModel>;
    async fn copy(&self, rule_id: RuleId) -> AppResult<RuleViewModel>;
    async fn delete(
        &self,
        rule_id: RuleId,
        expected_revision: u64,
    ) -> AppResult<OperationResultViewModel>;
    async fn toggle(
        &self,
        rule_id: RuleId,
        expected_revision: u64,
        enabled: bool,
    ) -> AppResult<RuleViewModel>;
    async fn import(&self) -> AppResult<OperationResultViewModel>;
    async fn export(&self) -> AppResult<OperationResultViewModel>;
}

#[async_trait]
/// 将“故障模板”转换为普通规则并管理其生命周期的端口。
pub trait FaultServicePort: Send + Sync + std::fmt::Debug {
    async fn templates(&self) -> AppResult<Vec<FaultTemplateViewModel>>;
    async fn configure(&self, draft: FaultConfigurationDraft) -> AppResult<ActiveFaultViewModel>;
    async fn active(&self) -> AppResult<Vec<ActiveFaultViewModel>>;
    async fn stop(
        &self,
        rule_id: RuleId,
        expected_revision: u64,
    ) -> AppResult<ActiveFaultViewModel>;
}

#[async_trait]
/// 证书生成、导入、导出与校验端口。
///
/// application 只处理用例顺序和错误，不接触私钥字节或平台密钥库。
pub trait CertificateServicePort: Send + Sync + std::fmt::Debug {
    async fn overview(&self) -> AppResult<CertificateOverviewViewModel>;
    async fn generate_ca(&self, sans: Vec<String>) -> AppResult<CertificateOverviewViewModel>;
    async fn export_ca(&self) -> AppResult<OperationResultViewModel>;
    async fn reissue_leaf(
        &self,
        expected_revision: u64,
        sans: Vec<String>,
    ) -> AppResult<CertificateOverviewViewModel>;
    async fn import_pkcs12(&self, password: String) -> AppResult<CertificateOverviewViewModel>;
    async fn import_upstream_ca(&self) -> AppResult<CertificateOverviewViewModel>;
    async fn validate(&self) -> AppResult<CertificateValidationViewModel>;
    async fn reset_ca(&self, expected_revision: u64) -> AppResult<CertificateOverviewViewModel>;
}

#[async_trait]
/// 保存配置与维护“已保存/已生效”双快照的端口。
pub trait SettingsRepositoryPort: Send + Sync + std::fmt::Debug {
    async fn defaults(&self) -> AppResult<SettingsDraft>;
    async fn get(&self) -> AppResult<SettingsViewModel>;
    async fn validate(&self, draft: &SettingsDraft) -> AppResult<SettingsValidationViewModel>;
    async fn save(&self, draft: SettingsDraft) -> AppResult<SettingsViewModel>;
    async fn restore(&self, settings: SettingsViewModel) -> AppResult<SettingsViewModel>;
    async fn apply_effective(&self, settings: SettingsDraft) -> AppResult<SettingsViewModel>;
    async fn clear_effective(&self) -> AppResult<SettingsViewModel>;
}

#[async_trait]
/// 会话导出端口。选择路径和写文件属于平台能力，因此从核心用例中倒置出去。
pub trait FileExportPort: Send + Sync + std::fmt::Debug {
    async fn export_session(
        &self,
        session: SessionDetailViewModel,
        sensitive_data_confirmed: bool,
    ) -> AppResult<OperationResultViewModel>;
}

#[async_trait]
/// 会话查询端口，负责 Rust 侧筛选、排序和分页。
pub trait SessionQueryPort: Send + Sync + std::fmt::Debug {
    async fn query(&self, query: SessionQuery) -> AppResult<SessionPageViewModel>;
    async fn get(&self, session_id: SessionId) -> AppResult<SessionDetailViewModel>;
    async fn clear_completed(&self) -> AppResult<usize>;
}

/// 断点编辑校验端口。
///
/// 任何展示层都只能提交意图，而不能自行重建报文。
pub trait BreakpointValidationPort: Send + Sync + std::fmt::Debug {
    fn format_json(&self, draft: BreakpointDraft) -> AppResult<BreakpointDraft>;
    fn restore_original(&self, detail: &BreakpointDetailViewModel) -> AppResult<BreakpointDraft>;
    fn validate(
        &self,
        detail: &BreakpointDetailViewModel,
        draft: &BreakpointDraft,
    ) -> AppResult<BreakpointValidationViewModel>;
    fn validate_decision(
        &self,
        detail: &BreakpointDetailViewModel,
        decision: &BreakpointDecision,
    ) -> AppResult<BreakpointValidationViewModel>;
}
