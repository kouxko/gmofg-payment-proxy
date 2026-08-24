use std::{net::SocketAddr, sync::Arc, time::SystemTime};

use async_trait::async_trait;
use tokio_util::sync::CancellationToken;
use uuid::Uuid;

use crate::listener::{ConnectionHandler, ConnectionTaskScope, PrimaryConnectionOutcome, sealed};
use crate::transport::BoxIo;
use crate::transport::ConnectionContext;
use crate::transport::relay::{RelayBytes, RelayFailure, RelayProgress};
use crate::{ErrorCode, ProxyError, Result};

use super::connector::PreparedSocketSecurity;
use super::handler_support::{
    SocketHandlerConfig, SocketHandlerProcessing, connection_identity, normalize_cancelled,
    processing_failure, socket_failure,
};
use super::{
    SocketConnectionEvent, SocketConnectionObserver, SocketLocalResponderConfig,
    SocketOpenedEvidence, SocketPipelineLimits, SocketProcessingFailure,
    SocketProtocolCapabilityFactory, SocketRelayConfig, SocketRelayDirection, SocketRelayFailure,
    SocketRelayMetrics, SocketRelayRunContext, SocketRelayStage,
    SocketUpstreamConnectionTestResult,
};

#[derive(Debug)]
pub(crate) struct SocketConnectionHandler {
    config: SocketHandlerConfig,
    security: PreparedSocketSecurity,
    processing: SocketHandlerProcessing,
    observer: Arc<dyn SocketConnectionObserver>,
    metrics: Arc<SocketRelayMetrics>,
    run: Arc<std::sync::RwLock<SocketRelayRunContext>>,
}

mod exchange;
mod local_raw;

impl SocketConnectionHandler {
    pub(crate) fn build_direct(
        config: SocketRelayConfig,
        observer: Arc<dyn SocketConnectionObserver>,
        metrics: Arc<SocketRelayMetrics>,
        run: Arc<std::sync::RwLock<SocketRelayRunContext>>,
    ) -> Result<Self> {
        config.validate()?;
        let security = PreparedSocketSecurity::build(&config.security)?;
        Ok(Self {
            config: SocketHandlerConfig::Relay(config),
            security,
            processing: SocketHandlerProcessing::Direct,
            observer,
            metrics,
            run,
        })
    }

    pub(crate) fn build_direct_local(
        config: SocketLocalResponderConfig,
        observer: Arc<dyn SocketConnectionObserver>,
        metrics: Arc<SocketRelayMetrics>,
        run: Arc<std::sync::RwLock<SocketRelayRunContext>>,
    ) -> Result<Self> {
        config.validate()?;
        let security = PreparedSocketSecurity::build_downstream(&config.security)?;
        Ok(Self {
            config: SocketHandlerConfig::LocalResponder(config),
            security,
            processing: SocketHandlerProcessing::DirectLocal,
            observer,
            metrics,
            run,
        })
    }

    pub(crate) fn build_scripted(
        config: SocketRelayConfig,
        factory: Arc<dyn SocketProtocolCapabilityFactory>,
        limits: SocketPipelineLimits,
        observer: Arc<dyn SocketConnectionObserver>,
        metrics: Arc<SocketRelayMetrics>,
        run: Arc<std::sync::RwLock<SocketRelayRunContext>>,
    ) -> Result<Self> {
        config.validate()?;
        let security = PreparedSocketSecurity::build(&config.security)?;
        Ok(Self {
            config: SocketHandlerConfig::Relay(config),
            security,
            processing: SocketHandlerProcessing::ScriptedRelay { factory, limits },
            observer,
            metrics,
            run,
        })
    }

    pub(crate) fn build_local_responder(
        config: SocketLocalResponderConfig,
        factory: Arc<dyn SocketProtocolCapabilityFactory>,
        limits: SocketPipelineLimits,
        observer: Arc<dyn SocketConnectionObserver>,
        metrics: Arc<SocketRelayMetrics>,
        run: Arc<std::sync::RwLock<SocketRelayRunContext>>,
    ) -> Result<Self> {
        config.validate()?;
        let security = PreparedSocketSecurity::build_downstream(&config.security)?;
        Ok(Self {
            config: SocketHandlerConfig::LocalResponder(config),
            security,
            processing: SocketHandlerProcessing::LocalResponder { factory, limits },
            observer,
            metrics,
            run,
        })
    }

    async fn handle_inner(
        &self,
        io: BoxIo,
        peer: SocketAddr,
        connection_id: Uuid,
        cancellation: CancellationToken,
    ) -> Result<()> {
        let run = self.run();
        let progress = Arc::new(RelayProgress::default());
        self.metrics.admitted(connection_id, Arc::clone(&progress));
        self.observer.record(SocketConnectionEvent::Admitted {
            run: run.clone(),
            connection_id,
            peer,
            target: self.config.target(),
            mode: self.security.mode(),
            at: SystemTime::now(),
        });

        match (&self.config, &self.processing) {
            (SocketHandlerConfig::Relay(config), processing) => {
                self.handle_relay(
                    io,
                    peer,
                    connection_id,
                    run,
                    config,
                    processing,
                    cancellation,
                    progress,
                )
                .await
            }
            (
                SocketHandlerConfig::LocalResponder(config),
                SocketHandlerProcessing::LocalResponder { factory, limits },
            ) => {
                self.handle_local_responder(
                    io,
                    peer,
                    connection_id,
                    run,
                    config,
                    factory.as_ref(),
                    *limits,
                    cancellation,
                    progress,
                )
                .await
            }
            (SocketHandlerConfig::LocalResponder(config), SocketHandlerProcessing::DirectLocal) => {
                self.handle_local_raw_responder(
                    io,
                    peer,
                    connection_id,
                    run,
                    config,
                    cancellation,
                    progress,
                )
                .await
            }
            _ => unreachable!("socket handler topology and processing mode are built together"),
        }
    }

    #[allow(clippy::too_many_arguments)]
    async fn handle_local_responder(
        &self,
        io: BoxIo,
        peer: SocketAddr,
        connection_id: Uuid,
        run: SocketRelayRunContext,
        config: &SocketLocalResponderConfig,
        factory: &dyn SocketProtocolCapabilityFactory,
        limits: SocketPipelineLimits,
        cancellation: CancellationToken,
        progress: Arc<RelayProgress>,
    ) -> Result<()> {
        let accepted = self
            .security
            .accept_downstream(io, peer, config.handshake_timeout, &cancellation)
            .await;
        let accepted = match accepted {
            Ok(accepted) => accepted,
            Err(error) => {
                let error = normalize_cancelled(error);
                let failure = SocketRelayFailure {
                    stage: if error.code == ErrorCode::SocketRelayCancelled.as_str() {
                        SocketRelayStage::Shutdown
                    } else {
                        SocketRelayStage::DownstreamTls
                    },
                    direction: Some(SocketRelayDirection::Downstream),
                    code: error.code,
                };
                self.close(
                    run,
                    connection_id,
                    false,
                    RelayBytes::default(),
                    Some(failure),
                );
                return Err(error);
            }
        };
        self.open(
            &run,
            connection_id,
            SocketOpenedEvidence::LocalResponder {
                downstream_tls_peer: accepted.downstream_tls_peer,
            },
        );
        let result = super::protocol_exchange::run_local_exchange(
            accepted.downstream,
            connection_identity(&run, connection_id, peer),
            factory,
            limits,
            config.read_timeout,
            config.write_timeout,
            cancellation,
            progress,
        )
        .await
        .map_err(|failure| match failure {
            super::protocol_exchange::ProtocolExchangeFailure::Processing(failure) => failure,
            super::protocol_exchange::ProtocolExchangeFailure::Preparation(_) => {
                unreachable!("LocalServer never prepares a remote connection")
            }
        });
        self.finish_processing(run, connection_id, result)
    }

    fn open(
        &self,
        run: &SocketRelayRunContext,
        connection_id: Uuid,
        evidence: SocketOpenedEvidence,
    ) {
        self.metrics.opened(connection_id);
        self.observer.record(SocketConnectionEvent::Opened {
            run: run.clone(),
            connection_id,
            evidence,
            at: SystemTime::now(),
        });
    }

    fn finish_preparation_failure(
        &self,
        run: SocketRelayRunContext,
        connection_id: Uuid,
        preparation: super::connector::SocketPreparationFailure,
    ) -> Result<()> {
        let error = normalize_cancelled(preparation.error);
        let failure = if error.code == ErrorCode::SocketRelayCancelled.as_str() {
            SocketRelayFailure {
                stage: SocketRelayStage::Shutdown,
                direction: preparation.failure.direction,
                code: error.code,
            }
        } else {
            SocketRelayFailure {
                code: error.code,
                ..preparation.failure
            }
        };
        self.close(
            run,
            connection_id,
            false,
            RelayBytes::default(),
            Some(failure),
        );
        Err(error)
    }

    fn finish_direct(
        &self,
        run: SocketRelayRunContext,
        connection_id: Uuid,
        result: std::result::Result<RelayBytes, RelayFailure>,
    ) -> Result<()> {
        match result {
            Ok(bytes) => {
                self.close(run, connection_id, true, bytes, None);
                Ok(())
            }
            Err(failure) => {
                let socket_failure = socket_failure(&failure);
                let code = socket_failure.code;
                let bytes = failure.bytes;
                self.close(run, connection_id, true, bytes, Some(socket_failure));
                Err(ProxyError {
                    code,
                    message: failure.error.message,
                })
            }
        }
    }

    fn finish_processing(
        &self,
        run: SocketRelayRunContext,
        connection_id: Uuid,
        result: std::result::Result<RelayBytes, SocketProcessingFailure>,
    ) -> Result<()> {
        match result {
            Ok(bytes) => {
                self.close(run, connection_id, true, bytes, None);
                Ok(())
            }
            Err(failure) => {
                let socket_failure = processing_failure(&failure);
                let bytes = failure.bytes();
                self.close(run, connection_id, true, bytes, Some(socket_failure));
                Err(ProxyError {
                    code: socket_failure.code,
                    message: "socket frame processing failed".into(),
                })
            }
        }
    }

    fn close(
        &self,
        run: SocketRelayRunContext,
        connection_id: Uuid,
        opened: bool,
        bytes: crate::transport::relay::RelayBytes,
        failure: Option<SocketRelayFailure>,
    ) {
        let bytes = self.metrics.closed(connection_id, opened, bytes);
        self.observer.record(SocketConnectionEvent::Closed {
            run,
            connection_id,
            target: self.config.target(),
            opened,
            bytes,
            failure,
            at: SystemTime::now(),
        });
    }

    fn run(&self) -> SocketRelayRunContext {
        self.run
            .read()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .clone()
    }

    pub(crate) async fn test_upstream_connection(
        &self,
    ) -> Result<SocketUpstreamConnectionTestResult> {
        let SocketHandlerConfig::Relay(config) = &self.config else {
            return Err(ProxyError::new(
                ErrorCode::ConfigInvalid,
                "local responder has no upstream connection to test",
            ));
        };
        self.security
            .test_upstream(
                &config.upstream,
                config.connect_timeout,
                &CancellationToken::new(),
            )
            .await
    }
}

impl sealed::Sealed for SocketConnectionHandler {}

#[async_trait]
impl ConnectionHandler for SocketConnectionHandler {
    async fn handle(
        &self,
        io: BoxIo,
        context: ConnectionContext,
        _child_tasks: ConnectionTaskScope,
        cancellation: CancellationToken,
    ) -> PrimaryConnectionOutcome {
        let connection_id = context.connection_id;
        let peer = context.peer_addr;
        let result = self
            .handle_inner(io, peer, connection_id, cancellation)
            .await;
        match result {
            Ok(()) => PrimaryConnectionOutcome::Success,
            Err(error) if error.code == ErrorCode::SocketRelayCancelled.as_str() => {
                PrimaryConnectionOutcome::Cancelled
            }
            Err(error) => PrimaryConnectionOutcome::Failed(error),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::transport::relay::{RelayDirection, RelayOperation};

    #[test]
    fn relay_failures_preserve_stage_direction_and_partial_counters() {
        let failure = RelayFailure {
            error: ProxyError::new(ErrorCode::Io, "forced write failure"),
            direction: RelayDirection::ClientToServer,
            operation: RelayOperation::Write,
            bytes: RelayBytes {
                client_to_server: 37,
                server_to_client: 11,
            },
        };

        let mapped = socket_failure(&failure);
        assert_eq!(mapped.stage, SocketRelayStage::RelayWrite);
        assert_eq!(mapped.direction, Some(SocketRelayDirection::ClientToServer));
        assert_eq!(mapped.code, "SOCKET_WRITE_FAILED");
        assert_eq!(failure.bytes.client_to_server, 37);
        assert_eq!(failure.bytes.server_to_client, 11);
    }

    #[test]
    fn framed_cancellation_uses_the_standard_connection_cancel_code() {
        let failure = SocketProcessingFailure::new(
            crate::socket_relay::SocketProcessingFailureKind::Cancelled,
            "injected cancellation",
        )
        .in_direction(crate::socket_relay::SocketPayloadDirection::LocalExchange);
        let mapped = processing_failure(&failure);
        assert_eq!(mapped.stage, SocketRelayStage::Shutdown);
        assert_eq!(mapped.direction, Some(SocketRelayDirection::LocalExchange));
        assert_eq!(mapped.code, ErrorCode::SocketRelayCancelled.as_str());
    }
}
