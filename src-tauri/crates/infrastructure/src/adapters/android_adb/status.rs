use std::{collections::HashMap, path::Path};

use intercept_proxy_application::{
    ANDROID_COMPANION_PACKAGE, AndroidAdbViewModel, AndroidCompanionInstallViewModel,
    AndroidControlTransport, AndroidNetworkState, AndroidNetworkStatusViewModel,
    AndroidPackageViewModel, UiTone,
};

use super::protocol::fallback_unsupported_fields;

pub(super) fn adb_view_model(
    adb_path: Option<&Path>,
    version: Option<String>,
    selected_serial: Option<String>,
) -> AndroidAdbViewModel {
    AndroidAdbViewModel {
        available: adb_path.is_some(),
        executable: adb_path.map(|path| path.display().to_string()),
        version,
        selected_serial,
    }
}

pub(super) fn normalize_packages(
    mut packages: Vec<AndroidPackageViewModel>,
) -> Vec<AndroidPackageViewModel> {
    let counts = packages
        .iter()
        .fold(HashMap::<u32, usize>::new(), |mut counts, package| {
            *counts.entry(package.uid).or_default() += 1;
            counts
        });
    for package in &mut packages {
        package.shared_uid =
            (counts.get(&package.uid).copied().unwrap_or_default() > 1).then_some(package.uid);
    }
    packages.sort_by(|left, right| left.package_name.cmp(&right.package_name));
    packages
}

pub(super) fn companion_install_view_model(
    serial: String,
    version_name: Option<String>,
    version_code: Option<String>,
) -> AndroidCompanionInstallViewModel {
    AndroidCompanionInstallViewModel {
        serial,
        package_name: ANDROID_COMPANION_PACKAGE.into(),
        installed: true,
        version_name,
        version_code,
    }
}

pub(super) fn consent_opened_status(serial: String) -> AndroidNetworkStatusViewModel {
    AndroidNetworkStatusViewModel {
        serial,
        state: AndroidNetworkState::Unknown,
        state_text: "状态未知".into(),
        ui_tone: UiTone::Warning,
        verified: false,
        transport: AndroidControlTransport::RescueActivity,
        active_profile_id: None,
        active_profile_fingerprint: None,
        active_route_fingerprint: None,
        active_route_count: 0,
        companion_process_running: None,
        message: "已打开 Android 系统 VPN consent 页面；用户授权结果仅能在设备上确认。".into(),
        unsupported_fields: vec!["vpn_consent_granted".into()],
        stats: None,
    }
}

pub(super) fn control_unavailable_status(
    serial: String,
    running: bool,
) -> AndroidNetworkStatusViewModel {
    AndroidNetworkStatusViewModel {
        serial,
        state: AndroidNetworkState::Unknown,
        state_text: "状态未知".into(),
        ui_tone: UiTone::Warning,
        verified: false,
        transport: AndroidControlTransport::Unavailable,
        active_profile_id: None,
        active_profile_fingerprint: None,
        active_route_fingerprint: None,
        active_route_count: 0,
        companion_process_running: Some(running),
        message: "设备端组件未提供控制通道；仅凭进程是否存在无法证明网络接管或弱网数据面状态。"
            .into(),
        unsupported_fields: fallback_unsupported_fields(),
        stats: None,
    }
}
