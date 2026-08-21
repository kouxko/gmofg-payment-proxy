use std::{collections::HashSet, fmt, net::IpAddr, pin::Pin, sync::Arc};

use async_trait::async_trait;
use openssl::{
    nid::Nid,
    pkey::PKey,
    ssl::{SslConnector, SslMethod, SslVerifyMode},
    x509::{X509, X509VerifyResult, store::X509StoreBuilder},
};
use tokio_openssl::SslStream;

use crate::tls::peer_identity;
use crate::transport::BoxIo;
use crate::{ErrorCode, ProxyError, Result};

use super::{SocketTlsEvidence, SocketUpstreamTlsConfig};

pub(super) struct SocketUpstreamTlsConnection {
    pub(super) io: BoxIo,
    pub(super) evidence: SocketTlsEvidence,
    pub(super) server_name_candidates: Vec<String>,
}

impl fmt::Debug for SocketUpstreamTlsConnection {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("SocketUpstreamTlsConnection")
            .field("io", &"<TLS stream>")
            .field("evidence", &self.evidence)
            .finish()
    }
}

/// Socket 上游 TLS 的 crate 内部替换边界。
///
/// 领域配置、Workspace、IPC 与 UI 只描述 TLS 语义，不知道具体加密后端。后端必须返回
/// 同一种异步字节流和公开握手证据，因此更换实现不会穿透到 Relay、协议包或抓包管线。
#[async_trait]
pub(super) trait SocketUpstreamTlsConnector: fmt::Debug + Send + Sync {
    async fn connect(&self, domain: &str, io: BoxIo) -> Result<SocketUpstreamTlsConnection>;
    fn requires_server_name_discovery(&self, endpoint_host: &str) -> bool;
    async fn discover_server_names(
        &self,
        endpoint_host: &str,
        io: BoxIo,
    ) -> Result<SocketUpstreamTlsConnection>;
}

pub(super) fn build_socket_upstream_tls_connector(
    config: &SocketUpstreamTlsConfig,
) -> Result<Arc<dyn SocketUpstreamTlsConnector>> {
    Ok(Arc::new(OpenSslSocketUpstreamTlsConnector::build(config)?))
}

#[derive(Clone)]
struct OpenSslSocketUpstreamTlsConnector {
    connector: SslConnector,
    hostname_verification_enabled: bool,
    client_identity_configured: bool,
    tls_server_name: Option<String>,
}

impl fmt::Debug for OpenSslSocketUpstreamTlsConnector {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("SocketUpstreamTlsConnector")
            .field(
                "hostname_verification_enabled",
                &self.hostname_verification_enabled,
            )
            .field(
                "client_identity_configured",
                &self.client_identity_configured,
            )
            .finish_non_exhaustive()
    }
}

impl OpenSslSocketUpstreamTlsConnector {
    fn build(config: &SocketUpstreamTlsConfig) -> Result<Self> {
        let mut builder = SslConnector::builder(SslMethod::tls_client()).map_err(config_error)?;
        // 保留 OpenSSL Connector 的通用安全默认值，由握手自动协商所有当前启用的 TLS
        // 版本和密码套件。这里不为某一个旧服务端建立产品级白名单，也不重新开启 SSLv2/3、
        // 匿名、空加密、RC4、DES/3DES 等已被默认排除的能力。
        builder.set_verify(SslVerifyMode::PEER);
        builder.set_cert_store(trust_store(&config.server_trust_der)?);
        if let Some(identity) = &config.client_identity {
            let (leaf, chain) = identity
                .certificate_chain_der
                .split_first()
                .ok_or_else(|| {
                    ProxyError::new(
                        ErrorCode::ConfigInvalid,
                        "socket upstream client certificate chain is empty",
                    )
                })?;
            let leaf_certificate = X509::from_der(leaf).map_err(config_error)?;
            builder
                .set_certificate(&leaf_certificate)
                .map_err(config_error)?;
            for certificate in chain {
                builder
                    .add_extra_chain_cert(X509::from_der(certificate).map_err(config_error)?)
                    .map_err(config_error)?;
            }
            let private_key = PKey::private_key_from_pkcs8(&identity.private_key_pkcs8_der)
                .map_err(config_error)?;
            builder
                .set_private_key(&private_key)
                .map_err(config_error)?;
            builder.check_private_key().map_err(config_error)?;
        }
        Ok(Self {
            connector: builder.build(),
            hostname_verification_enabled: config.verify_hostname,
            client_identity_configured: config.client_identity.is_some(),
            tls_server_name: validate_tls_server_name(config.tls_server_name.as_deref())?,
        })
    }

    async fn connect_with_verification(
        &self,
        server_name: &str,
        verify_hostname: bool,
        io: BoxIo,
    ) -> Result<SocketUpstreamTlsConnection> {
        let mut configuration = self.connector.configure().map_err(config_error)?;
        configuration.set_verify_hostname(verify_hostname);
        let ssl = configuration.into_ssl(server_name).map_err(config_error)?;
        let mut stream = SslStream::new(ssl, io).map_err(config_error)?;
        if Pin::new(&mut stream).connect().await.is_err() {
            let verification = stream.ssl().verify_result();
            if verification != X509VerifyResult::OK {
                return Err(ProxyError::new(
                    ErrorCode::CertificateInvalid,
                    format!(
                        "socket upstream certificate verification failed: {verification}; check Server CA and TLS Server Name",
                    ),
                ));
            }
            return Err(ProxyError::new(
                ErrorCode::TlsHandshakeFailed,
                "socket upstream TLS handshake failed",
            ));
        }
        let ssl = stream.ssl();
        let certificate = ssl.peer_certificate().ok_or_else(|| {
            ProxyError::new(
                ErrorCode::CertificateInvalid,
                "socket upstream TLS handshake returned no peer certificate",
            )
        })?;
        let server_name_candidates = certificate_server_names(&certificate);
        let peer = peer_identity(&certificate.to_der().map_err(handshake_error)?)?;
        let evidence = SocketTlsEvidence {
            tls_version: normalize_tls_version(ssl.version_str()),
            cipher_suite: ssl
                .current_cipher()
                .map_or_else(|| "未知".to_owned(), |cipher| cipher.name().to_owned()),
            peer_subject: peer.subject_summary,
            peer_sha256_fingerprint: peer.sha256_fingerprint,
            hostname_verification_enabled: verify_hostname,
            client_identity_configured: self.client_identity_configured,
        };
        Ok(SocketUpstreamTlsConnection {
            io: Box::new(stream),
            evidence,
            server_name_candidates,
        })
    }
}

#[async_trait]
impl SocketUpstreamTlsConnector for OpenSslSocketUpstreamTlsConnector {
    async fn connect(&self, domain: &str, io: BoxIo) -> Result<SocketUpstreamTlsConnection> {
        let tls_server_name = self.tls_server_name.as_deref().unwrap_or(domain);
        self.connect_with_verification(tls_server_name, self.hostname_verification_enabled, io)
            .await
    }

    fn requires_server_name_discovery(&self, endpoint_host: &str) -> bool {
        self.hostname_verification_enabled
            && self.tls_server_name.is_none()
            && endpoint_host.parse::<IpAddr>().is_ok()
    }

    async fn discover_server_names(
        &self,
        endpoint_host: &str,
        io: BoxIo,
    ) -> Result<SocketUpstreamTlsConnection> {
        self.connect_with_verification(endpoint_host, false, io)
            .await
    }
}

fn certificate_server_names(certificate: &X509) -> Vec<String> {
    let mut names = certificate
        .subject_alt_names()
        .into_iter()
        .flatten()
        .filter_map(|name| name.dnsname().map(str::to_owned))
        .filter(|name| !name.starts_with("*.") && valid_dns_name(name))
        .collect::<Vec<_>>();
    if names.is_empty() {
        names.extend(
            certificate
                .subject_name()
                .entries_by_nid(Nid::COMMONNAME)
                .filter_map(|entry| entry.data().to_string().ok())
                .filter(|name| !name.starts_with("*.") && valid_dns_name(name)),
        );
    }
    let mut seen = HashSet::new();
    names.retain(|name| seen.insert(name.to_ascii_lowercase()));
    names
}

fn valid_dns_name(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 253
        && value.split('.').all(|label| {
            !label.is_empty()
                && label.len() <= 63
                && !label.starts_with('-')
                && !label.ends_with('-')
                && label
                    .bytes()
                    .all(|byte| byte.is_ascii_alphanumeric() || byte == b'-')
        })
}

fn validate_tls_server_name(value: Option<&str>) -> Result<Option<String>> {
    let Some(value) = value else {
        return Ok(None);
    };
    if value.is_empty()
        || value != value.trim()
        || (value.parse::<IpAddr>().is_err() && !valid_dns_name(value))
    {
        return Err(ProxyError::new(
            ErrorCode::ConfigInvalid,
            "socket upstream TLS Server Name must be an exact DNS name or IP without a port, URL, or path",
        ));
    }
    Ok(Some(value.to_owned()))
}

fn trust_store(explicit_roots: &[Vec<u8>]) -> Result<openssl::x509::store::X509Store> {
    let native;
    let roots = if explicit_roots.is_empty() {
        native = rustls_native_certs::load_native_certs();
        if native.certs.is_empty() {
            return Err(ProxyError::new(
                ErrorCode::CertificateNotReady,
                format!(
                    "system trust store contains no usable roots: {:?}",
                    native.errors
                ),
            ));
        }
        native
            .certs
            .iter()
            .map(|certificate| certificate.as_ref())
            .collect::<Vec<_>>()
    } else {
        explicit_roots.iter().map(Vec::as_slice).collect::<Vec<_>>()
    };
    let mut builder = X509StoreBuilder::new().map_err(config_error)?;
    let mut unique = HashSet::new();
    for certificate in roots {
        if unique.insert(certificate.to_vec()) {
            builder
                .add_cert(X509::from_der(certificate).map_err(config_error)?)
                .map_err(config_error)?;
        }
    }
    Ok(builder.build())
}

fn normalize_tls_version(version: &str) -> String {
    match version {
        "TLSv1" => "TLS 1.0".to_owned(),
        "TLSv1.1" => "TLS 1.1".to_owned(),
        "TLSv1.2" => "TLS 1.2".to_owned(),
        "TLSv1.3" => "TLS 1.3".to_owned(),
        other => other.to_owned(),
    }
}

fn config_error(error: impl fmt::Display) -> ProxyError {
    ProxyError::new(ErrorCode::ConfigInvalid, error.to_string())
}

fn handshake_error(error: impl fmt::Display) -> ProxyError {
    ProxyError::new(ErrorCode::TlsHandshakeFailed, error.to_string())
}
