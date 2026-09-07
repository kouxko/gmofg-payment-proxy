use std::{
    collections::{BTreeMap, BTreeSet, hash_map::DefaultHasher},
    fmt::Write as _,
    hash::{Hash, Hasher},
};

use intercept_proxy_application::{AndroidProxyRouteActivation, AppError, AppResult};

pub(super) fn reverse_create_error(
    error: &AppError,
    device_port: u16,
    desktop_port: u16,
) -> AppError {
    AppError::new(
        "ANDROID_ADB_REVERSE_CREATE_FAILED",
        format!(
            "ADB 业务代理映射 tcp:{device_port} → tcp:{desktop_port} 创建失败：{}",
            error.view_model.message
        ),
    )
    .retryable("请保持设备在线，确认 ADB reverse 可用后重新启动设备网络接管。")
}

pub(super) fn reverse_cleanup_error(error: &AppError, device_port: u16) -> AppError {
    AppError::new(
        "ANDROID_ADB_REVERSE_CLEANUP_FAILED",
        format!(
            "ADB 业务代理映射 tcp:{device_port} 清理失败：{}",
            error.view_model.message
        ),
    )
    .retryable("请保持设备在线并再次停止网络接管，或执行紧急恢复网络。")
}

#[cfg(test)]
pub(in crate::adapters::android_adb) fn allocated_reverse_ports(
    routes: &[AndroidProxyRouteActivation],
) -> BTreeMap<String, u16> {
    allocated_reverse_ports_avoiding(routes, std::iter::empty())
}

pub(super) fn allocated_reverse_ports_avoiding(
    routes: &[AndroidProxyRouteActivation],
    reserved_ports: impl IntoIterator<Item = u16>,
) -> BTreeMap<String, u16> {
    let mut listener_ports = BTreeMap::new();
    let mut used_device_ports = reserved_ports.into_iter().collect::<BTreeSet<_>>();
    for route in routes {
        if listener_ports.contains_key(&route.listener_id) {
            continue;
        }
        let mut hasher = DefaultHasher::new();
        route.listener_id.hash(&mut hasher);
        let mut device_port = 40_000 + u16::try_from(hasher.finish() % 20_000).unwrap_or(0);
        while !used_device_ports.insert(device_port) {
            device_port = if device_port == 59_999 {
                40_000
            } else {
                device_port + 1
            };
        }
        listener_ports.insert(route.listener_id.clone(), device_port);
    }
    listener_ports
}

pub(in crate::adapters::android_adb) fn reverse_mapping_present(
    listing: &str,
    device_port: u16,
    desktop_port: u16,
) -> bool {
    let device = format!("tcp:{device_port}");
    let desktop = format!("tcp:{desktop_port}");
    listing.lines().any(|line| {
        let fields = line.split_whitespace().collect::<Vec<_>>();
        fields
            .windows(2)
            .any(|pair| pair[0] == device && pair[1] == desktop)
    })
}

pub(in crate::adapters::android_adb) fn combine_operation_and_cleanup(
    mut operation: AppError,
    cleanup: &AppError,
) -> AppError {
    let _ = write!(
        operation.view_model.message,
        "；同时 adb reverse 清理失败：{}",
        cleanup.view_model.message
    );
    operation.view_model.retryable = true;
    operation.view_model.suggested_action =
        Some("请保持设备在线并再次停止设备网络接管或执行紧急恢复，以重试清理残留映射。".into());
    operation
}

pub(in crate::adapters::android_adb) fn combine_stop_failures(
    mut graceful: AppError,
    force_stop: &AppError,
) -> AppError {
    let _ = write!(
        graceful.view_model.message,
        "；ADB 强制停止也失败：{}",
        force_stop.view_model.message
    );
    graceful.view_model.retryable = true;
    graceful.view_model.suggested_action =
        Some("请保持 USB/ADB 连接后执行紧急恢复；必要时在 Android VPN 设置中手动停止接管。".into());
    graceful
}

pub(super) fn reconcile_operation_cleanup<T>(
    operation: AppResult<T>,
    cleanup: AppResult<()>,
) -> AppResult<T> {
    match (operation, cleanup) {
        (Ok(value), Ok(())) => Ok(value),
        (Ok(_), Err(cleanup_error)) => Err(cleanup_error),
        (Err(operation_error), Ok(())) => Err(operation_error),
        (Err(operation_error), Err(cleanup_error)) => Err(combine_operation_and_cleanup(
            operation_error,
            &cleanup_error,
        )),
    }
}
