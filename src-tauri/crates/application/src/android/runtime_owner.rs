use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use specta::Type;
use uuid::Uuid;

/// Android 网络运行态的实际设备所有者。
/// 它与 UI 当前选择的设备完全独立；停止、状态查询和恢复只能以这里记录的 serial 为目标。
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize, Type)]
#[serde(rename_all = "snake_case")]
pub enum AndroidRuntimeOwnerMode {
    DeviceOnly,
    Lan,
    AdbReverse,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize, Type)]
#[serde(rename_all = "snake_case")]
pub enum AndroidRuntimeOwnerState {
    Active,
    Uncertain,
    WaitingReconnect,
    CleanupRequired,
    StopFailed,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize, Type)]
#[serde(rename_all = "snake_case")]
pub enum AndroidRuntimeOwnerSource {
    Start,
    Apply,
    Recovery,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize, Type)]
#[serde(rename_all = "snake_case")]
pub enum AndroidRuntimeOwnerTransitionReason {
    ActivationConfirmed,
    ActivationUncertain,
    ReversePreparation,
    ReverseCleanupRequired,
    DeviceDisconnected,
    DeviceReconnected,
    StopFailed,
    RecoveredFromStorage,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, Type)]
pub struct AndroidRuntimeOwnerViewModel {
    pub serial: String,
    pub epoch: Uuid,
    pub mode: AndroidRuntimeOwnerMode,
    pub profile_id: String,
    pub state: AndroidRuntimeOwnerState,
    pub source: AndroidRuntimeOwnerSource,
    pub transition_reason: AndroidRuntimeOwnerTransitionReason,
    pub updated_at: DateTime<Utc>,
}
