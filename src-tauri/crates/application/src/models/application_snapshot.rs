use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use specta::Type;

use super::{
    DiagnosticLogPageViewModel, ExternalPackageServiceStatusViewModel, ListenerStatusViewModel,
    ProtocolPackageGroupViewModel, ProxyWorkspace, SettingsViewModel, WorkspaceSummaryViewModel,
};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
/// 由 Application 在 mutation gate 内一次编排得到的只读应用快照。
///
/// `generation` 是除观察时间和 generation 本身以外所有返回字段的观察指纹，仅用于比较
/// 两份返回内容，不是数据库 revision 或强冲突令牌。持久化配置在整个读取窗口内不会被其他
/// Application 写用例修改；运行态字段各读取一次，代表该窗口内的关联观察。
pub struct ApplicationSnapshotViewModel {
    pub generation: String,
    pub observed_at: DateTime<Utc>,
    pub settings: SettingsViewModel,
    pub workspaces: Vec<WorkspaceSummaryViewModel>,
    pub workspace_details: Vec<ProxyWorkspace>,
    pub entry_statuses: Vec<ListenerStatusViewModel>,
    pub protocol_packages: Vec<ProtocolPackageGroupViewModel>,
    pub external_package_service: ExternalPackageServiceStatusViewModel,
    pub diagnostics: DiagnosticLogPageViewModel,
}
