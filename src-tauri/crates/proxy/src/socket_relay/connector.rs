use std::{net::SocketAddr, sync::Arc, time::Duration};

use futures_util::{StreamExt, stream::FuturesUnordered};
use tokio::{
    io::AsyncWriteExt,
    net::{TcpStream, lookup_host},
};
use tokio_util::sync::CancellationToken;

use crate::reverse::{DownstreamTlsAcceptor, ReverseClientIdentity, ReverseDownstreamTls};
use crate::transport::relay::timeout_cancel_first;
use crate::transport::{AcceptedConnection, BoxIo, ConnectionContext};
use crate::{ChannelId, ErrorCode, ProxyError, Result};

use super::upstream_tls::{SocketUpstreamTlsConnector, build_socket_upstream_tls_connector};
use super::{
    SocketDownstreamSecurity, SocketDownstreamTlsConfig, SocketEndpoint, SocketRelayDirection,
    SocketRelayFailure, SocketRelaySecurity, SocketRelayStage, SocketTlsEvidence,
    SocketTlsIdentity, SocketTransportMode, SocketUpstreamConnectionTestResult,
    SocketUpstreamTlsConfig, SocketUpstreamTransport,
};

#[derive(Debug)]
pub(super) struct SocketPreparationFailure {
    pub(super) error: ProxyError,
    pub(super) failure: SocketRelayFailure,
}

impl SocketPreparationFailure {
    fn new(
        error: ProxyError,
        stage: SocketRelayStage,
        direction: Option<SocketRelayDirection>,
    ) -> Self {
        Self {
            failure: SocketRelayFailure {
                stage,
                direction,
                code: error.code,
            },
            error,
        }
    }
}

#[derive(Clone, Debug)]
pub(super) struct PreparedSocketSecurity {
    downstream: Option<DownstreamTlsAcceptor>,
    upstream: Option<Arc<dyn SocketUpstreamTlsConnector>>,
    mode: SocketTransportMode,
}

pub(super) struct ConnectedSocket {
    pub(super) downstream: BoxIo,
    pub(super) upstream: BoxIo,
    pub(super) resolved_address: SocketAddr,
    pub(super) downstream_tls_peer: Option<String>,
    pub(super) upstream_tls: Option<SocketTlsEvidence>,
}

/// App 侧已接纳的 Socket 连接。
///
/// 该结果故意不包含上游地址、连接耗时或上游 TLS 证据。LocalResponder 只拿到这个
/// 类型，因此后续代码无法把一次下游 TLS 握手误报成“已连接上游”。
pub(super) struct AcceptedDownstreamSocket {
    pub(super) downstream: BoxIo,
    pub(super) downstream_tls_peer: Option<String>,
}

impl PreparedSocketSecurity {
    pub(super) fn build(security: &SocketRelaySecurity) -> Result<Self> {
        let (downstream, upstream, mode) = match security {
            SocketRelaySecurity::Transparent => (None, None, SocketTransportMode::Transparent),
            SocketRelaySecurity::TcpToTls { upstream_tls } => (
                None,
                Some(upstream_adapter(upstream_tls)?),
                SocketTransportMode::TcpToTls,
            ),
            SocketRelaySecurity::TlsToTcp { downstream_tls } => (
                Some(downstream_acceptor(downstream_tls)?),
                None,
                SocketTransportMode::TlsToTcp,
            ),
            SocketRelaySecurity::TlsToTls {
                downstream_tls,
                upstream_tls,
            } => (
                Some(downstream_acceptor(downstream_tls)?),
                Some(upstream_adapter(upstream_tls)?),
                SocketTransportMode::TlsToTls,
            ),
        };
        Ok(Self {
            downstream,
            upstream,
            mode,
        })
    }

    /// 只构造 App 侧的接入安全能力，不创建 resolver、TCP dialer 或上游 TLS adapter。
    ///
    /// 这个构造器是 `LocalResponder` 与 Relay 的安全边界：即使调用方配置错误，返回值
    /// 中也没有可用于建立上游连接的 adapter。
    pub(super) fn build_downstream(security: &SocketDownstreamSecurity) -> Result<Self> {
        let (downstream, mode) = match security {
            SocketDownstreamSecurity::Tcp => (None, SocketTransportMode::Transparent),
            SocketDownstreamSecurity::Tls { downstream_tls } => (
                Some(downstream_acceptor(downstream_tls)?),
                SocketTransportMode::TlsToTcp,
            ),
        };
        Ok(Self {
            downstream,
            upstream: None,
            mode,
        })
    }

    pub(super) fn mode(&self) -> SocketTransportMode {
        self.mode.clone()
    }

    pub(super) async fn connect(
        &self,
        downstream: BoxIo,
        peer: SocketAddr,
        endpoint: &SocketEndpoint,
        connect_timeout: Duration,
        cancellation: &CancellationToken,
    ) -> std::result::Result<ConnectedSocket, SocketPreparationFailure> {
        let downstream = self
            .accept_downstream(downstream, peer, connect_timeout, cancellation)
            .await
            .map_err(|error| {
                SocketPreparationFailure::new(
                    error,
                    SocketRelayStage::DownstreamTls,
                    Some(SocketRelayDirection::Downstream),
                )
            })?;
        let addresses = resolve(endpoint, connect_timeout, cancellation)
            .await
            .map_err(|error| SocketPreparationFailure::new(error, SocketRelayStage::Dns, None))?;
        let (resolved_address, upstream) = connect_tcp(&addresses, connect_timeout, cancellation)
            .await
            .map_err(|error| {
                SocketPreparationFailure::new(error, SocketRelayStage::Connect, None)
            })?;
        let (upstream, upstream_tls) = self
            .connect_upstream(upstream, endpoint, connect_timeout, cancellation)
            .await
            .map_err(|error| {
                SocketPreparationFailure::new(
                    error,
                    SocketRelayStage::UpstreamTls,
                    Some(SocketRelayDirection::Upstream),
                )
            })?;
        Ok(ConnectedSocket {
            downstream: downstream.downstream,
            upstream,
            resolved_address,
            downstream_tls_peer: downstream.downstream_tls_peer,
            upstream_tls,
        })
    }

    pub(super) async fn test_upstream(
        &self,
        endpoint: &SocketEndpoint,
        connect_timeout: Duration,
        cancellation: &CancellationToken,
    ) -> Result<SocketUpstreamConnectionTestResult> {
        let started = std::time::Instant::now();
        let addresses = resolve(endpoint, connect_timeout, cancellation).await?;
        let (resolved_address, tcp) =
            connect_tcp(&addresses, connect_timeout, cancellation).await?;
        let (mut io, tls, tls_server_name_candidates) = self
            .test_connect_upstream(tcp, endpoint, connect_timeout, cancellation)
            .await?;
        let _ = io.shutdown().await;
        Ok(SocketUpstreamConnectionTestResult {
            resolved_address,
            transport: if tls.is_some() {
                SocketUpstreamTransport::Tls
            } else {
                SocketUpstreamTransport::Tcp
            },
            tls,
            tls_server_name_candidates,
            elapsed_millis: u64::try_from(started.elapsed().as_millis()).unwrap_or(u64::MAX),
        })
    }

    /// 接纳 App 侧连接；纯 TCP 原样返回，TLS 模式仅在本地完成服务端握手。
    ///
    /// 此方法不解析上游地址，也不会调用 DNS、TCP connect 或上游 TLS。调用方可据此
    /// 实现不配置上游的 `LocalResponder`。
    pub(super) async fn accept_downstream(
        &self,
        io: BoxIo,
        peer: SocketAddr,
        timeout: Duration,
        cancellation: &CancellationToken,
    ) -> Result<AcceptedDownstreamSocket> {
        let Some(acceptor) = &self.downstream else {
            return Ok(AcceptedDownstreamSocket {
                downstream: io,
                downstream_tls_peer: None,
            });
        };
        let context = ConnectionContext {
            runtime_epoch: uuid::Uuid::new_v4(),
            connection_id: uuid::Uuid::new_v4(),
            channel: ChannelId::new("socket-relay")?,
            peer_addr: peer,
            accepted_at: std::time::SystemTime::now(),
            tls_peer: None,
        };
        let accepted: AcceptedConnection = timeout_cancel_first(
            timeout,
            cancellation,
            acceptor.accept(io, &context),
            ErrorCode::SocketDownstreamTlsTimeout,
            "socket connection cancelled during downstream TLS",
            "socket downstream TLS handshake",
        )
        .await?
        .map_err(|error| ProxyError::new(ErrorCode::SocketDownstreamTlsFailed, error.message))?;
        Ok(AcceptedDownstreamSocket {
            downstream: accepted.io,
            downstream_tls_peer: accepted.tls_peer.map(|peer| peer.sha256_fingerprint),
        })
    }

    async fn connect_upstream(
        &self,
        io: BoxIo,
        endpoint: &SocketEndpoint,
        timeout: Duration,
        cancellation: &CancellationToken,
    ) -> Result<(BoxIo, Option<SocketTlsEvidence>)> {
        let Some(connector) = &self.upstream else {
            return Ok((io, None));
        };
        let connected = timeout_cancel_first(
            timeout,
            cancellation,
            connector.connect(&endpoint.host, io),
            ErrorCode::SocketUpstreamTlsTimeout,
            "socket relay cancelled during upstream TLS",
            "socket upstream TLS handshake",
        )
        .await?
        .map_err(|error| ProxyError::new(ErrorCode::SocketUpstreamTlsFailed, error.message))?;
        Ok((connected.io, Some(connected.evidence)))
    }

    async fn test_connect_upstream(
        &self,
        io: BoxIo,
        endpoint: &SocketEndpoint,
        timeout: Duration,
        cancellation: &CancellationToken,
    ) -> Result<(BoxIo, Option<SocketTlsEvidence>, Vec<String>)> {
        let Some(connector) = &self.upstream else {
            return Ok((io, None, Vec::new()));
        };
        let discovering = connector.requires_server_name_discovery(&endpoint.host);
        let connect = async {
            if discovering {
                connector.discover_server_names(&endpoint.host, io).await
            } else {
                connector.connect(&endpoint.host, io).await
            }
        };
        let connected = timeout_cancel_first(
            timeout,
            cancellation,
            connect,
            ErrorCode::SocketUpstreamTlsTimeout,
            "socket relay cancelled during upstream TLS test",
            "socket upstream TLS test handshake",
        )
        .await?
        .map_err(|error| ProxyError::new(ErrorCode::SocketUpstreamTlsFailed, error.message))?;
        let candidates = if discovering {
            connected.server_name_candidates
        } else {
            Vec::new()
        };
        Ok((connected.io, Some(connected.evidence), candidates))
    }
}

async fn resolve(
    endpoint: &SocketEndpoint,
    timeout: Duration,
    cancellation: &CancellationToken,
) -> Result<Vec<SocketAddr>> {
    let addresses = timeout_cancel_first(
        timeout,
        cancellation,
        lookup_host((endpoint.host.as_str(), endpoint.port)),
        ErrorCode::SocketDnsTimeout,
        "socket relay cancelled during DNS",
        "socket upstream DNS",
    )
    .await?
    .map_err(|error| ProxyError::new(ErrorCode::SocketDnsFailed, error.to_string()))?;
    let addresses = addresses.collect::<Vec<_>>();
    if addresses.is_empty() {
        return Err(ProxyError::new(
            ErrorCode::SocketDnsFailed,
            "socket upstream DNS returned no addresses",
        ));
    }
    Ok(addresses)
}

async fn connect_tcp(
    addresses: &[SocketAddr],
    timeout: Duration,
    cancellation: &CancellationToken,
) -> Result<(SocketAddr, BoxIo)> {
    let (address, stream) = timeout_cancel_first(
        timeout,
        cancellation,
        connect_first_available(addresses),
        ErrorCode::SocketConnectTimeout,
        "socket relay cancelled during TCP connect",
        "socket upstream TCP connect",
    )
    .await?
    .map_err(|error| ProxyError::new(ErrorCode::SocketConnectFailed, error.to_string()))?;
    stream
        .set_nodelay(true)
        .map_err(|error| ProxyError::new(ErrorCode::SocketConnectFailed, error.to_string()))?;
    Ok((address, Box::new(stream)))
}

async fn connect_first_available(
    addresses: &[SocketAddr],
) -> std::io::Result<(SocketAddr, TcpStream)> {
    let mut attempts = addresses
        .iter()
        .copied()
        .map(|address| async move { (address, TcpStream::connect(address).await) })
        .collect::<FuturesUnordered<_>>();
    let mut last_error = None;
    while let Some((address, result)) = attempts.next().await {
        match result {
            Ok(stream) => return Ok((address, stream)),
            Err(error) => last_error = Some(error),
        }
    }
    Err(last_error.unwrap_or_else(|| {
        std::io::Error::new(
            std::io::ErrorKind::InvalidInput,
            "socket upstream has no resolved address",
        )
    }))
}

fn downstream_acceptor(config: &SocketDownstreamTlsConfig) -> Result<DownstreamTlsAcceptor> {
    DownstreamTlsAcceptor::new(&ReverseDownstreamTls {
        server_identity: reverse_identity(&config.server_identity),
        dynamic_server_identity: None,
        dynamic_server_name_allowlist: Vec::new(),
        client_trust_der: config.client_trust_der.clone(),
        client_authentication_required: config.client_authentication_required,
    })
}

fn upstream_adapter(
    config: &SocketUpstreamTlsConfig,
) -> Result<Arc<dyn SocketUpstreamTlsConnector>> {
    build_socket_upstream_tls_connector(config)
}

fn reverse_identity(identity: &SocketTlsIdentity) -> ReverseClientIdentity {
    ReverseClientIdentity {
        certificate_chain_der: identity.certificate_chain_der.clone(),
        private_key_pkcs8_der: identity.private_key_pkcs8_der.clone(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    use tokio::net::TcpListener;

    #[tokio::test]
    async fn connect_tries_all_resolved_addresses_under_one_deadline() {
        let target = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let target_address = target.local_addr().unwrap();
        let unavailable = {
            let reservation = TcpListener::bind("127.0.0.1:0").await.unwrap();
            let address = reservation.local_addr().unwrap();
            drop(reservation);
            address
        };
        let accepted = tokio::spawn(async move { target.accept().await.unwrap() });

        let (connected_address, _stream) = connect_tcp(
            &[unavailable, target_address],
            Duration::from_secs(1),
            &CancellationToken::new(),
        )
        .await
        .expect("second resolved address must be attempted");

        assert_eq!(connected_address, target_address);
        accepted.await.unwrap();
    }

    #[tokio::test]
    async fn downstream_only_tcp_accept_never_needs_an_upstream_endpoint() {
        let prepared = PreparedSocketSecurity::build_downstream(&SocketDownstreamSecurity::Tcp)
            .expect("plain downstream security must build");
        assert_eq!(prepared.mode(), SocketTransportMode::Transparent);

        let (proxy_side, mut app_side) = tokio::io::duplex(32);
        let accepted = prepared
            .accept_downstream(
                Box::new(proxy_side),
                "127.0.0.1:12345".parse().unwrap(),
                Duration::from_secs(1),
                &CancellationToken::new(),
            )
            .await
            .expect("plain downstream connection must be accepted locally");
        assert!(accepted.downstream_tls_peer.is_none());

        let mut downstream = accepted.downstream;
        downstream.write_all(b"reply").await.unwrap();
        let mut bytes = [0_u8; 5];
        app_side.read_exact(&mut bytes).await.unwrap();
        assert_eq!(&bytes, b"reply");
    }
}
