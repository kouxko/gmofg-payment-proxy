use std::{net::SocketAddr, time::Duration};

use tokio::{
    io::AsyncWriteExt,
    net::{TcpStream, lookup_host},
};
use tokio_util::sync::CancellationToken;

use crate::reverse::{
    DownstreamTlsAcceptor, ReverseClientIdentity, ReverseDownstreamTls, ReverseUpstreamTls,
    build_client_connector,
};
use crate::tls::ClientTlsAdapter;
use crate::transport::relay::timeout_cancel_first;
use crate::transport::{AcceptedConnection, BoxIo, ConnectionContext};
use crate::{ChannelId, ErrorCode, ProxyError, Result};

use super::{
    SocketDownstreamTlsConfig, SocketEndpoint, SocketRelayDirection, SocketRelayFailure,
    SocketRelaySecurity, SocketRelayStage, SocketTlsEvidence, SocketTlsIdentity,
    SocketTransportMode, SocketUpstreamConnectionTestResult, SocketUpstreamTlsConfig,
    SocketUpstreamTransport,
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
    upstream: Option<ClientTlsAdapter>,
    mode: SocketTransportMode,
}

pub(super) struct ConnectedSocket {
    pub(super) downstream: BoxIo,
    pub(super) upstream: BoxIo,
    pub(super) resolved_address: SocketAddr,
    pub(super) downstream_tls_peer: Option<String>,
    pub(super) upstream_tls: Option<SocketTlsEvidence>,
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
            downstream: downstream.io,
            upstream,
            resolved_address,
            downstream_tls_peer: downstream.tls_peer.map(|peer| peer.sha256_fingerprint),
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
        let (mut io, tls) = self
            .connect_upstream(tcp, endpoint, connect_timeout, cancellation)
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
            elapsed_millis: u64::try_from(started.elapsed().as_millis()).unwrap_or(u64::MAX),
        })
    }

    async fn accept_downstream(
        &self,
        io: BoxIo,
        peer: SocketAddr,
        timeout: Duration,
        cancellation: &CancellationToken,
    ) -> Result<AcceptedConnection> {
        let Some(acceptor) = &self.downstream else {
            return Ok(AcceptedConnection { io, tls_peer: None });
        };
        let context = ConnectionContext {
            runtime_epoch: uuid::Uuid::new_v4(),
            connection_id: uuid::Uuid::new_v4(),
            channel: ChannelId::new("socket-relay")?,
            peer_addr: peer,
            accepted_at: std::time::SystemTime::now(),
            tls_peer: None,
        };
        timeout_cancel_first(
            timeout,
            cancellation,
            acceptor.accept(io, &context),
            ErrorCode::SocketDownstreamTlsTimeout,
            "socket relay cancelled during downstream TLS",
            "socket downstream TLS handshake",
        )
        .await?
        .map_err(|error| ProxyError::new(ErrorCode::SocketDownstreamTlsFailed, error.message))
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
            connector.connect_with_evidence(&endpoint.host, io),
            ErrorCode::SocketUpstreamTlsTimeout,
            "socket relay cancelled during upstream TLS",
            "socket upstream TLS handshake",
        )
        .await?
        .map_err(|error| ProxyError::new(ErrorCode::SocketUpstreamTlsFailed, error.message))?;
        let evidence = SocketTlsEvidence {
            tls_version: connected.evidence.tls_version,
            cipher_suite: connected.evidence.cipher_suite,
            peer_subject: connected.evidence.peer.subject_summary,
            peer_sha256_fingerprint: connected.evidence.peer.sha256_fingerprint,
            hostname_verification_enabled: connected.evidence.hostname_verification_enabled,
            client_identity_configured: connected.evidence.client_identity_configured,
        };
        Ok((connected.io, Some(evidence)))
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
    let deadline = tokio::time::Instant::now() + timeout;
    let mut last_error = None;
    for &address in addresses {
        let remaining = deadline.saturating_duration_since(tokio::time::Instant::now());
        if remaining.is_zero() {
            return Err(ProxyError::new(
                ErrorCode::SocketConnectTimeout,
                format!(
                    "socket upstream TCP connect timed out after {} ms",
                    timeout.as_millis()
                ),
            ));
        }
        let result = timeout_cancel_first(
            remaining,
            cancellation,
            TcpStream::connect(address),
            ErrorCode::SocketConnectTimeout,
            "socket relay cancelled during TCP connect",
            "socket upstream TCP connect",
        )
        .await?;
        match result {
            Ok(stream) => {
                stream.set_nodelay(true).map_err(|error| {
                    ProxyError::new(ErrorCode::SocketConnectFailed, error.to_string())
                })?;
                return Ok((address, Box::new(stream)));
            }
            Err(error) => last_error = Some(error),
        }
    }
    Err(ProxyError::new(
        ErrorCode::SocketConnectFailed,
        last_error.map_or_else(
            || "socket upstream has no resolved address".to_owned(),
            |error| error.to_string(),
        ),
    ))
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

fn upstream_adapter(config: &SocketUpstreamTlsConfig) -> Result<ClientTlsAdapter> {
    build_client_connector(&ReverseUpstreamTls {
        server_trust_der: config.server_trust_der.clone(),
        client_identity: config.client_identity.as_ref().map(reverse_identity),
        verify_hostname: config.verify_hostname,
    })
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
}
