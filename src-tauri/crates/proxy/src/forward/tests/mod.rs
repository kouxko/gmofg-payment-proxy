//! 正向代理测试共享夹具。
//!
//! 各职责文件通过 `include!` 进入同一个私有测试模块，因此仍可直接访问生产模块的私有边界，
//! 同时不会为了测试拆分而扩大任何运行时 API 的可见性。

use super::*;
use http::HeaderMap;
use http::header::{CONNECTION, UPGRADE};
use http_body_util::BodyExt;
use rcgen::{
    BasicConstraints, CertificateParams, IsCa, Issuer, KeyPair, KeyUsagePurpose,
    PKCS_ECDSA_P256_SHA256, SanType,
};
use rustls::pki_types::{CertificateDer, PrivateKeyDer, PrivatePkcs8KeyDer, ServerName};
use rustls::{ClientConfig, RootCertStore, ServerConfig};
use std::net::IpAddr;
use std::sync::Mutex as StdMutex;
use std::sync::atomic::{AtomicUsize, Ordering};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpListener;
use tokio::sync::{Mutex, oneshot};
use tokio_rustls::TlsConnector;

use super::config::MitmLeafCache;
use super::pipeline::response_from_pipeline_disposition;
use crate::fault::{FaultAction, ResponseDisposition};
use crate::message::Message;
use crate::message::RawHeader;
use crate::traffic::TrafficSchedule;
use crate::transport::HandshakePolicy;

fn loopback_config() -> ForwardProxyConfig {
    ForwardProxyConfig {
        bind_addr: "127.0.0.1:8080".parse().unwrap(),
        authentication: ForwardAuthenticationMode::None,
        allowed_client_cidrs: Vec::new(),
        connect_timeout: Duration::from_secs(1),
        read_timeout: Duration::from_secs(1),
        write_timeout: Duration::from_secs(1),
        tunnel_idle_timeout: Duration::from_secs(1),
    }
}

#[derive(Debug, Default)]
struct CapturingPipelinePorts {
    requests: StdMutex<Vec<Message>>,
    responses: StdMutex<Vec<Message>>,
    mutate: bool,
    request_actions: Vec<FaultAction>,
}

impl HandshakePolicy for CapturingPipelinePorts {}

#[async_trait::async_trait]
impl PipelinePorts for CapturingPipelinePorts {
    async fn request(
        &self,
        _context: &ConnectionContext,
        message: &mut Message,
    ) -> Result<Vec<FaultAction>> {
        self.requests.lock().unwrap().push(message.clone());
        if self.mutate {
            message.headers.push(RawHeader::new("X-Rule", "applied"));
            message.replace_body(Bytes::from_static(b"rule-request"));
        }
        Ok(self.request_actions.clone())
    }

    async fn response(
        &self,
        _context: &ConnectionContext,
        message: &mut Message,
    ) -> Result<Vec<FaultAction>> {
        self.responses.lock().unwrap().push(message.clone());
        if self.mutate {
            message.replace_body(Bytes::from_static(b"rule-response"));
            return Ok(vec![FaultAction::CustomStatus(StatusCode::CREATED)]);
        }
        Ok(Vec::new())
    }
}

#[derive(Debug, Default)]
struct CountingCertificateAuthority {
    issued: AtomicUsize,
}

impl CountingCertificateAuthority {
    fn count(&self) -> usize {
        self.issued.load(Ordering::SeqCst)
    }
}

impl MitmCertificateAuthority for CountingCertificateAuthority {
    fn issue_server_identity(&self, authority_host: &str) -> Result<MitmServerIdentity> {
        self.issued.fetch_add(1, Ordering::SeqCst);
        let key = KeyPair::generate_for(&PKCS_ECDSA_P256_SHA256).unwrap();
        let mut params = CertificateParams::default();
        params.subject_alt_names = authority_host.parse::<IpAddr>().map_or_else(
            |_| {
                vec![SanType::DnsName(
                    authority_host.to_owned().try_into().unwrap(),
                )]
            },
            |address| vec![SanType::IpAddress(address)],
        );
        let certificate = params.self_signed(&key).unwrap();
        Ok(MitmServerIdentity {
            certificate_chain_der: vec![certificate.der().to_vec()],
            private_key_pkcs8_der: zeroize::Zeroizing::new(key.serialize_der()),
        })
    }
}

#[derive(Debug)]
struct NeverMitmUpstreamConnector;

#[async_trait::async_trait]
impl MitmUpstreamConnector for NeverMitmUpstreamConnector {
    async fn connect(
        &self,
        _authority_host: &str,
        _upstream: TcpStream,
        _cancellation: &CancellationToken,
    ) -> Result<BoxIo> {
        panic!("allowlist-excluded CONNECT must never enter MITM upstream TLS")
    }
}

fn client_hello_with_alpn(protocol: &[u8]) -> Bytes {
    let mut alpn = Vec::new();
    let list_len = 1 + protocol.len();
    alpn.extend_from_slice(&u16::try_from(list_len).unwrap().to_be_bytes());
    alpn.push(u8::try_from(protocol.len()).unwrap());
    alpn.extend_from_slice(protocol);
    let mut extensions = Vec::new();
    extensions.extend_from_slice(&16u16.to_be_bytes());
    extensions.extend_from_slice(&u16::try_from(alpn.len()).unwrap().to_be_bytes());
    extensions.extend_from_slice(&alpn);
    let mut hello = Vec::new();
    hello.extend_from_slice(&[0x03, 0x03]);
    hello.extend_from_slice(&[0x42; 32]);
    hello.push(0); // session ID
    hello.extend_from_slice(&2u16.to_be_bytes());
    hello.extend_from_slice(&0x1301u16.to_be_bytes());
    hello.push(1);
    hello.push(0);
    hello.extend_from_slice(&u16::try_from(extensions.len()).unwrap().to_be_bytes());
    hello.extend_from_slice(&extensions);
    let mut handshake = vec![1];
    let length = hello.len();
    handshake.extend_from_slice(&[
        u8::try_from((length >> 16) & 0xff).unwrap(),
        u8::try_from((length >> 8) & 0xff).unwrap(),
        u8::try_from(length & 0xff).unwrap(),
    ]);
    handshake.extend_from_slice(&hello);
    let mut record = vec![22, 0x03, 0x01];
    record.extend_from_slice(&u16::try_from(handshake.len()).unwrap().to_be_bytes());
    record.extend_from_slice(&handshake);
    Bytes::from(record)
}

fn fragmented_client_hello_with_alpn(protocol: &[u8]) -> Bytes {
    let record = client_hello_with_alpn(protocol);
    let payload = &record[5..];
    let split = 7.min(payload.len());
    let mut records = Vec::new();
    for fragment in [&payload[..split], &payload[split..]] {
        records.extend_from_slice(&[22, 0x03, 0x01]);
        records.extend_from_slice(&u16::try_from(fragment.len()).unwrap().to_be_bytes());
        records.extend_from_slice(fragment);
    }
    Bytes::from(records)
}

// 基础协议转换与管线语义。
include!("request_semantics.rs");
// 监听安全、CIDR 与 MITM 缓存配置。
include!("configuration.rs");
// 普通 HTTP 转发及 DropResponse 边界。
include!("plain_http.rs");
// CONNECT 透明隧道、双向复制与 HTTP/2 透传。
include!("connect_tunnel.rs");
// WebSocket 仅拦截握手，升级后透明转发。
include!("websocket.rs");
// HTTPS MITM、原始字节透传与响应丢弃边界。
// HTTPS MITM 专用证书、TLS 与响应丢弃测试夹具。
include!("mitm_support.rs");
include!("mitm.rs");
// Listener 取消和空闲连接回收。
include!("listener_lifecycle.rs");
