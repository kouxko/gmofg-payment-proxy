//! 为每个运行 epoch 组装 rustls mTLS 与 Hyper 上游连接服务。
//!
//! 启动时一次性读取设置和证书，构建不可变 TLS 快照，再为全部启用通道准备服务；任一
//! 通道失败会让整个 epoch 启动失败。DNS、TCP、TLS 和 HTTP 握手分别受超时/取消约束，
//! 不能把“连上 TCP”误认为 mTLS 或业务请求成功。

use std::collections::BTreeMap;
use std::fmt;
use std::future::Future;
use std::io;
use std::net::SocketAddr;
use std::sync::Arc;
use std::time::Duration;

use async_trait::async_trait;
use http::Uri;
use zeroize::Zeroizing;

use crate::http::{ConnectionAdmission, ConnectionService, HyperUpstreamConnector, PipelinePorts};
use crate::supervisor::{ChannelId, ProxyConfig, RuntimeServiceFactory};
use crate::tls::{ClientTlsAdapter, ServerTlsAdapter};
use crate::transport::{Clock, HandshakePolicy};
use crate::{ErrorCode, ProxyError, Result};

/// Decrypted, validated materials for one runtime epoch. Debug output is
/// intentionally redacted so private material cannot enter logs.
pub struct TlsMaterialSnapshot {
    pub server_certificate_chain_der: Vec<Vec<u8>>,
    pub server_private_key_pkcs8_der: Zeroizing<Vec<u8>>,
    pub app_client_ca_der: Vec<u8>,
    pub allowed_app_client_fingerprint: Option<Vec<u8>>,
    pub upstream_client_certificate_chain_der: Vec<Vec<u8>>,
    pub upstream_client_private_key_pkcs8_der: Zeroizing<Vec<u8>>,
    pub upstream_ca_der: Vec<u8>,
}

impl fmt::Debug for TlsMaterialSnapshot {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("TlsMaterialSnapshot")
            .field(
                "has_app_fingerprint_pin",
                &self.allowed_app_client_fingerprint.is_some(),
            )
            .field("server_private_key", &"<redacted>")
            .field("upstream_client_private_key", &"<redacted>")
            .finish_non_exhaustive()
    }
}

/// Infrastructure implements this trait using DPAPI-backed certificate
/// storage. One call must return a self-consistent immutable snapshot.
#[async_trait]
pub trait TlsMaterialProvider: fmt::Debug + Send + Sync {
    async fn load_epoch_snapshot(&self, leaf_sans: &[String]) -> Result<TlsMaterialSnapshot>;
}

/// Production factory used by the application composition root. It never
/// constructs plaintext or no-op transports.
#[derive(Debug)]
pub struct RustlsRuntimeServiceFactory {
    materials: Arc<dyn TlsMaterialProvider>,
    ports: Arc<dyn PipelinePorts>,
    clock: Arc<dyn Clock>,
}

impl RustlsRuntimeServiceFactory {
    pub fn new(
        materials: Arc<dyn TlsMaterialProvider>,
        ports: Arc<dyn PipelinePorts>,
        clock: Arc<dyn Clock>,
    ) -> Self {
        Self {
            materials,
            ports,
            clock,
        }
    }
}

#[async_trait]
impl RuntimeServiceFactory for RustlsRuntimeServiceFactory {
    async fn build(&self, config: &ProxyConfig) -> Result<BTreeMap<ChannelId, ConnectionService>> {
        let materials = self
            .materials
            .load_epoch_snapshot(&config.leaf_sans)
            .await?;
        let handshake_policy: Arc<dyn HandshakePolicy> = self.ports.clone();
        let acceptor = Arc::new(ServerTlsAdapter::build(
            materials.server_certificate_chain_der,
            materials.server_private_key_pkcs8_der.to_vec(),
            materials.app_client_ca_der,
            materials.allowed_app_client_fingerprint.clone(),
            handshake_policy,
        )?);
        let client_tls = ClientTlsAdapter::build(
            materials.upstream_client_certificate_chain_der,
            materials.upstream_client_private_key_pkcs8_der.to_vec(),
            materials.upstream_ca_der,
        )?;
        let admission = ConnectionAdmission::new(config.max_connections)?;

        let mut services = BTreeMap::new();
        for channel in config.channels.iter().filter(|channel| channel.enabled) {
            let endpoint =
                HttpsEndpoint::parse(&channel.upstream_url, config.connect_timeout).await?;
            let connector = HyperUpstreamConnector {
                address: endpoint.address,
                host: endpoint.server_name,
                host_header: endpoint.host_header,
                rewrite_host: config.rewrite_host,
                tls: Some(client_tls.clone()),
                connect_timeout: config.connect_timeout,
                write_timeout: config.write_timeout,
                read_timeout: config.read_timeout,
                limits: config.limits,
            };
            services.insert(
                channel.channel.clone(),
                ConnectionService {
                    acceptor: acceptor.clone(),
                    upstream: Arc::new(connector),
                    ports: Arc::clone(&self.ports),
                    clock: Arc::clone(&self.clock),
                    admission: admission.clone(),
                    allowed_client_cidrs: Vec::new(),
                    limits: config.limits,
                    read_timeout: config.read_timeout,
                    write_timeout: config.write_timeout,
                },
            );
        }
        Ok(services)
    }
}

#[derive(Debug)]
struct HttpsEndpoint {
    address: SocketAddr,
    server_name: String,
    host_header: String,
}

impl HttpsEndpoint {
    async fn parse(value: &str, connect_timeout: Duration) -> Result<Self> {
        let uri = value.parse::<Uri>().map_err(|error| {
            ProxyError::new(
                ErrorCode::ConfigInvalid,
                format!("invalid upstream URL: {error}"),
            )
        })?;
        if uri.scheme_str() != Some("https") {
            return Err(ProxyError::new(
                ErrorCode::ConfigInvalid,
                "production upstream URL must use https",
            ));
        }
        let has_userinfo = uri
            .authority()
            .is_some_and(|authority| authority.as_str().contains('@'));
        let has_non_empty_request_target = uri
            .path_and_query()
            .is_some_and(|path_and_query| path_and_query.as_str() != "/");
        // `http::Uri` does not retain a fragment, so inspect the original
        // configuration string before accepting it as an origin.
        let has_fragment = value.contains('#');
        if has_userinfo || has_non_empty_request_target || has_fragment {
            return Err(ProxyError::new(
                ErrorCode::ConfigInvalid,
                "production upstream URL must be an HTTPS origin without path, query, fragment, or userinfo",
            ));
        }
        let uri_host = uri.host().ok_or_else(|| {
            ProxyError::new(ErrorCode::ConfigInvalid, "upstream URL host is missing")
        })?;
        let host = uri_host
            .strip_prefix('[')
            .and_then(|value| value.strip_suffix(']'))
            .unwrap_or(uri_host);
        let port = uri.port_u16().unwrap_or(443);
        let address = resolve_address(host, connect_timeout, async {
            tokio::net::lookup_host((host, port))
                .await
                .map(Iterator::collect)
        })
        .await?;
        let authority_host = if host.contains(':') {
            format!("[{host}]")
        } else {
            host.to_owned()
        };
        let host_header = if port == 443 {
            authority_host
        } else {
            format!("{authority_host}:{port}")
        };
        Ok(Self {
            address,
            server_name: host.to_owned(),
            host_header,
        })
    }
}

async fn resolve_address<F>(host: &str, connect_timeout: Duration, lookup: F) -> Result<SocketAddr>
where
    F: Future<Output = io::Result<Vec<SocketAddr>>>,
{
    tokio::time::timeout(connect_timeout, lookup)
        .await
        .map_err(|_| {
            ProxyError::new(
                ErrorCode::UpstreamConnectTimeout,
                format!(
                    "upstream DNS resolution timed out after {} ms",
                    connect_timeout.as_millis()
                ),
            )
        })?
        .map_err(|error| {
            ProxyError::new(
                ErrorCode::ConfigInvalid,
                format!("cannot resolve upstream host {host}: {error}"),
            )
        })?
        .into_iter()
        .next()
        .ok_or_else(|| {
            ProxyError::new(
                ErrorCode::ConfigInvalid,
                format!("upstream host {host} resolved to no addresses"),
            )
        })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn production_endpoint_rejects_plain_http() {
        let error =
            HttpsEndpoint::parse("http://localhost:8080/", std::time::Duration::from_secs(1))
                .await
                .unwrap_err();
        assert_eq!(error.code, "CONFIG_INVALID");
    }

    #[tokio::test]
    async fn endpoint_preserves_non_default_port_in_host_header() {
        let endpoint =
            HttpsEndpoint::parse("https://127.0.0.1:18443", std::time::Duration::from_secs(1))
                .await
                .unwrap();
        assert_eq!(endpoint.server_name, "127.0.0.1");
        assert_eq!(endpoint.host_header, "127.0.0.1:18443");
        assert_eq!(endpoint.address.port(), 18_443);
    }

    #[tokio::test]
    async fn endpoint_brackets_ipv6_host_header() {
        let endpoint =
            HttpsEndpoint::parse("https://[::1]:18443/", std::time::Duration::from_secs(1))
                .await
                .unwrap();
        assert_eq!(endpoint.server_name, "::1");
        assert_eq!(endpoint.host_header, "[::1]:18443");
        assert_eq!(endpoint.address.port(), 18_443);
    }

    #[tokio::test]
    async fn production_endpoint_rejects_non_origin_urls_before_dns_resolution() {
        for invalid in [
            "https://127.0.0.1:18443/api",
            "https://127.0.0.1:18443/?mode=test",
            "https://127.0.0.1:18443?mode=test",
            "https://127.0.0.1:18443/#fragment",
            "https://user:secret@127.0.0.1:18443",
        ] {
            let result = HttpsEndpoint::parse(invalid, Duration::from_secs(1)).await;
            assert!(
                result.is_err(),
                "runtime must reject upstream URL {invalid}, got {result:?}"
            );
            let error = result.unwrap_err();
            assert_eq!(error.code, ErrorCode::ConfigInvalid.as_str(), "{invalid}");
        }
    }

    #[tokio::test]
    async fn dns_resolution_uses_the_configured_connect_timeout() {
        let error = resolve_address(
            "slow.test",
            Duration::from_millis(5),
            std::future::pending(),
        )
        .await
        .expect_err("a stalled resolver must respect the configured connect timeout");

        assert_eq!(error.code, ErrorCode::UpstreamConnectTimeout.as_str());
        assert!(error.message.contains("5 ms"));
    }

    #[test]
    fn default_limits_remain_expected_for_factory_callers() {
        assert_eq!(
            crate::message::MessageLimits::default().max_body_bytes,
            4 * 1024 * 1024
        );
    }

    #[test]
    fn tls_snapshot_debug_never_contains_secret_buffers() {
        let snapshot = TlsMaterialSnapshot {
            server_certificate_chain_der: vec![b"server-cert".to_vec()],
            server_private_key_pkcs8_der: Zeroizing::new(b"secret-server-key".to_vec()),
            app_client_ca_der: b"app-ca".to_vec(),
            allowed_app_client_fingerprint: None,
            upstream_client_certificate_chain_der: vec![b"client-cert".to_vec()],
            upstream_client_private_key_pkcs8_der: Zeroizing::new(b"secret-client-key".to_vec()),
            upstream_ca_der: b"upstream-ca".to_vec(),
        };
        let debug = format!("{snapshot:?}");
        assert!(!debug.contains("secret-server-key"));
        assert!(!debug.contains("secret-client-key"));
        assert!(debug.contains("<redacted>"));
    }
}
