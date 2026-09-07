use std::collections::{BTreeMap, BTreeSet};
use std::net::IpAddr;

use intercept_proxy_domain::{normalize_android_ip_cidr, normalize_android_network_destination};
use thiserror::Error;

use crate::{
    COMPANION_PACKAGE_NAME, InstalledApplication, MAX_TARGET_APPLICATIONS, NetworkProfile,
    TargetApplication,
};

/// Profile 在建立 TUN 前必须返回给 UI 的稳定校验错误。
#[derive(Clone, Debug, Eq, Error, PartialEq)]
pub enum ProfileValidationError {
    #[error("至少需要选择一个目标应用")]
    EmptyTargetApplications,
    #[error("一个 Profile 最多只能选择 {maximum} 个应用，当前为 {actual} 个")]
    TooManyTargetApplications { maximum: usize, actual: usize },
    #[error("Android Companion 自身不能加入 VPN 允许列表")]
    CompanionSelected,
    #[error("包名格式无效：{0}")]
    InvalidPackageName(String),
    #[error("目标应用重复：{0}")]
    DuplicatePackage(String),
    #[error("目标应用未安装：{0}")]
    PackageNotInstalled(String),
    #[error("应用 UID 已变化：{package_name}，保存值 {saved_uid}，当前值 {actual_uid}")]
    UidChanged {
        package_name: String,
        saved_uid: u32,
        actual_uid: u32,
    },
    #[error("一个 Profile 最多配置 {maximum} 个目标地址范围，当前为 {actual} 个")]
    TooManyDestinationTargets { maximum: usize, actual: usize },
    #[error("目标地址必须是单个 IP 或合法 IPv4/IPv6 CIDR：{0}")]
    InvalidDestinationCidr(String),
    #[error("目标地址端口必须位于 1..=65535 且不能重复：{cidr}")]
    InvalidDestinationPorts { cidr: String },
    #[error("目标地址范围与端口组合重复：{0}")]
    DuplicateDestinationTarget(String),
    #[error("一个 Profile 最多配置 {maximum} 条透明代理映射，当前为 {actual} 条")]
    TooManyProxyRoutes { maximum: usize, actual: usize },
    #[error("透明代理映射的 Listener ID 无效")]
    InvalidProxyListenerId,
    #[error("透明代理原始 host/IP/CIDR 无效：{0}")]
    InvalidProxyOriginalHost(String),
    #[error("透明代理映射必须配置不重复的原始端口：{0}")]
    InvalidProxyOriginalPorts(String),
    #[error("透明代理映射重复：{0}")]
    DuplicateProxyRoute(String),
    #[error("shared UID {uid} 必须整体选择，缺少：{missing_packages:?}")]
    PartialSharedUidSelection {
        uid: u32,
        missing_packages: Vec<String>,
    },
    #[error("shared UID {uid} 需要用户显式确认，应用组：{packages:?}")]
    SharedUidConfirmationRequired { uid: u32, packages: Vec<String> },
    #[error("概率必须位于 0..=10000 基点：{field}={value}")]
    InvalidProbability { field: &'static str, value: u16 },
    #[error("速率限制必须大于 0：{0}")]
    ZeroRateLimit(&'static str),
    #[error("第 N 个 TCP 标志包中的 N 必须大于 0")]
    ZeroNthTcpFlag,
    #[error("MTU 不能小于 576，当前为 {0}")]
    MtuTooSmall(u16),
    #[error("MSS Clamp {mss} 必须小于 MTU {mtu}")]
    MssNotBelowMtu { mss: u16, mtu: u16 },
    #[error("每个包的位翻转次数不能超过 64，当前为 {0}")]
    TooManyCorruptedBits(u8),
}

/// 已通过启动前检查的 Profile。
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ValidatedProfile(NetworkProfile);

impl ValidatedProfile {
    #[must_use]
    pub fn as_profile(&self) -> &NetworkProfile {
        &self.0
    }

    #[must_use]
    pub fn into_profile(self) -> NetworkProfile {
        self.0
    }
}

impl NetworkProfile {
    /// 根据 Android 当前安装清单进行 fail-closed 的启动前校验。
    pub fn validate_for_start(
        &self,
        installed: &[InstalledApplication],
    ) -> Result<ValidatedProfile, ProfileValidationError> {
        validate_targets(self, installed)?;
        validate_destinations(self)?;
        validate_proxy_routes(self)?;
        validate_faults(self)?;
        Ok(ValidatedProfile(self.clone()))
    }
}

fn validate_proxy_routes(profile: &NetworkProfile) -> Result<(), ProfileValidationError> {
    const MAXIMUM: usize = 128;
    if profile.proxy_routes.is_empty() {
        return Ok(());
    }
    if profile.proxy_routes.len() > MAXIMUM {
        return Err(ProfileValidationError::TooManyProxyRoutes {
            maximum: MAXIMUM,
            actual: profile.proxy_routes.len(),
        });
    }
    let mut seen = BTreeSet::new();
    for route in &profile.proxy_routes {
        if route.listener_id.trim().is_empty() || route.listener_id.len() > 128 {
            return Err(ProfileValidationError::InvalidProxyListenerId);
        }
        let Some(destination) = normalize_android_network_destination(&route.destination) else {
            return Err(ProfileValidationError::InvalidProxyOriginalHost(
                route.destination.clone(),
            ));
        };
        let mut ports = BTreeSet::new();
        if route.ports.is_empty()
            || route
                .ports
                .iter()
                .any(|port| *port == 0 || !ports.insert(*port))
        {
            return Err(ProfileValidationError::InvalidProxyOriginalPorts(
                route.destination.clone(),
            ));
        }
        for port in ports {
            let key = (destination.clone(), port);
            if !seen.insert(key) {
                return Err(ProfileValidationError::DuplicateProxyRoute(
                    route.destination.clone(),
                ));
            }
        }
    }
    Ok(())
}

fn validate_destinations(profile: &NetworkProfile) -> Result<(), ProfileValidationError> {
    const MAXIMUM: usize = 128;
    if profile.destination_targets.len() > MAXIMUM {
        return Err(ProfileValidationError::TooManyDestinationTargets {
            maximum: MAXIMUM,
            actual: profile.destination_targets.len(),
        });
    }
    let mut seen = BTreeSet::new();
    for target in &profile.destination_targets {
        let Some(destination) = normalize_android_ip_cidr(&target.cidr) else {
            return Err(ProfileValidationError::InvalidDestinationCidr(
                target.cidr.clone(),
            ));
        };
        let mut ports = BTreeSet::new();
        if target
            .ports
            .iter()
            .any(|port| *port == 0 || !ports.insert(*port))
        {
            return Err(ProfileValidationError::InvalidDestinationPorts {
                cidr: target.cidr.clone(),
            });
        }
        let key = (destination, ports.into_iter().collect::<Vec<_>>());
        if !seen.insert(key) {
            return Err(ProfileValidationError::DuplicateDestinationTarget(
                target.cidr.clone(),
            ));
        }
    }
    Ok(())
}

pub(crate) fn parse_ip_cidr(value: &str) -> Option<(IpAddr, u8)> {
    let value = value.trim();
    let (address, prefix) = value
        .split_once('/')
        .map_or((value, None), |(address, prefix)| (address, Some(prefix)));
    let address = address.parse::<IpAddr>().ok()?;
    let maximum = if address.is_ipv4() { 32 } else { 128 };
    let prefix = prefix.map_or(Some(maximum), |prefix| prefix.parse::<u8>().ok())?;
    (prefix <= maximum).then_some((address, prefix))
}

fn validate_targets(
    profile: &NetworkProfile,
    installed: &[InstalledApplication],
) -> Result<(), ProfileValidationError> {
    if profile.target_applications.is_empty() {
        return Err(ProfileValidationError::EmptyTargetApplications);
    }
    if profile.target_applications.len() > MAX_TARGET_APPLICATIONS {
        return Err(ProfileValidationError::TooManyTargetApplications {
            maximum: MAX_TARGET_APPLICATIONS,
            actual: profile.target_applications.len(),
        });
    }

    let installed_by_package: BTreeMap<_, _> = installed
        .iter()
        .map(|application| (application.package_name.as_str(), application))
        .collect();
    let mut selected_packages = BTreeSet::new();
    let mut selected_by_uid: BTreeMap<u32, Vec<&TargetApplication>> = BTreeMap::new();

    for target in &profile.target_applications {
        if target.package_name == COMPANION_PACKAGE_NAME {
            return Err(ProfileValidationError::CompanionSelected);
        }
        if !is_valid_package_name(&target.package_name) {
            return Err(ProfileValidationError::InvalidPackageName(
                target.package_name.clone(),
            ));
        }
        if !selected_packages.insert(target.package_name.as_str()) {
            return Err(ProfileValidationError::DuplicatePackage(
                target.package_name.clone(),
            ));
        }

        let Some(actual) = installed_by_package.get(target.package_name.as_str()) else {
            return Err(ProfileValidationError::PackageNotInstalled(
                target.package_name.clone(),
            ));
        };
        if actual.uid != target.uid {
            return Err(ProfileValidationError::UidChanged {
                package_name: target.package_name.clone(),
                saved_uid: target.uid,
                actual_uid: actual.uid,
            });
        }
        selected_by_uid.entry(target.uid).or_default().push(target);
    }

    let installed_by_uid = installed.iter().fold(
        BTreeMap::<u32, Vec<&InstalledApplication>>::new(),
        |mut groups, application| {
            groups.entry(application.uid).or_default().push(application);
            groups
        },
    );

    for (uid, selected_group) in selected_by_uid {
        let installed_group = installed_by_uid.get(&uid).map_or(&[][..], Vec::as_slice);
        if installed_group.len() <= 1 {
            continue;
        }
        let mut missing_packages = installed_group
            .iter()
            .filter(|application| !selected_packages.contains(application.package_name.as_str()))
            .map(|application| application.package_name.clone())
            .collect::<Vec<_>>();
        missing_packages.sort();
        if !missing_packages.is_empty() {
            return Err(ProfileValidationError::PartialSharedUidSelection {
                uid,
                missing_packages,
            });
        }
        if !profile.confirmed_shared_uids.contains(&uid) {
            let mut packages = selected_group
                .iter()
                .map(|application| application.package_name.clone())
                .collect::<Vec<_>>();
            packages.sort();
            return Err(ProfileValidationError::SharedUidConfirmationRequired { uid, packages });
        }
    }

    Ok(())
}

fn validate_faults(profile: &NetworkProfile) -> Result<(), ProfileValidationError> {
    let weak = &profile.weak_network;
    for (field, value) in [
        ("random_loss_basis_points", weak.random_loss_basis_points),
        ("duplicate_basis_points", weak.duplicate_basis_points),
        ("reorder_basis_points", weak.reorder_basis_points),
        (
            "corruption.probability_basis_points",
            weak.corruption.probability_basis_points,
        ),
    ] {
        validate_probability(field, value)?;
    }
    if let Some(burst) = weak.burst_loss {
        for (field, value) in [
            (
                "burst_loss.enter_bad_state_basis_points",
                burst.enter_bad_state_basis_points,
            ),
            (
                "burst_loss.leave_bad_state_basis_points",
                burst.leave_bad_state_basis_points,
            ),
            (
                "burst_loss.good_state_loss_basis_points",
                burst.good_state_loss_basis_points,
            ),
            (
                "burst_loss.bad_state_loss_basis_points",
                burst.bad_state_loss_basis_points,
            ),
        ] {
            validate_probability(field, value)?;
        }
    }
    if weak.upload_bytes_per_second == Some(0) {
        return Err(ProfileValidationError::ZeroRateLimit(
            "upload_bytes_per_second",
        ));
    }
    if weak.download_bytes_per_second == Some(0) {
        return Err(ProfileValidationError::ZeroRateLimit(
            "download_bytes_per_second",
        ));
    }
    if weak.nth_tcp_flag_drops.iter().any(|item| item.nth == 0) {
        return Err(ProfileValidationError::ZeroNthTcpFlag);
    }
    if let Some(mtu) = weak.path_mtu.mtu {
        if mtu < 576 {
            return Err(ProfileValidationError::MtuTooSmall(mtu));
        }
        if let Some(mss) = weak.path_mtu.mss_clamp
            && mss >= mtu
        {
            return Err(ProfileValidationError::MssNotBelowMtu { mss, mtu });
        }
    }
    if weak.corruption.bits_per_packet > 64 {
        return Err(ProfileValidationError::TooManyCorruptedBits(
            weak.corruption.bits_per_packet,
        ));
    }
    Ok(())
}

fn validate_probability(field: &'static str, value: u16) -> Result<(), ProfileValidationError> {
    if value > 10_000 {
        return Err(ProfileValidationError::InvalidProbability { field, value });
    }
    Ok(())
}

fn is_valid_package_name(value: &str) -> bool {
    let parts = value.split('.').collect::<Vec<_>>();
    parts.len() >= 2
        && parts.iter().all(|part| {
            let mut characters = part.chars();
            characters.next().is_some_and(|first| {
                (first.is_ascii_alphabetic() || first == '_')
                    && characters
                        .all(|character| character.is_ascii_alphanumeric() || character == '_')
            })
        })
}

#[cfg(test)]
#[path = "validation/tests.rs"]
mod tests;
