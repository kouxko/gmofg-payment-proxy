//! `RemoteServer` 的 raw 与 protocol Exchange 分派。

use std::{net::SocketAddr, sync::Arc, time::SystemTime};

use tokio_util::sync::CancellationToken;
use uuid::Uuid;

use crate::transport::BoxIo;
use crate::transport::relay::{RelayBytes, RelayProgress};
use crate::{ErrorCode, ProxyError, Result};

use super::SocketConnectionHandler;
use crate::socket_relay::handler_support::{SocketHandlerProcessing, connection_identity};
use crate::socket_relay::protocol_exchange::{ProtocolExchangeFailure, run_scripted_exchange};
use crate::socket_relay::raw_exchange::{RawExchangeOutcome, run_remote_raw_exchange};
use crate::socket_relay::{
    SocketConnectionEvent, SocketObservationMetadata, SocketPipelineLimits,
    SocketProtocolCapabilityFactory, SocketRelayConfig, SocketRelayDirection, SocketRelayFailure,
    SocketRelayRunContext, SocketRelayStage,
};

impl SocketConnectionHandler {
    #[allow(clippy::too_many_arguments)]
    pub(super) async fn handle_relay(
        &self,
        io: BoxIo,
        peer: SocketAddr,
        connection_id: Uuid,
        run: SocketRelayRunContext,
        config: &SocketRelayConfig,
        processing: &SocketHandlerProcessing,
        cancellation: CancellationToken,
        progress: Arc<RelayProgress>,
    ) -> Result<()> {
        match processing {
            SocketHandlerProcessing::Direct => {
                self.handle_raw_exchange(
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
            SocketHandlerProcessing::ScriptedRelay { factory, limits } => {
                self.handle_protocol_exchange(
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
            SocketHandlerProcessing::LocalResponder { .. } => {
                unreachable!("relay config cannot carry a local responder processor")
            }
            SocketHandlerProcessing::DirectLocal => {
                unreachable!("relay config cannot carry a local raw processor")
            }
        }
    }

    #[allow(clippy::too_many_arguments)]
    async fn handle_protocol_exchange(
        &self,
        io: BoxIo,
        peer: SocketAddr,
        connection_id: Uuid,
        run: SocketRelayRunContext,
        config: &SocketRelayConfig,
        factory: &dyn SocketProtocolCapabilityFactory,
        limits: SocketPipelineLimits,
        cancellation: CancellationToken,
        progress: Arc<RelayProgress>,
    ) -> Result<()> {
        let accepted = self
            .security
            .accept_downstream(io, peer, config.connect_timeout, &cancellation)
            .await;
        let accepted = match accepted {
            Ok(accepted) => accepted,
            Err(error) => {
                return self.finish_preparation_failure(
                    run,
                    connection_id,
                    super::super::connector::SocketPreparationFailure {
                        error,
                        failure: downstream_tls_failure(),
                    },
                );
            }
        };
        let observer = Arc::clone(&self.observer);
        let metrics = Arc::clone(&self.metrics);
        let open_run = run.clone();
        let on_open = Box::new(move |evidence| {
            metrics.opened(connection_id);
            observer.record(SocketConnectionEvent::Opened {
                run: open_run,
                connection_id,
                evidence,
                at: SystemTime::now(),
            });
        });
        let result = run_scripted_exchange(
            accepted.downstream,
            accepted.downstream_tls_peer,
            self.security.clone(),
            config.clone(),
            connection_identity(&run, connection_id, peer),
            factory,
            limits,
            cancellation,
            progress,
            on_open,
        )
        .await;
        match result {
            Ok(bytes) => {
                self.close(run, connection_id, true, bytes, None);
                Ok(())
            }
            Err(ProtocolExchangeFailure::Preparation(failure)) => {
                self.finish_preparation_failure(run, connection_id, failure)
            }
            Err(ProtocolExchangeFailure::Processing(failure)) => {
                self.finish_processing(run, connection_id, Err(failure))
            }
        }
    }

    #[allow(clippy::too_many_arguments)]
    async fn handle_raw_exchange(
        &self,
        io: BoxIo,
        peer: SocketAddr,
        connection_id: Uuid,
        run: SocketRelayRunContext,
        config: &SocketRelayConfig,
        cancellation: CancellationToken,
        progress: Arc<RelayProgress>,
    ) -> Result<()> {
        let accepted = self
            .security
            .accept_downstream(io, peer, config.connect_timeout, &cancellation)
            .await;
        let accepted = match accepted {
            Ok(accepted) => accepted,
            Err(error) => {
                return self.finish_preparation_failure(
                    run,
                    connection_id,
                    super::super::connector::SocketPreparationFailure {
                        error,
                        failure: downstream_tls_failure(),
                    },
                );
            }
        };
        let observer = Arc::clone(&self.observer);
        let metrics = Arc::clone(&self.metrics);
        let open_run = run.clone();
        let on_open = Box::new(move |evidence| {
            metrics.opened(connection_id);
            observer.record(SocketConnectionEvent::Opened {
                run: open_run,
                connection_id,
                evidence,
                at: SystemTime::now(),
            });
        });
        let identity = connection_identity(&run, connection_id, peer);
        let metadata = SocketObservationMetadata {
            workspace_id: run.workspace_id.clone(),
            listener_id: run.listener_id.clone(),
        };
        let outcome = run_remote_raw_exchange(
            accepted.downstream,
            accepted.downstream_tls_peer,
            self.security.clone(),
            config.clone(),
            cancellation,
            progress,
            identity,
            metadata,
            on_open,
        )
        .await;
        match outcome {
            RawExchangeOutcome::Completed { bytes, opened } => {
                self.close(run, connection_id, opened, bytes, None);
                Ok(())
            }
            RawExchangeOutcome::Preparation(failure) => {
                self.finish_preparation_failure(run, connection_id, failure)
            }
            RawExchangeOutcome::Relay(failure) => {
                self.finish_direct(run, connection_id, Err(failure))
            }
            RawExchangeOutcome::Exchange(error) => {
                let failure = SocketRelayFailure {
                    stage: SocketRelayStage::RelayRead,
                    direction: None,
                    code: ErrorCode::Internal.as_str(),
                };
                self.close(
                    run,
                    connection_id,
                    false,
                    RelayBytes::default(),
                    Some(failure),
                );
                Err(ProxyError::new(ErrorCode::Internal, error.message))
            }
        }
    }
}

fn downstream_tls_failure() -> SocketRelayFailure {
    SocketRelayFailure {
        stage: SocketRelayStage::DownstreamTls,
        direction: Some(SocketRelayDirection::Downstream),
        code: ErrorCode::SocketDownstreamTlsFailed.as_str(),
    }
}
