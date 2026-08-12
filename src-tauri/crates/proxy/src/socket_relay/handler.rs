use std::{net::SocketAddr, sync::Arc, time::SystemTime};

use async_trait::async_trait;
use tokio_util::sync::CancellationToken;
use uuid::Uuid;

use crate::listener::{ConnectionHandler, ConnectionTaskScope, PrimaryConnectionOutcome, sealed};
use crate::transport::BoxIo;
use crate::transport::ConnectionContext;
use crate::transport::relay::{
    RelayBytes, RelayFailure, RelayOperation, RelayProgress, RelayTimeoutCodes, RelayTimeouts,
    relay_bidirectional_with_progress,
};
use crate::{ErrorCode, ProxyError, Result};

use super::connector::PreparedSocketSecurity;
use super::{
    SocketConnectionEvent, SocketConnectionObserver, SocketRelayBytes, SocketRelayConfig,
    SocketRelayDirection, SocketRelayFailure, SocketRelayMetrics, SocketRelayRunContext,
    SocketRelayStage, SocketUpstreamConnectionTestResult,
};

#[derive(Debug)]
pub(crate) struct SocketConnectionHandler {
    config: SocketRelayConfig,
    security: PreparedSocketSecurity,
    observer: Arc<dyn SocketConnectionObserver>,
    metrics: Arc<SocketRelayMetrics>,
    run: Arc<std::sync::RwLock<SocketRelayRunContext>>,
}

impl SocketConnectionHandler {
    pub(crate) fn build(
        config: SocketRelayConfig,
        observer: Arc<dyn SocketConnectionObserver>,
        metrics: Arc<SocketRelayMetrics>,
        run: Arc<std::sync::RwLock<SocketRelayRunContext>>,
    ) -> Result<Self> {
        config.validate()?;
        let security = PreparedSocketSecurity::build(&config.security)?;
        Ok(Self {
            config,
            security,
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
        let target = format!(
            "{}:{}",
            self.config.upstream.host, self.config.upstream.port
        );
        let run = self.run();
        let progress = Arc::new(RelayProgress::default());
        self.metrics.admitted(connection_id, Arc::clone(&progress));
        self.observer.record(SocketConnectionEvent::Admitted {
            run: run.clone(),
            connection_id,
            peer,
            target,
            mode: self.security.mode(),
            at: SystemTime::now(),
        });

        let connected = self
            .security
            .connect(
                io,
                peer,
                &self.config.upstream,
                self.config.connect_timeout,
                &cancellation,
            )
            .await;
        let connected = match connected {
            Ok(connected) => connected,
            Err(preparation) => {
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
                return Err(error);
            }
        };

        self.metrics.opened(connection_id);
        self.observer.record(SocketConnectionEvent::Opened {
            run: run.clone(),
            connection_id,
            resolved_address: connected.resolved_address,
            downstream_tls_peer: connected.downstream_tls_peer,
            upstream_tls: connected.upstream_tls,
            at: SystemTime::now(),
        });

        let relayed = relay_bidirectional_with_progress(
            connected.downstream,
            connected.upstream,
            RelayTimeouts::new(
                self.config.read_timeout,
                self.config.write_timeout,
                RelayTimeoutCodes {
                    read: ErrorCode::SocketReadTimeout,
                    write: ErrorCode::SocketWriteTimeout,
                },
            ),
            cancellation,
            progress,
        )
        .await;
        match relayed {
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
            opened,
            bytes: SocketRelayBytes::from(bytes),
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
        self.security
            .test_upstream(
                &self.config.upstream,
                self.config.connect_timeout,
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

fn socket_failure(failure: &RelayFailure) -> SocketRelayFailure {
    let direction = match failure.direction {
        crate::transport::relay::RelayDirection::ClientToServer => {
            SocketRelayDirection::ClientToServer
        }
        crate::transport::relay::RelayDirection::ServerToClient => {
            SocketRelayDirection::ServerToClient
        }
    };
    if failure.error.code == ErrorCode::ProxyStopped.as_str() {
        return SocketRelayFailure {
            stage: SocketRelayStage::Shutdown,
            direction: Some(direction),
            code: ErrorCode::SocketRelayCancelled.as_str(),
        };
    }
    let (stage, code) = match failure.operation {
        RelayOperation::Read => (
            SocketRelayStage::RelayRead,
            if failure.error.code == ErrorCode::SocketReadTimeout.as_str() {
                ErrorCode::SocketReadTimeout.as_str()
            } else {
                ErrorCode::SocketReadFailed.as_str()
            },
        ),
        RelayOperation::Write | RelayOperation::Flush | RelayOperation::HalfClose => (
            SocketRelayStage::RelayWrite,
            if failure.error.code == ErrorCode::SocketWriteTimeout.as_str() {
                ErrorCode::SocketWriteTimeout.as_str()
            } else {
                ErrorCode::SocketWriteFailed.as_str()
            },
        ),
    };
    SocketRelayFailure {
        stage,
        direction: Some(direction),
        code,
    }
}
fn normalize_cancelled(error: ProxyError) -> ProxyError {
    if error.code == ErrorCode::ProxyStopped.as_str() {
        ProxyError::new(ErrorCode::SocketRelayCancelled, error.message)
    } else {
        error
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
}
