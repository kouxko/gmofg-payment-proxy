use serde::{Deserialize, Serialize};

use crate::{AppError, AppResult};

mod projection;

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub(super) struct AndroidNetworkProfileTemplate {
    id: Option<String>,
    name: String,
    target_applications: Vec<AndroidTargetApplicationTemplate>,
    destination_targets: Vec<AndroidDestinationTargetTemplate>,
    proxy_routes: Vec<AndroidProxyRouteTemplate>,
    confirmed_shared_uids: Vec<u32>,
    auto_resume_after_reboot: bool,
    weak_network: WeakNetworkProfileTemplate,
}

impl AndroidNetworkProfileTemplate {
    pub(super) fn id(&self) -> Option<&str> {
        self.id.as_deref()
    }
    pub(super) fn validate_weak_network(&self) -> bool {
        self.weak_network.is_valid()
    }

    pub(super) fn listener_aliases(&self) -> impl Iterator<Item = &str> {
        self.proxy_routes
            .iter()
            .map(|route| route.listener_alias.as_str())
    }

    pub(super) fn validate_domain_limits(&self) -> AppResult<()> {
        let weak_network_bytes = serde_json::to_vec(&self.weak_network)
            .map_err(|_| dto_limit_error())?
            .len();
        if self.id.as_ref().is_some_and(|id| !valid_id(id, 128))
            || self.name.chars().count() > 80
            || !(1..=64).contains(&self.target_applications.len())
            || self.destination_targets.len() > 128
            || self.proxy_routes.len() > 128
            || weak_network_bytes > 256 * 1024
        {
            return Err(dto_limit_error());
        }
        Ok(())
    }
}

fn valid_id(value: &str, maximum_bytes: usize) -> bool {
    !value.is_empty()
        && value.len() <= maximum_bytes
        && value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'_' | b'-'))
}

fn dto_limit_error() -> AppError {
    AppError::new(
        "DTO_LIMIT_EXCEEDED",
        "environment candidate exceeds its DTO limit",
    )
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
struct AndroidTargetApplicationTemplate {
    package_name: String,
    uid: u32,
    #[serde(deserialize_with = "required_nullable")]
    display_name: Option<String>,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
struct AndroidDestinationTargetTemplate {
    cidr: String,
    ports: Vec<u16>,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
struct AndroidProxyRouteTemplate {
    destination: String,
    ports: Vec<u16>,
    listener_alias: String,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
struct WeakNetworkProfileTemplate {
    seed: u64,
    fixed_delay_millis: u64,
    uniform_jitter_millis: u64,
    #[serde(deserialize_with = "required_nullable")]
    upload_bytes_per_second: Option<u64>,
    #[serde(deserialize_with = "required_nullable")]
    download_bytes_per_second: Option<u64>,
    random_loss_basis_points: u16,
    #[serde(deserialize_with = "required_nullable")]
    burst_loss: Option<BurstLossProfileTemplate>,
    duplicate_basis_points: u16,
    reorder_basis_points: u16,
    maximum_reorder_hold_millis: u64,
    blackout_windows: Vec<BlackoutWindowTemplate>,
    dns_blackhole: bool,
    nth_tcp_flag_drops: Vec<NthTcpFlagDropTemplate>,
    path_mtu: PathMtuProfileTemplate,
    corruption: BitCorruptionProfileTemplate,
}

impl WeakNetworkProfileTemplate {
    fn is_valid(&self) -> bool {
        self.upload_bytes_per_second.is_none_or(|rate| rate > 0)
            && self.download_bytes_per_second.is_none_or(|rate| rate > 0)
            && self.random_loss_basis_points <= 10_000
            && self.duplicate_basis_points <= 10_000
            && self.reorder_basis_points <= 10_000
            && self
                .burst_loss
                .as_ref()
                .is_none_or(BurstLossProfileTemplate::is_valid)
            && self.nth_tcp_flag_drops.iter().all(|drop| drop.nth > 0)
            && self.path_mtu.is_valid()
            && self.corruption.probability_basis_points <= 10_000
            && self.corruption.bits_per_packet <= 64
    }
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
struct BurstLossProfileTemplate {
    #[serde(rename = "enter_bad_state_basis_points")]
    enter_bad_state: u16,
    #[serde(rename = "leave_bad_state_basis_points")]
    leave_bad_state: u16,
    #[serde(rename = "good_state_loss_basis_points")]
    good_state_loss: u16,
    #[serde(rename = "bad_state_loss_basis_points")]
    bad_state_loss: u16,
}

impl BurstLossProfileTemplate {
    fn is_valid(&self) -> bool {
        self.enter_bad_state <= 10_000
            && self.leave_bad_state <= 10_000
            && self.good_state_loss <= 10_000
            && self.bad_state_loss <= 10_000
    }
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
struct BlackoutWindowTemplate {
    start_after_millis: u64,
    duration_millis: u64,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
struct NthTcpFlagDropTemplate {
    direction: PacketDirection,
    flag: TcpFlag,
    nth: u64,
}

#[derive(Clone, Copy, Debug, Deserialize, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
enum PacketDirection {
    Upload,
    Download,
}

#[derive(Clone, Copy, Debug, Deserialize, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
enum TcpFlag {
    Syn,
    SynAck,
    Ack,
    Fin,
    Rst,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
struct PathMtuProfileTemplate {
    #[serde(deserialize_with = "required_nullable")]
    mtu: Option<u16>,
    #[serde(deserialize_with = "required_nullable")]
    mss_clamp: Option<u16>,
    mode: PmtuMode,
}

impl PathMtuProfileTemplate {
    fn is_valid(&self) -> bool {
        self.mtu.is_none_or(|mtu| mtu >= 68)
            && self.mss_clamp.is_none_or(|mss| mss > 0)
            && match (self.mtu, self.mss_clamp) {
                (Some(mtu), Some(mss)) => mss < mtu,
                _ => true,
            }
    }
}

#[derive(Clone, Copy, Debug, Deserialize, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
enum PmtuMode {
    Pass,
    FragmentOrPacketTooBig,
    SignalTooBig,
    Blackhole,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
struct BitCorruptionProfileTemplate {
    probability_basis_points: u16,
    bits_per_packet: u8,
}

fn required_nullable<'de, D, T>(deserializer: D) -> Result<Option<T>, D::Error>
where
    D: serde::Deserializer<'de>,
    T: Deserialize<'de>,
{
    Option::<T>::deserialize(deserializer)
}
