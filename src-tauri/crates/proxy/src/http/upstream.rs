use super::{
    BoxIo, CancellationToken, ClientTlsAdapter, ConnectionContext, Debug, Duration, ErrorCode,
    FaultAction, ForwardRequest, InformationalResponseSink, MessageLimits, PipelinePorts,
    ProxyError, Result, SocketAddr, TcpStream, UpstreamConnector, UpstreamExchange,
    UpstreamSecurityEvidence, UpstreamTransportSecurity, UpstreamWriteReceipt, async_trait,
    timeout_stage,
};

#[derive(Debug, Clone)]
pub struct HyperUpstreamConnector {
    pub address: SocketAddr,
    /// TLS SNI and certificate hostname.
    pub host: String,
    /// HTTP Host header, including a non-default port when configured.
    pub host_header: String,
    pub rewrite_host: bool,
    pub tls: Option<ClientTlsAdapter>,
    pub connect_timeout: Duration,
    pub write_timeout: Duration,
    pub read_timeout: Duration,
    pub limits: MessageLimits,
}

impl HyperUpstreamConnector {
    async fn connect_upstream(
        &self,
        context: &ConnectionContext,
        ports: &dyn PipelinePorts,
        cancellation: &CancellationToken,
    ) -> Result<BoxIo> {
        let tcp = timeout_stage(
            self.connect_timeout,
            cancellation,
            TcpStream::connect(self.address),
            ErrorCode::UpstreamConnectTimeout,
        )
        .await?
        .map_err(|error| ProxyError::io("connect upstream", &error))?;
        tcp.set_nodelay(true)
            .map_err(|error| ProxyError::io("configure upstream", &error))?;
        let io: BoxIo = Box::new(tcp);

        let (io, evidence) = if let Some(tls) = &self.tls {
            let connected = timeout_stage(
                self.connect_timeout,
                cancellation,
                tls.connect_with_evidence(&self.host, io),
                ErrorCode::UpstreamConnectTimeout,
            )
            .await??;
            let evidence = UpstreamSecurityEvidence {
                resolved_address: self.address,
                transport: UpstreamTransportSecurity::Tls,
                tls_version: Some(connected.evidence.tls_version),
                cipher_suite: Some(connected.evidence.cipher_suite),
                peer_subject: Some(connected.evidence.peer.subject_summary),
                peer_sha256_fingerprint: Some(connected.evidence.peer.sha256_fingerprint),
                hostname_verification_enabled: Some(
                    connected.evidence.hostname_verification_enabled,
                ),
                client_identity_configured: connected.evidence.client_identity_configured,
                client_identity_submitted: connected.evidence.client_identity_submitted,
            };
            (connected.io, evidence)
        } else {
            (
                io,
                UpstreamSecurityEvidence {
                    resolved_address: self.address,
                    transport: UpstreamTransportSecurity::PlaintextHttp,
                    tls_version: None,
                    cipher_suite: None,
                    peer_subject: None,
                    peer_sha256_fingerprint: None,
                    hostname_verification_enabled: None,
                    client_identity_configured: false,
                    client_identity_submitted: false,
                },
            )
        };
        ports
            .upstream_security_established(context, &evidence)
            .await;
        Ok(io)
    }
}

#[async_trait]
impl UpstreamConnector for HyperUpstreamConnector {
    async fn send(
        &self,
        context: &ConnectionContext,
        ports: &dyn PipelinePorts,
        mut request: ForwardRequest,
        actions: &[FaultAction],
        write_receipt: UpstreamWriteReceipt,
        informational: Option<&InformationalResponseSink>,
        cancellation: &CancellationToken,
    ) -> Result<UpstreamExchange> {
        request
            .message
            .normalize_for_forward(&self.host_header, self.rewrite_host);
        super::egress::send_http1_exchange(
            self.connect_upstream(context, ports, cancellation),
            request,
            actions,
            super::egress::Http1TransportSettings {
                write_timeout: self.write_timeout,
                read_timeout: self.read_timeout,
                limits: self.limits,
            },
            informational,
            &write_receipt,
            cancellation,
        )
        .await
    }
}
