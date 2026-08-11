//! 正向代理的安全配置、认证边界与 MITM 依赖。

use std::collections::{HashMap, VecDeque};
use std::fmt::Debug;
use std::net::{IpAddr, SocketAddr};
use std::sync::Arc;
use std::time::Duration;

use http::HeaderValue;
use rustls::pki_types::ServerName;
use rustls::{ClientConfig, RootCertStore, ServerConfig};
use tokio::net::TcpStream;
use tokio::sync::Mutex;
use tokio_rustls::TlsConnector;
use tokio_util::sync::CancellationToken;
use uuid::Uuid;

use super::target::{Network, valid_authority_pattern};
use super::tunnel::timeout_or_cancel;
use super::{config_error, tls_config_error};
use crate::message::MessageLimits;
use crate::supervisor::ChannelId;
use crate::transport::{BoxIo, PipelinePorts};
use crate::{ErrorCode, ProxyError, Result};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ForwardAuthenticationMode {
    None,
    Required,
}

#[derive(Debug, Clone)]
pub struct ForwardProxyConfig {
    pub bind_addr: SocketAddr,
    pub authentication: ForwardAuthenticationMode,
    pub allowed_client_cidrs: Vec<String>,
    pub connect_timeout: Duration,
    pub read_timeout: Duration,
    pub write_timeout: Duration,
    /// 每次成功读取或写入都会重新开始计时，因此它是真正的空闲超时，不是隧道总寿命。
    pub tunnel_idle_timeout: Duration,
}

impl ForwardProxyConfig {
    /// 在打开监听套接字前执行安全校验。
    pub fn validate(&self) -> Result<()> {
        if self.bind_addr.port() == 0 {
            return Err(config_error(
                "forward proxy listen port must be greater than zero",
            ));
        }
        if self.connect_timeout.is_zero()
            || self.read_timeout.is_zero()
            || self.write_timeout.is_zero()
            || self.tunnel_idle_timeout.is_zero()
        {
            return Err(config_error(
                "forward proxy timeouts must be greater than zero",
            ));
        }
        if !self.bind_addr.ip().is_loopback()
            && (self.authentication != ForwardAuthenticationMode::Required
                || self.allowed_client_cidrs.is_empty())
        {
            return Err(config_error(
                "non-loopback forward proxy listeners require authentication and a client CIDR allowlist",
            ));
        }
        for cidr in &self.allowed_client_cidrs {
            Network::parse(cidr).ok_or_else(|| {
                config_error(format!("invalid forward proxy client CIDR {cidr:?}"))
            })?;
        }
        Ok(())
    }

    pub(super) fn permits_peer(&self, peer: IpAddr) -> bool {
        let peer = match peer {
            IpAddr::V6(address) => address
                .to_ipv4_mapped()
                .map_or(IpAddr::V6(address), IpAddr::V4),
            IpAddr::V4(_) => peer,
        };
        // ADB reverse terminates on the desktop as a loopback connection. It still passes
        // through proxy authentication, while remote clients remain constrained by CIDR.
        peer.is_loopback()
            || self.allowed_client_cidrs.is_empty()
            || self
                .allowed_client_cidrs
                .iter()
                .filter_map(|value| Network::parse(value))
                .any(|network| network.contains(peer))
    }
}

pub trait ForwardProxyAuthenticator: Debug + Send + Sync {
    fn authorize(&self, peer: SocketAddr, presented: Option<&HeaderValue>) -> bool;
}

pub trait MitmCertificateAuthority: Debug + Send + Sync {
    fn issue_server_identity(&self, authority_host: &str) -> Result<MitmServerIdentity>;
}

pub struct MitmServerIdentity {
    pub certificate_chain_der: Vec<Vec<u8>>,
    pub private_key_pkcs8_der: zeroize::Zeroizing<Vec<u8>>,
}

impl Debug for MitmServerIdentity {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("MitmServerIdentity")
            .field("certificate_chain_len", &self.certificate_chain_der.len())
            .field("private_key_pkcs8_der", &"<redacted>")
            .finish()
    }
}

#[async_trait::async_trait]
pub trait MitmUpstreamConnector: Debug + Send + Sync {
    async fn connect(
        &self,
        authority_host: &str,
        upstream: TcpStream,
        cancellation: &CancellationToken,
    ) -> Result<BoxIo>;
}

#[derive(Debug, Clone)]
pub struct NativeRootMitmConnector {
    config: Arc<ClientConfig>,
}

impl NativeRootMitmConnector {
    pub fn new() -> Result<Self> {
        let mut roots = RootCertStore::empty();
        let loaded = rustls_native_certs::load_native_certs();
        let (added, ignored) = roots.add_parsable_certificates(loaded.certs);
        if added == 0 {
            return Err(ProxyError::new(
                ErrorCode::CertificateNotReady,
                format!(
                    "platform trust store contains no usable certificates ({} load errors, {ignored} invalid certificates)",
                    loaded.errors.len()
                ),
            ));
        }
        let config =
            ClientConfig::builder_with_provider(Arc::new(rustls::crypto::ring::default_provider()))
                .with_safe_default_protocol_versions()
                .map_err(tls_config_error)?
                .with_root_certificates(roots)
                .with_no_client_auth();
        Ok(Self {
            config: Arc::new(config),
        })
    }
}

#[async_trait::async_trait]
impl MitmUpstreamConnector for NativeRootMitmConnector {
    async fn connect(
        &self,
        authority_host: &str,
        upstream: TcpStream,
        cancellation: &CancellationToken,
    ) -> Result<BoxIo> {
        let server_name = ServerName::try_from(authority_host.to_owned())
            .map_err(|error| config_error(format!("invalid MITM upstream server name: {error}")))?;
        let stream = timeout_or_cancel(
            Duration::from_secs(30),
            cancellation,
            TlsConnector::from(self.config.clone()).connect(server_name, upstream),
            ErrorCode::UpstreamConnectTimeout,
        )
        .await?
        .map_err(|error| {
            ProxyError::new(
                ErrorCode::TlsHandshakeFailed,
                format!("MITM upstream TLS handshake failed: {error}"),
            )
        })?;
        Ok(Box::new(stream))
    }
}

#[derive(Debug, Clone)]
pub struct ForwardMitmConfig {
    pub authority_allowlist: Vec<String>,
    pub maximum_cached_leaf_certificates: usize,
}

impl ForwardMitmConfig {
    pub(super) fn validate(&self) -> Result<()> {
        if self.authority_allowlist.is_empty() {
            return Err(config_error("MITM authority allowlist must not be empty"));
        }
        if !(1..=256).contains(&self.maximum_cached_leaf_certificates) {
            return Err(config_error(
                "MITM leaf certificate cache capacity must be in 1..=256",
            ));
        }
        if self
            .authority_allowlist
            .iter()
            .any(|pattern| !valid_authority_pattern(pattern))
        {
            return Err(config_error(
                "MITM allowlist entries must be exact hosts/IPs or *.example.test patterns",
            ));
        }
        Ok(())
    }
}

#[derive(Debug)]
pub(super) struct MitmLeafCache {
    pub(super) entries: HashMap<String, Arc<ServerConfig>>,
    recency: VecDeque<String>,
    capacity: usize,
}

impl MitmLeafCache {
    pub(super) fn new(capacity: usize) -> Self {
        Self {
            entries: HashMap::with_capacity(capacity),
            recency: VecDeque::with_capacity(capacity),
            capacity,
        }
    }

    pub(super) fn get(&mut self, host: &str) -> Option<Arc<ServerConfig>> {
        let value = self.entries.get(host).cloned()?;
        self.touch(host);
        Some(value)
    }

    pub(super) fn insert(&mut self, host: &str, value: &Arc<ServerConfig>) {
        self.entries.insert(host.to_owned(), value.clone());
        self.touch(host);
        while self.entries.len() > self.capacity {
            if let Some(evicted) = self.recency.pop_front() {
                self.entries.remove(&evicted);
            }
        }
    }

    fn touch(&mut self, host: &str) {
        self.recency.retain(|entry| entry != host);
        self.recency.push_back(host.to_owned());
    }
}

#[derive(Debug)]
pub(super) struct ForwardMitmRuntime {
    pub(super) config: ForwardMitmConfig,
    pub(super) certificate_authority: Arc<dyn MitmCertificateAuthority>,
    pub(super) upstream_connector: Arc<dyn MitmUpstreamConnector>,
    pub(super) leaf_cache: Mutex<MitmLeafCache>,
}

impl ForwardMitmRuntime {
    pub(super) fn new(
        config: ForwardMitmConfig,
        certificate_authority: Arc<dyn MitmCertificateAuthority>,
        upstream_connector: Arc<dyn MitmUpstreamConnector>,
    ) -> Self {
        let capacity = config.maximum_cached_leaf_certificates;
        Self {
            config,
            certificate_authority,
            upstream_connector,
            leaf_cache: Mutex::new(MitmLeafCache::new(capacity)),
        }
    }
}

#[derive(Debug)]
pub(super) struct ForwardPipelineRuntime {
    pub(super) channel: ChannelId,
    pub(super) runtime_epoch: Uuid,
    pub(super) ports: Arc<dyn PipelinePorts>,
    pub(super) limits: MessageLimits,
}

#[derive(Debug, Default)]
pub struct NoAuthentication;

impl ForwardProxyAuthenticator for NoAuthentication {
    fn authorize(&self, _peer: SocketAddr, _presented: Option<&HeaderValue>) -> bool {
        true
    }
}
