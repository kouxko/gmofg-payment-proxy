//! 应用层端口（依赖倒置边界）。
//!
//! 用例只面向这些 trait 编程，代理、仓储、证书和文件等具体实现由 Host 注入。测试可
//! 使用内存实现，TUI/CLI 也无需启动 Tauri 或 `WebView`。

use async_trait::async_trait;

mod android;
pub use android::AndroidControlPort;
pub(crate) use android::UnavailableAndroidControlPort;

use crate::{
    ActiveFaultViewModel, AppError, AppResult, ApplicationConfigurationDocument,
    BreakpointDecision, BreakpointDetailViewModel, BreakpointDraft, BreakpointValidationViewModel,
    CaptureDetailViewModel, CapturePageViewModel, CaptureQuery, CertificateItemViewModel,
    CertificateOverviewViewModel, CertificateReference, CertificateValidationViewModel,
    FaultConfigurationDraft, FaultTemplateViewModel, ListenerCertificateImportViewModel,
    ListenerId, ListenerStatusViewModel, ListenerUpstreamTlsTestViewModel,
    OperationResultViewModel, ProxyListener, ProxyStatusViewModel, ProxyWorkspace, RuleDraft,
    RuleId, RuleSummaryViewModel, RuleValidationViewModel, RuleViewModel, RuntimeEpoch,
    SecretReference, SessionDetailViewModel, SessionId, SessionPageViewModel, SessionQuery,
    SettingsDraft, SettingsValidationViewModel, SettingsViewModel, WorkspaceId,
    WorkspaceSummaryViewModel, WorkspaceValidationViewModel,
};

#[async_trait]
/// Listener TLS 材料的原生导入边界。
///
/// 实现负责文件选择、格式校验与系统级保护；应用层和展示层只获得可安全写入
/// Workspace 的引用。`None` 表示用户取消选择。
pub trait ListenerCertificateImportPort: Send + Sync + std::fmt::Debug {
    async fn import_downstream_server_identity(
        &self,
        label: String,
    ) -> AppResult<Option<ListenerCertificateImportViewModel>>;

    async fn import_downstream_client_trust(
        &self,
        label: String,
    ) -> AppResult<Option<ListenerCertificateImportViewModel>>;

    async fn import_upstream_client_identity(
        &self,
        label: String,
        password: String,
    ) -> AppResult<Option<ListenerCertificateImportViewModel>>;

    async fn import_upstream_server_trust(
        &self,
        label: String,
    ) -> AppResult<Option<ListenerCertificateImportViewModel>>;

    /// 读取安全引用并只返回证书的公开元数据，不返回路径、密码、私钥或证书原始字节。
    async fn inspect(&self, reference: CertificateReference)
    -> AppResult<CertificateItemViewModel>;

    /// 删除尚未被任何 Workspace 引用的受保护证书材料。
    ///
    /// 应用层必须先完成全局引用检查；基础设施层仍需拒绝非托管引用并校验材料类型，
    /// 防止把该接口扩展成任意文件或任意密钥删除能力。
    async fn discard(&self, reference: CertificateReference) -> AppResult<()>;
}

#[async_trait]
/// 系统密钥保护下的秘密写入边界。
///
/// 展示层只提交本次输入的用户名和密码，并只拿回不可逆的安全引用。明文不会进入
/// Workspace、SQLite、事件、日志或返回 DTO；未来 CLI/TUI 复用同一用例即可。
pub trait ProtectedSecretPort: Send + Sync + std::fmt::Debug {
    async fn store_basic_auth(
        &self,
        username: String,
        password: String,
    ) -> AppResult<SecretReference>;
}

#[derive(Debug)]
pub(crate) struct UnavailableProtectedSecretPort;

#[async_trait]
impl ProtectedSecretPort for UnavailableProtectedSecretPort {
    async fn store_basic_auth(
        &self,
        _username: String,
        _password: String,
    ) -> AppResult<SecretReference> {
        Err(crate::AppError::new(
            "SECRET_PROTECTOR_UNAVAILABLE",
            "当前宿主没有提供系统密钥保护能力。",
        ))
    }
}

#[async_trait]
/// Workspace 的应用层持久化边界。
///
/// 导入导出实现必须只处理领域模型中的安全引用，不能自行附加证书私钥、PKCS#12 密码
/// 或代理认证明文。文件选择仍由 Tauri/Dialog 或未来 CLI 适配器负责。
pub trait WorkspaceRepositoryPort: Send + Sync + std::fmt::Debug {
    async fn list(&self) -> AppResult<Vec<WorkspaceSummaryViewModel>>;
    async fn get(&self, workspace_id: WorkspaceId) -> AppResult<ProxyWorkspace>;
    async fn create(&self, name: String) -> AppResult<ProxyWorkspace>;
    async fn copy(&self, workspace_id: WorkspaceId) -> AppResult<ProxyWorkspace>;
    async fn select(&self, workspace_id: WorkspaceId) -> AppResult<WorkspaceSummaryViewModel>;
    async fn validate(&self, workspace: ProxyWorkspace) -> AppResult<WorkspaceValidationViewModel>;
    async fn save(&self, workspace: ProxyWorkspace) -> AppResult<ProxyWorkspace>;
    async fn delete(
        &self,
        workspace_id: WorkspaceId,
        expected_revision: u64,
    ) -> AppResult<OperationResultViewModel>;
    /// 解析并保存由文档端口读取的内容。
    async fn import_document(&self, document: Vec<u8>) -> AppResult<ProxyWorkspace>;
    /// 序列化不含秘密材料的 `.intercept-workspace` 文档。
    async fn export_document(&self, workspace_id: WorkspaceId) -> AppResult<Vec<u8>>;
}

#[async_trait]
/// Workspace 文件选择与字节 I/O 的平台边界。
///
/// Tauri 实现可使用系统 Dialog，CLI 实现可使用命令行路径；前端既不接触路径，也不接触
/// 文件字节。`None`/`false` 表示用户取消，不是失败。
pub trait WorkspaceDocumentPort: Send + Sync + std::fmt::Debug {
    async fn pick_import_document(&self) -> AppResult<Option<Vec<u8>>>;
    async fn save_export_document(
        &self,
        suggested_file_name: String,
        document: Vec<u8>,
    ) -> AppResult<bool>;
    async fn pick_import_application_configuration(&self) -> AppResult<Option<Vec<u8>>>;
    async fn save_export_application_configuration(
        &self,
        suggested_file_name: String,
        document: Vec<u8>,
    ) -> AppResult<bool>;
}

#[async_trait]
/// 完整应用配置的原子持久化边界。
///
/// 实现必须在同一事务中替换全部 Workspace、当前选择和全局 Settings。调用前文档已由
/// application 全量校验；实现失败时不得留下任何部分写入。
pub trait ApplicationConfigurationStorePort: Send + Sync + std::fmt::Debug {
    async fn replace_all(&self, document: ApplicationConfigurationDocument) -> AppResult<()>;
}

#[derive(Debug, Default)]
pub struct UnavailableApplicationConfigurationStore;

#[async_trait]
impl ApplicationConfigurationStorePort for UnavailableApplicationConfigurationStore {
    async fn replace_all(&self, _: ApplicationConfigurationDocument) -> AppResult<()> {
        Err(AppError::new(
            "APPLICATION_CONFIGURATION_STORE_UNAVAILABLE",
            "当前 Host 未提供完整配置原子存储能力。",
        ))
    }
}

#[async_trait]
/// 每个动态 Listener 的网络生命周期边界。
pub trait ListenerRuntimePort: Send + Sync + std::fmt::Debug {
    async fn statuses(&self) -> AppResult<Vec<ListenerStatusViewModel>>;
    /// 以 application 已校验的不可变 Workspace 快照启动入口。运行时不得再反查 `SQLite`
    /// 或依赖 UI 当前选择状态，这也是未来 CLI/TUI 使用同一核心的稳定边界。
    async fn start(
        &self,
        workspace: ProxyWorkspace,
        listener: ProxyListener,
    ) -> AppResult<ListenerStatusViewModel>;
    async fn stop(&self, listener_id: ListenerId) -> AppResult<ListenerStatusViewModel>;
    /// 使用该入口持久化的上游地址、CA、主机名校验和可选客户端身份执行真实握手。
    async fn test_upstream_tls(
        &self,
        workspace: ProxyWorkspace,
        listener: ProxyListener,
    ) -> AppResult<ListenerUpstreamTlsTestViewModel>;
}

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
    /// 只读取非敏感元数据，用于应用启动状态栏和列表渲染。
    ///
    /// 这个调用不得解密私钥，也不得触发系统钥匙串授权；需要验证证书与私钥是否匹配时
    /// 必须显式调用 [`Self::overview`] 或 [`Self::validate`]。
    async fn status(&self) -> AppResult<CertificateOverviewViewModel>;
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
