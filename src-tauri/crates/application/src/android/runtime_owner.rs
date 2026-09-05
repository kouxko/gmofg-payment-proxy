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
    Faulted,
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
    LanEndpointReapplied,
    LanEndpointFaulted,
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

/// Read-only owner capacity from one runtime owner snapshot. Admission remains atomic in storage.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, Type)]
pub struct AndroidNetworkControlOverviewViewModel {
    pub owners: Vec<AndroidRuntimeOwnerViewModel>,
    pub max_runtime_owners: u8,
    pub can_start_new_owner: bool,
}

impl AndroidNetworkControlOverviewViewModel {
    pub(crate) fn from_owners(owners: Vec<AndroidRuntimeOwnerViewModel>) -> Self {
        let max_runtime_owners = intercept_proxy_domain::ANDROID_MAX_RUNTIME_OWNERS;
        Self {
            can_start_new_owner: owners.len() < usize::from(max_runtime_owners),
            max_runtime_owners,
            owners,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn retained_owners_count_toward_capacity_even_when_cleanup_is_required() {
        let owner = AndroidRuntimeOwnerViewModel {
            serial: "device".into(),
            epoch: Uuid::nil(),
            mode: AndroidRuntimeOwnerMode::DeviceOnly,
            profile_id: "profile".into(),
            state: AndroidRuntimeOwnerState::CleanupRequired,
            source: AndroidRuntimeOwnerSource::Recovery,
            transition_reason: AndroidRuntimeOwnerTransitionReason::RecoveredFromStorage,
            updated_at: Utc::now(),
        };
        for (count, can_start) in [(0, true), (7, true), (8, false)] {
            let owners = (0..count)
                .map(|index| AndroidRuntimeOwnerViewModel {
                    serial: format!("device-{index}"),
                    ..owner.clone()
                })
                .collect::<Vec<_>>();
            let overview = AndroidNetworkControlOverviewViewModel::from_owners(owners.clone());
            assert_eq!(overview.owners, owners);
            assert_eq!(overview.max_runtime_owners, 8);
            assert_eq!(overview.can_start_new_owner, can_start);
        }
    }
}
