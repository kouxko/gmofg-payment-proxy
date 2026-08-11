//! 反向监听器的客户端网络准入与下游 TLS 接受。

use std::{net::IpAddr, sync::Arc};

use async_trait::async_trait;
use ring::digest::{SHA256, digest};
use tokio_rustls::TlsAcceptor;
use x509_parser::parse_x509_certificate;

use crate::transport::{
    AcceptedConnection, BoxIo, ConnectionAcceptor, ConnectionContext, TlsPeerIdentity,
};
use crate::{ErrorCode, ProxyError, Result};

#[derive(Clone)]
pub(super) struct ReverseConnectionAcceptor {
    pub(super) tls: Option<TlsAcceptor>,
    pub(super) allowed_client_networks: Arc<Vec<ClientNetwork>>,
}

impl std::fmt::Debug for ReverseConnectionAcceptor {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("ReverseConnectionAcceptor")
            .field("tls", &self.tls.is_some())
            .field("allowed_client_networks", &self.allowed_client_networks)
            .finish()
    }
}

#[async_trait]
impl ConnectionAcceptor for ReverseConnectionAcceptor {
    async fn accept(&self, io: BoxIo, context: &ConnectionContext) -> Result<AcceptedConnection> {
        if !peer_is_allowed(
            context.peer_addr.ip(),
            self.allowed_client_networks.as_ref(),
        ) {
            return Err(ProxyError::new(
                ErrorCode::ConfigInvalid,
                format!(
                    "reverse downstream peer {} is not allowed",
                    context.peer_addr.ip()
                ),
            ));
        }
        let Some(acceptor) = &self.tls else {
            return Ok(AcceptedConnection { io, tls_peer: None });
        };
        let stream = acceptor.accept(io).await.map_err(|error| {
            // 下游握手发生在 HTTP Session 创建之前。保留对端地址和 rustls 原始错误，
            // 让桌面诊断页能够区分 SNI、签名算法、协议版本和证书链等失败原因。
            tracing::warn!(
                peer = %context.peer_addr,
                error = %error,
                "reverse downstream TLS handshake failed"
            );
            ProxyError::new(
                ErrorCode::DownstreamTlsHandshakeFailed,
                format!(
                    "客户端到代理的 TLS 握手失败（对端 {}）：{error}",
                    context.peer_addr
                ),
            )
        })?;
        let tls_peer = stream
            .get_ref()
            .1
            .peer_certificates()
            .and_then(|certificates| certificates.first())
            .map(|certificate| peer_identity(certificate.as_ref()))
            .transpose()?;
        Ok(AcceptedConnection {
            io: Box::new(stream),
            tls_peer,
        })
    }
}

/// 判断下游连接是否可以进入 TLS/HTTP 管线。
///
/// Android 设备网络接管通过 `adb reverse` 把设备端临时端口映射到桌面监听端口。
/// 该连接到达桌面进程时，操作系统报告的对端是本机回环地址，而不是 Android 的
/// WLAN 地址。回环地址不能由远程主机直接伪造，因此可作为受控本机传输放行；
/// 配置 CIDR 时，非回环连接必须命中准入列表；列表留空则由用户显式选择允许所有地址。
pub(super) fn peer_is_allowed(peer: IpAddr, allowed_networks: &[ClientNetwork]) -> bool {
    canonical_peer_ip(peer).is_loopback()
        || allowed_networks.is_empty()
        || allowed_networks
            .iter()
            .any(|network| network.contains(peer))
}

/// 将操作系统可能返回的 IPv4-mapped IPv6 地址还原为 IPv4。
///
/// ADB reverse 在不同平台上可能把同一个本机连接报告为 `127.0.0.1`，也可能报告为
/// `::ffff:127.0.0.1`。两者的安全含义相同，必须在回环判断和 IPv4 CIDR 匹配前统一。
fn canonical_peer_ip(peer: IpAddr) -> IpAddr {
    match peer {
        IpAddr::V6(address) => address.to_ipv4_mapped().map_or(peer, IpAddr::V4),
        IpAddr::V4(_) => peer,
    }
}

#[derive(Clone, Copy, Debug)]
pub(super) struct ClientNetwork {
    address: IpAddr,
    prefix: u8,
}

impl ClientNetwork {
    pub(super) fn parse(value: &str) -> Result<Self> {
        let (address, prefix) = value.split_once('/').ok_or_else(|| invalid_cidr(value))?;
        let address = address.parse::<IpAddr>().map_err(|_| invalid_cidr(value))?;
        let prefix = prefix.parse::<u8>().map_err(|_| invalid_cidr(value))?;
        let maximum = if address.is_ipv4() { 32 } else { 128 };
        if prefix > maximum {
            return Err(ProxyError::new(
                ErrorCode::ConfigInvalid,
                format!("invalid client CIDR prefix: {value}"),
            ));
        }
        Ok(Self { address, prefix })
    }

    pub(super) fn contains(self, candidate: IpAddr) -> bool {
        match (self.address, canonical_peer_ip(candidate)) {
            (IpAddr::V4(network), IpAddr::V4(candidate)) => {
                masked(u128::from(u32::from(network)), self.prefix, 32)
                    == masked(u128::from(u32::from(candidate)), self.prefix, 32)
            }
            (IpAddr::V6(network), IpAddr::V6(candidate)) => {
                masked(u128::from(network), self.prefix, 128)
                    == masked(u128::from(candidate), self.prefix, 128)
            }
            _ => false,
        }
    }
}

fn invalid_cidr(value: &str) -> ProxyError {
    ProxyError::new(
        ErrorCode::ConfigInvalid,
        format!("invalid client CIDR: {value}"),
    )
}

fn masked(value: u128, prefix: u8, width: u8) -> u128 {
    if prefix == 0 {
        return 0;
    }
    value & (u128::MAX << (width - prefix))
}

fn peer_identity(certificate_der: &[u8]) -> Result<TlsPeerIdentity> {
    let (_, certificate) = parse_x509_certificate(certificate_der).map_err(super::config_error)?;
    let fingerprint = digest(&SHA256, certificate_der)
        .as_ref()
        .iter()
        .map(|byte| format!("{byte:02X}"))
        .collect::<Vec<_>>()
        .join(":");
    Ok(TlsPeerIdentity {
        sha256_fingerprint: fingerprint,
        subject_summary: certificate.subject().to_string(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn client_network_matches_only_its_address_family_and_prefix() {
        let network = ClientNetwork::parse("10.0.34.0/24").unwrap();
        assert!(network.contains("10.0.34.94".parse().unwrap()));
        assert!(!network.contains("10.0.35.1".parse().unwrap()));
        assert!(!network.contains("::1".parse().unwrap()));
    }

    #[test]
    fn adb_reverse_loopback_is_allowed_without_weakening_remote_cidr() {
        let networks = vec![ClientNetwork::parse("10.0.34.0/23").unwrap()];

        assert!(peer_is_allowed("127.0.0.1".parse().unwrap(), &networks));
        assert!(peer_is_allowed("::1".parse().unwrap(), &networks));
        assert!(peer_is_allowed(
            "::ffff:127.0.0.1".parse().unwrap(),
            &networks
        ));
        assert!(peer_is_allowed("10.0.34.42".parse().unwrap(), &networks));
        assert!(peer_is_allowed(
            "::ffff:10.0.34.42".parse().unwrap(),
            &networks
        ));
        assert!(!peer_is_allowed("10.0.36.42".parse().unwrap(), &networks));
    }

    #[test]
    fn empty_cidr_list_keeps_existing_allow_all_behavior() {
        assert!(peer_is_allowed("203.0.113.10".parse().unwrap(), &[]));
    }
}
