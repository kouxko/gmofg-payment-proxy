//! HTTP 监听安全配置与动态服务端证书接口。
//!
//! 动态证书接口同时供 Reverse Listener 使用；当前 Forward Exchange 不支持 CONNECT
//! 或 MITM 隧道，不能把这里的类型误解为 Forward 的旁路能力。

use std::fmt::Debug;
use std::net::SocketAddr;
use std::sync::Arc;
use std::time::Duration;

use http::HeaderValue;
use uuid::Uuid;

use super::config_error;
use super::target::Network;
use crate::Result;
use crate::http::{HttpProtocolCapabilityFactory, PipelinePorts};
use crate::message::MessageLimits;
use crate::supervisor::ChannelId;

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

#[derive(Debug)]
pub(super) struct ForwardPipelineRuntime {
    pub(super) channel: ChannelId,
    pub(super) runtime_epoch: Uuid,
    pub(super) ports: Arc<dyn PipelinePorts>,
    pub(super) capabilities: Arc<dyn HttpProtocolCapabilityFactory>,
    pub(super) limits: MessageLimits,
}

#[derive(Debug, Default)]
pub struct NoAuthentication;

impl ForwardProxyAuthenticator for NoAuthentication {
    fn authorize(&self, _peer: SocketAddr, _presented: Option<&HeaderValue>) -> bool {
        true
    }
}
