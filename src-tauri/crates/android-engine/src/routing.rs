//! Android 原始目标到桌面代理入口的运行态透明路由。
//!
//! Workspace 只保存 `destination + ports + listener_id`。桌面应用在启动 Android
//! 数据面时把当前 Listener 和 USB/LAN 链路解析成 [`ProxyRuntimeConfiguration`]；本
//! 模块再次校验这份临时配置，并把域名解析成 TUN 能看到的 IP 集合。

use std::{collections::BTreeSet, io, net::IpAddr};

use intercept_proxy_domain::normalize_android_network_destination;

use crate::{
    ProxyRuntimeConfiguration, ResolvedProxyRoute, ValidatedProfile, validation::parse_ip_cidr,
};

#[derive(Clone, Debug, Eq, PartialEq)]
struct NetworkMatcher {
    address: IpAddr,
    prefix: u8,
}

impl NetworkMatcher {
    fn contains(&self, candidate: IpAddr) -> bool {
        match (self.address, candidate) {
            (IpAddr::V4(network), IpAddr::V4(candidate)) => {
                let mask = u32::MAX
                    .checked_shl(32 - u32::from(self.prefix))
                    .unwrap_or(0);
                u32::from(network) & mask == u32::from(candidate) & mask
            }
            (IpAddr::V6(network), IpAddr::V6(candidate)) => {
                let mask = u128::MAX
                    .checked_shl(128 - u32::from(self.prefix))
                    .unwrap_or(0);
                u128::from(network) & mask == u128::from(candidate) & mask
            }
            _ => false,
        }
    }
}

#[derive(Clone, Debug)]
struct CompiledRoute {
    original_destination: String,
    network: Option<NetworkMatcher>,
    resolved_ips: BTreeSet<IpAddr>,
    ports: BTreeSet<u16>,
    proxy_addresses: Vec<std::net::SocketAddr>,
}

/// Profile 启动时编译的不可变透明路由表。
#[derive(Clone, Debug, Default)]
pub(crate) struct ProxyRouteTable {
    routes: Vec<CompiledRoute>,
}

impl ProxyRouteTable {
    pub(crate) async fn compile(
        profile: &ValidatedProfile,
        runtime: &ProxyRuntimeConfiguration,
    ) -> io::Result<Self> {
        validate_runtime_shape(profile, runtime)?;
        let mut routes = Vec::with_capacity(runtime.routes.len());
        for route in &runtime.routes {
            routes.push(compile_route(route).await?);
        }
        Ok(Self { routes })
    }

    pub(crate) fn for_domain(&self, domain: &str, port: u16) -> Option<&[std::net::SocketAddr]> {
        let normalized = normalize_android_network_destination(domain)?;
        self.routes
            .iter()
            .find(|route| route.ports.contains(&port) && route.original_destination == normalized)
            .map(|route| route.proxy_addresses.as_slice())
    }

    pub(crate) fn for_ip(&self, address: IpAddr, port: u16) -> Option<&[std::net::SocketAddr]> {
        self.routes
            .iter()
            .find(|route| {
                route.ports.contains(&port)
                    && (route
                        .network
                        .as_ref()
                        .is_some_and(|network| network.contains(address))
                        || route.resolved_ips.contains(&address))
            })
            .map(|route| route.proxy_addresses.as_slice())
    }
}

async fn compile_route(route: &ResolvedProxyRoute) -> io::Result<CompiledRoute> {
    let destination = normalize_destination(&route.original_destination)?;
    let network =
        parse_ip_cidr(&destination).map(|(address, prefix)| NetworkMatcher { address, prefix });
    let mut resolved_ips = route
        .resolved_original_ips
        .iter()
        .copied()
        .collect::<BTreeSet<_>>();
    if network.is_none() {
        let device_resolved = tokio::net::lookup_host((destination.as_str(), 0))
            .await
            .map_err(|error| {
                invalid_input(format!(
                    "透明代理原始域名 {destination} 在 Android 启动阶段解析失败：{error}"
                ))
            })?
            .map(|address| address.ip())
            .collect::<BTreeSet<_>>();
        if device_resolved.is_empty() {
            return Err(invalid_input(format!(
                "透明代理原始域名 {destination} 没有 A/AAAA 记录"
            )));
        }
        resolved_ips.extend(device_resolved);
    }

    let proxy_addresses = tokio::net::lookup_host((route.proxy_host.as_str(), route.proxy_port))
        .await
        .map_err(|error| {
            invalid_input(format!(
                "代理监听 {} 的运行态地址 {}:{} 无法解析：{error}",
                route.listener_id, route.proxy_host, route.proxy_port
            ))
        })?
        .collect::<Vec<_>>();
    if proxy_addresses.is_empty() {
        return Err(invalid_input(format!(
            "代理监听 {} 没有可连接的运行态地址",
            route.listener_id
        )));
    }

    Ok(CompiledRoute {
        original_destination: destination,
        network,
        resolved_ips,
        ports: route.original_ports.iter().copied().collect(),
        proxy_addresses,
    })
}

fn validate_runtime_shape(
    profile: &ValidatedProfile,
    runtime: &ProxyRuntimeConfiguration,
) -> io::Result<()> {
    let expected = profile
        .as_profile()
        .proxy_routes
        .iter()
        .map(|route| {
            Ok((
                route.listener_id.as_str(),
                normalize_destination(&route.destination)?,
                normalized_ports(&route.ports),
            ))
        })
        .collect::<io::Result<BTreeSet<_>>>()?;
    let actual = runtime
        .routes
        .iter()
        .map(|route| {
            if route.proxy_host.trim().is_empty() || route.proxy_port == 0 {
                return Err(invalid_input(format!(
                    "代理监听 {} 的运行态地址为空或端口为 0",
                    route.listener_id
                )));
            }
            if route.original_ports.is_empty() || route.original_ports.contains(&0) {
                return Err(invalid_input(format!(
                    "代理监听 {} 的原始端口无效",
                    route.listener_id
                )));
            }
            Ok((
                route.listener_id.as_str(),
                normalize_destination(&route.original_destination)?,
                normalized_ports(&route.original_ports),
            ))
        })
        .collect::<io::Result<BTreeSet<_>>>()?;
    if expected != actual {
        return Err(invalid_input(
            "Android 运行态透明代理路由与当前设备网络方案不一致".to_owned(),
        ));
    }
    Ok(())
}

fn normalize_destination(value: &str) -> io::Result<String> {
    normalize_android_network_destination(value)
        .ok_or_else(|| invalid_input(format!("透明代理原始 host/IP/CIDR 无效：{}", value.trim())))
}

fn normalized_ports(ports: &[u16]) -> Vec<u16> {
    let mut ports = ports.to_vec();
    ports.sort_unstable();
    ports.dedup();
    ports
}

fn invalid_input(message: String) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidInput, message)
}

#[cfg(test)]
#[path = "routing_tests.rs"]
mod tests;
