//! 独立于 Workspace 聚合结构的网络值校验。

use std::net::IpAddr;

#[must_use]
pub fn is_valid_cidr(value: &str) -> bool {
    let Some((address, prefix)) = value.split_once('/') else {
        return false;
    };
    let Ok(address) = address.parse::<IpAddr>() else {
        return false;
    };
    let Ok(prefix) = prefix.parse::<u8>() else {
        return false;
    };
    prefix <= if address.is_ipv4() { 32 } else { 128 }
}

#[must_use]
pub fn is_valid_upstream_origin(value: &str) -> bool {
    let rest = value
        .strip_prefix("http://")
        .or_else(|| value.strip_prefix("https://"));
    let Some(rest) = rest else { return false };
    if rest.is_empty() || rest.chars().any(char::is_whitespace) {
        return false;
    }
    let authority_end = rest.find(['/', '?', '#']).unwrap_or(rest.len());
    let authority = &rest[..authority_end];
    if authority.is_empty()
        || authority.contains('@')
        || !matches!(&rest[authority_end..], "" | "/")
    {
        return false;
    }
    valid_authority(authority)
}

#[must_use]
pub fn is_valid_socket_host(value: &str) -> bool {
    value == value.trim() && valid_host(value)
}

pub(super) fn is_valid_authority_pattern(value: &str) -> bool {
    let value = value.strip_prefix("*.").unwrap_or(value);
    !value.is_empty() && valid_host(value)
}

pub(super) fn is_valid_dns_authority_pattern(value: &str) -> bool {
    let value = value.strip_prefix("*.").unwrap_or(value);
    !value.is_empty() && value.parse::<IpAddr>().is_err() && valid_host(value)
}

fn valid_authority(authority: &str) -> bool {
    if let Some(bracketed) = authority.strip_prefix('[') {
        let Some(end) = bracketed.find(']') else {
            return false;
        };
        let suffix = &bracketed[end + 1..];
        return bracketed[..end].parse::<std::net::Ipv6Addr>().is_ok()
            && (suffix.is_empty() || suffix.strip_prefix(':').is_some_and(valid_port));
    }
    if let Some((host, port)) = authority.rsplit_once(':') {
        return !host.contains(':') && valid_host(host) && valid_port(port);
    }
    valid_host(authority)
}

fn valid_port(value: &str) -> bool {
    value.parse::<u16>().is_ok_and(|port| port > 0)
}

fn valid_host(value: &str) -> bool {
    value.parse::<IpAddr>().is_ok()
        || (!value.is_empty()
            && value.len() <= 253
            && value.split('.').all(|label| {
                !label.is_empty()
                    && label.len() <= 63
                    && !label.starts_with('-')
                    && !label.ends_with('-')
                    && label
                        .bytes()
                        .all(|byte| byte.is_ascii_alphanumeric() || byte == b'-')
            }))
}
