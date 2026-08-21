//! 外部软件包 WebSocket 服务的 Host 配置装配。

use std::{net::IpAddr, net::SocketAddr, time::Duration};

use intercept_proxy_application::{AppError, SettingsRepositoryPort};
use intercept_proxy_infrastructure::{
    ExternalPackageConnectionConfig, ExternalPackageServer, ExternalPackageServerConfig,
    InfrastructureServiceBundle,
};

use crate::HostBuildError;

pub(super) async fn start_external_package_server(
    services: &InfrastructureServiceBundle,
) -> Result<ExternalPackageServer, HostBuildError> {
    let settings = services.settings.get().await?.stored;
    let max_body_bytes = settings.max_body_bytes;
    let external = settings.external_package_service;
    let ip = external
        .bind_address
        .parse::<IpAddr>()
        .map_err(|_| AppError::new("CONFIG_INVALID", "外部软件包服务监听地址不是有效 IP 地址。"))?;
    // Base64 对二进制帧最多扩张到 4/3；额外预算留给 JSON-RPC 包络和字段名。
    let rpc_message_bytes = usize::try_from(max_body_bytes)
        .unwrap_or(usize::MAX / 2)
        .saturating_mul(4)
        .div_ceil(3)
        .saturating_add(64 * 1024);
    let connection = ExternalPackageConnectionConfig::new(
        Duration::from_secs(30),
        Duration::from_secs(external.rpc_timeout_seconds),
        Duration::from_secs(10),
        Duration::from_secs(30),
        external.max_in_flight,
        usize::try_from(max_body_bytes).unwrap_or(usize::MAX),
        1024 * 1024,
        rpc_message_bytes,
        128 * 1024,
    );
    Ok(ExternalPackageServer::start(
        ExternalPackageServerConfig {
            bind_address: SocketAddr::new(ip, external.port),
            connection,
        },
        services.external_packages.clone(),
        services.protocol_package_usage.clone(),
        services.listener_runtime.clone(),
    )
    .await)
}
