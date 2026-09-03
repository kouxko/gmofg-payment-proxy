//! 动态反向代理监听器。
//!
//! 该运行时只接收已经解析为 DER 的不可变证书快照，不知道 Workspace、SQLite、文件
//! 或系统密钥库。每个下游连接只连接配置中的固定上游 origin；TLS 在两端分别终止，
//! HTTP 字节流（包括 Header 与 Body）在未进入规则修改管线时逐字节转发。

use std::{
    net::SocketAddr,
    sync::Arc,
    time::{Duration, Instant},
};

use http::Uri;
use rustls::{
    ClientConfig, DigitallySignedStruct, RootCertStore, ServerConfig, SignatureScheme,
    client::{
        WebPkiServerVerifier,
        danger::{HandshakeSignatureValid, ServerCertVerified, ServerCertVerifier},
    },
    pki_types::{CertificateDer, PrivateKeyDer, PrivatePkcs8KeyDer, ServerName, UnixTime},
    server::WebPkiClientVerifier,
    version::TLS12,
};
use tokio::{
    io::AsyncWriteExt,
    net::{TcpListener, TcpStream},
};
use tokio_rustls::TlsAcceptor;
use tokio_util::sync::CancellationToken;
use uuid::Uuid;
use x509_parser::{extensions::GeneralName, parse_x509_certificate};
use zeroize::Zeroizing;

#[cfg(test)]
use tokio::io::AsyncReadExt;

use crate::http::{ConnectionAdmission, ConnectionService, HyperUpstreamConnector, PipelinePorts};
use crate::message::MessageLimits;
use crate::supervisor::ChannelId;
use crate::tls::ClientTlsAdapter;
use crate::transport::{BoxIo as TransportBoxIo, ConnectionAcceptor, SystemClock};
use crate::{ErrorCode, MitmCertificateAuthority, ProxyError, Result};

mod admission;
mod dynamic_identity;

pub use admission::DownstreamTlsAcceptor;
pub(crate) use admission::ReverseConnectionAcceptor;
use dynamic_identity::{DynamicServerIdentityResolver, certified_key};
mod config;
mod endpoint;
mod relay;
mod service;
mod tls_config;

pub use config::{
    ReverseClientIdentity, ReverseDownstreamTls, ReverseProxyConfig, ReverseUpstreamTls,
    UpstreamConnectionTestResult, UpstreamScheme, UpstreamTlsHandshakeResult, UpstreamTransport,
};
pub use service::ReverseProxyService;

use endpoint::UpstreamEndpoint;
use relay::{relay_exact, timeout_cancel};
pub(crate) use tls_config::build_client_connector;
use tls_config::{build_server_acceptor, config_error};

type BoxIo = TransportBoxIo;

#[cfg(test)]
mod tests;
