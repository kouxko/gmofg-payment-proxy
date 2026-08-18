use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use specta::Type;
use uuid::Uuid;

use super::{AndroidRuntimeOwnerMode, AndroidRuntimeOwnerViewModel};

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize, Type)]
#[serde(rename_all = "snake_case")]
pub enum AndroidRuntimeEndpointHealth {
    Healthy,
    WaitingReconnect,
    Faulted,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, Type)]
pub struct AndroidConfiguredEndpointViewModel {
    pub profile_id: String,
    pub original_destination: String,
    pub original_ports: Vec<u16>,
    pub listener_id: String,
    pub listener_name: String,
    pub listener_bind_address: String,
    pub listener_port: u16,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, Type)]
pub struct AndroidRuntimeEndpointViewModel {
    pub serial: String,
    pub epoch: Uuid,
    pub mode: AndroidRuntimeOwnerMode,
    pub original_destination: String,
    pub original_ports: Vec<u16>,
    pub resolved_original_ips: Vec<String>,
    pub listener_id: String,
    pub listener_name: String,
    pub desktop_listener_port: u16,
    pub proxy_host: String,
    pub proxy_port: u16,
    pub resolved_at: DateTime<Utc>,
    pub health: AndroidRuntimeEndpointHealth,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, Type)]
pub struct AndroidNetworkEndpointSnapshotViewModel {
    pub configured_profile_id: Option<String>,
    pub configured: Vec<AndroidConfiguredEndpointViewModel>,
    pub runtime_owner: Option<AndroidRuntimeOwnerViewModel>,
    pub runtime: Vec<AndroidRuntimeEndpointViewModel>,
}
