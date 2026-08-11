use std::net::{IpAddr, Ipv4Addr};

use intercept_proxy_application::AndroidProxyRouteActivation;

pub(super) fn parse_device_lan_addresses(output: &str) -> Vec<(Ipv4Addr, u8)> {
    output
        .lines()
        .filter_map(|line| {
            let mut fields = line.split_whitespace();
            let _index = fields.next()?;
            let interface = fields.next()?;
            if interface == "lo" || interface.starts_with("tun") {
                return None;
            }
            if fields.next()? != "inet" {
                return None;
            }
            let (address, prefix) = fields.next()?.split_once('/')?;
            let address = address.parse::<Ipv4Addr>().ok()?;
            let prefix = prefix.parse::<u8>().ok()?;
            (prefix <= 32 && !address.is_loopback() && !address.is_link_local())
                .then_some((address, prefix))
        })
        .collect()
}

pub(in crate::adapters::android_adb) fn lan_endpoint_is_eligible(
    host: Ipv4Addr,
    device: Ipv4Addr,
    device_prefix: u8,
    routes: &[AndroidProxyRouteActivation],
) -> bool {
    same_ipv4_network(host, device, device_prefix)
        && routes.iter().all(|route| {
            listener_accepts_lan_host(&route.desktop_listener_bind_address, host)
                && cidrs_allow_device(&route.allowed_client_cidrs, device)
        })
}

fn listener_accepts_lan_host(bind_address: &str, host: Ipv4Addr) -> bool {
    bind_address
        .parse::<IpAddr>()
        .is_ok_and(|address| address.is_unspecified() || address == IpAddr::V4(host))
}

fn cidrs_allow_device(cidrs: &[String], device: Ipv4Addr) -> bool {
    cidrs.is_empty() || cidrs.iter().any(|cidr| ipv4_cidr_contains(cidr, device))
}

fn ipv4_cidr_contains(cidr: &str, candidate: Ipv4Addr) -> bool {
    let Some((network, prefix)) = cidr.split_once('/') else {
        return false;
    };
    let Ok(network) = network.parse::<Ipv4Addr>() else {
        return false;
    };
    let Ok(prefix) = prefix.parse::<u8>() else {
        return false;
    };
    same_ipv4_network(network, candidate, prefix)
}

fn same_ipv4_network(left: Ipv4Addr, right: Ipv4Addr, prefix: u8) -> bool {
    if prefix > 32 {
        return false;
    }
    let mask = u32::MAX.checked_shl(32 - u32::from(prefix)).unwrap_or(0);
    u32::from(left) & mask == u32::from(right) & mask
}
