use super::{
    AsyncWriteExt, BoxIo, CancellationToken, ClientTlsAdapter, ConnectionContext, Debug, Duration,
    ErrorCode, FaultAction, ForwardRequest, Http1ExchangeConfig, InformationalResponseSink,
    InjectedTimeoutStage, MessageLimits, PipelinePorts, ProxyError, Result, SocketAddr, TcpStream,
    TrafficDirection, UpstreamConnector, UpstreamExchange, UpstreamSecurityEvidence,
    UpstreamTransportSecurity, async_trait, injected_timeout, send_http1_request,
    send_scheduled_upstream_abort, timeout_stage, traffic_schedule, wait_for_injected_timeout,
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

    async fn execute_injected_write(
        &self,
        io: &mut BoxIo,
        request: &ForwardRequest,
        actions: &[FaultAction],
        cancellation: &CancellationToken,
    ) -> Result<UpstreamExchange> {
        let wire_request = request.message.reconstruct();
        timeout_stage(
            self.write_timeout,
            cancellation,
            io.write_all(&wire_request),
            ErrorCode::UpstreamWriteTimeout,
        )
        .await?
        .map_err(|error| ProxyError::io("write injected upstream request", &error))?;
        timeout_stage(
            self.write_timeout,
            cancellation,
            io.flush(),
            ErrorCode::UpstreamWriteTimeout,
        )
        .await?
        .map_err(|error| ProxyError::io("flush injected upstream request", &error))?;

        if drops_response_without_reading(actions) {
            timeout_stage(
                self.write_timeout,
                cancellation,
                io.shutdown(),
                ErrorCode::UpstreamWriteTimeout,
            )
            .await?
            .map_err(|error| ProxyError::io("close injected upstream request", &error))?;
            return Err(ProxyError::new(
                ErrorCode::ClientDisconnected,
                "upstream request intentionally closed after complete write",
            ));
        }

        wait_for_injected_timeout(actions, InjectedTimeoutStage::Read, cancellation).await?;
        Err(ProxyError::new(
            ErrorCode::Internal,
            "injected read timeout unexpectedly completed",
        ))
    }
}

fn drops_response_without_reading(actions: &[FaultAction]) -> bool {
    actions.iter().any(|action| {
        matches!(
            action,
            FaultAction::DropResponse {
                read_upstream: false
            }
        )
    })
}

#[async_trait]
impl UpstreamConnector for HyperUpstreamConnector {
    async fn send(
        &self,
        context: &ConnectionContext,
        ports: &dyn PipelinePorts,
        mut request: ForwardRequest,
        actions: &[FaultAction],
        informational: Option<&InformationalResponseSink>,
        cancellation: &CancellationToken,
    ) -> Result<UpstreamExchange> {
        wait_for_injected_timeout(actions, InjectedTimeoutStage::Connect, cancellation).await?;

        request
            .message
            .normalize_for_forward(&self.host_header, self.rewrite_host);
        let mut io = self.connect_upstream(context, ports, cancellation).await?;

        wait_for_injected_timeout(actions, InjectedTimeoutStage::Write, cancellation).await?;
        let schedule = traffic_schedule(actions, TrafficDirection::Upstream)?;
        if schedule.disconnect_after_bytes.is_some() {
            return send_scheduled_upstream_abort(
                &mut io,
                &request.message,
                schedule,
                self.write_timeout,
                cancellation,
            )
            .await
            .map(UpstreamExchange::from);
        }

        let close_after_request_write = drops_response_without_reading(actions);
        let inject_read_timeout = injected_timeout(actions, InjectedTimeoutStage::Read).is_some();
        if close_after_request_write || inject_read_timeout {
            return self
                .execute_injected_write(&mut io, &request, actions, cancellation)
                .await;
        }

        send_http1_request(
            io,
            request,
            Http1ExchangeConfig {
                schedule,
                write_timeout: self.write_timeout,
                read_timeout: self.read_timeout,
                limits: self.limits,
            },
            informational,
            cancellation,
        )
        .await
    }
}
