use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use specta::Type;

use super::{
    BreakpointSummaryViewModel, CapturePageViewModel, CaptureRowViewModel,
    CertificateOverviewViewModel, ChannelPresentationViewModel, ListenerStatusViewModel, Revision,
    RuleSummaryViewModel, RuntimeEpoch, SessionSummaryViewModel, SettingsViewModel,
    WorkspaceChangedViewModel,
};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Type)]
/// 应用启动时一次返回的首屏快照，避免界面自行拼接不一致状态。
pub struct AppBootstrapViewModel {
    pub product_name: String,
    pub channel_catalog: Vec<ChannelPresentationViewModel>,
    pub recent_capture: CapturePageViewModel,
    pub pending_breakpoints: Vec<BreakpointSummaryViewModel>,
    pub certificate: CertificateOverviewViewModel,
    pub settings: SettingsViewModel,
    pub event_cursor: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
pub struct SubscriptionAckViewModel {
    pub subscription_id: u64,
    pub accepted_after_event_id: u64,
    pub current_event_id: u64,
    pub snapshot_required: bool,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Type)]
#[serde(tag = "type", content = "data", rename_all = "snake_case")]
/// 所有实时事件的封闭集合；适配器可穷举处理，不依赖字符串事件名。
pub enum UiEventPayload {
    WorkspaceChanged(WorkspaceChangedViewModel),
    ListenerStatusChanged(ListenerStatusViewModel),
    CaptureRowsAdded(Vec<CaptureRowViewModel>),
    /// 一条连接级 Exchange 观测证据已成功写入内存；正文仍由查询接口按需读取。
    ExchangeObservationChanged,
    DiagnosticLogAdded(crate::DiagnosticLogEntryViewModel),
    SessionUpdated(SessionSummaryViewModel),
    BreakpointQueued(BreakpointSummaryViewModel),
    BreakpointResolved(BreakpointSummaryViewModel),
    RuleHit(RuleSummaryViewModel),
    AndroidVpnStatusChanged(crate::AndroidNetworkStatusViewModel),
    CertificateStatusChanged(CertificateOverviewViewModel),
    SettingsChanged(Box<SettingsViewModel>),
    /// 外部软件包服务绑定状态或在线连接数发生变化。
    ExternalPackageServiceStatusChanged(super::ExternalPackageServiceStatusViewModel),
    /// 外部精确版本注册、断线、启停或删除后，目录消费者应重新读取权威快照。
    ProtocolPackageCatalogChanged {
        package: super::ProtocolPackageRef,
    },
    ResourceWarning {
        message: String,
    },
    OperationFailed(crate::AppErrorViewModel),
    SnapshotRequired {
        reason: String,
    },
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Type)]
/// 带顺序、周期和实体版本的实时事件信封。
pub struct UiEventEnvelope {
    pub event_id: u64,
    pub runtime_epoch: Option<RuntimeEpoch>,
    pub occurred_at: DateTime<Utc>,
    pub entity_id: Option<String>,
    pub entity_revision: Option<Revision>,
    pub payload: UiEventPayload,
}

impl UiEventEnvelope {
    pub fn logical_bytes(&self) -> u64 {
        serde_json::to_vec(self).map_or(0, |bytes| bytes.len() as u64)
    }
}
