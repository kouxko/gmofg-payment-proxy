//! Android 原始目标到桌面代理入口的运行态透明路由。
//!
//! Workspace 只保存 `destination + ports + listener_id`。桌面应用在启动 Android
//! 数据面时把当前 Listener 和 USB/LAN 链路解析成 [`ProxyRuntimeConfiguration`]；本
//! 模块再次校验这份临时配置。域名的真实解析由桌面端完成并通过
//! `resolved_original_ips` 下发；Android 端不依赖设备物理网络 DNS。

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
        let exact_match = self
            .routes
            .iter()
            .find(|route| {
                route.ports.contains(&port)
                    && (route
                        .network
                        .as_ref()
                        .is_some_and(|network| network.contains(address))
                        || route.resolved_ips.contains(&address))
            })
            .map(|route| route.proxy_addresses.as_slice());
        if exact_match.is_some() {
            return exact_match;
        }

        self.unique_domain_route_for_port(port)
    }

    /// 当 SOCKS5 只提供数值 IP、且该端口只属于一条域名路由时，按端口补偿匹配。
    ///
    /// 应用可能缓存旧 DNS 结果，域名也可能轮询到桌面启动快照之外的新 IP；部分
    /// tun2proxy 路径还会把 Fake-IP 作为 SOCKS5 目标上报。若此时直接回退到原始
    /// Server，就会绕过用户明确配置的透明代理。只有一条域名路由占用该端口时，
    /// 端口足以唯一确定代理入口；同端口存在多条域名路由时保持不匹配，禁止猜测。
    fn unique_domain_route_for_port(&self, port: u16) -> Option<&[std::net::SocketAddr]> {
        let mut candidates = self
            .routes
            .iter()
            .filter(|route| route.network.is_none() && route.ports.contains(&port));
        let route = candidates.next()?;
        if candidates.next().is_some() {
            return None;
        }
        Some(route.proxy_addresses.as_slice())
    }
}

async fn compile_route(route: &ResolvedProxyRoute) -> io::Result<CompiledRoute> {
    let destination = normalize_destination(&route.original_destination)?;
    let network =
        parse_ip_cidr(&destination).map(|(address, prefix)| NetworkMatcher { address, prefix });
    let resolved_ips = route
        .resolved_original_ips
        .iter()
        .copied()
        .collect::<BTreeSet<_>>();
    // 不在 Android 启动阶段再次解析域名：设备可能只有 USB/ADB、没有任何可用
    // 物理网络。Fake-IP DNS 会让新连接以域名进入 SOCKS5，因此 `for_domain` 仍可
    // 精确命中；桌面端快照则兼容应用缓存 DNS 后直接连接真实 IP 的情况。

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
