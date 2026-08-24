//! Direct `LocalResponder` execution through the core `LocalRawServer`.

use std::{net::SocketAddr, sync::Arc};

use tokio_util::sync::CancellationToken;
use uuid::Uuid;

use crate::socket_relay::handler_support::{connection_identity, normalize_cancelled};
use crate::socket_relay::{
    SocketLocalResponderConfig, SocketObservationMetadata, SocketOpenedEvidence,
    SocketRelayDirection, SocketRelayFailure, SocketRelayRunContext, SocketRelayStage,
    raw_exchange,
};
use crate::transport::BoxIo;
use crate::transport::relay::{RelayBytes, RelayProgress};
use crate::{ErrorCode, ProxyError, Result};

use super::SocketConnectionHandler;

impl SocketConnectionHandler {
    #[allow(clippy::too_many_arguments)]
    pub(super) async fn handle_local_raw_responder(
        &self,
        io: BoxIo,
        peer: SocketAddr,
        connection_id: Uuid,
        run: SocketRelayRunContext,
        config: &SocketLocalResponderConfig,
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
        let metadata = SocketObservationMetadata {
            workspace_id: run.workspace_id.clone(),
            listener_id: run.listener_id.clone(),
        };
        match raw_exchange::run_local_raw_exchange(
            accepted.downstream,
            config.read_timeout,
            config.write_timeout,
            config.read_chunk_bytes,
            cancellation,
            Arc::clone(&progress),
            connection_identity(&run, connection_id, peer),
            metadata,
        )
        .await
        {
            raw_exchange::RawExchangeOutcome::Completed { bytes, .. } => {
                self.close(run, connection_id, true, bytes, None);
                Ok(())
            }
            raw_exchange::RawExchangeOutcome::Relay(failure) => {
                self.finish_direct(run, connection_id, Err(failure))
            }
            raw_exchange::RawExchangeOutcome::Exchange(error) => {
                let failure = SocketRelayFailure {
                    stage: SocketRelayStage::RelayRead,
                    direction: None,
                    code: ErrorCode::Internal.as_str(),
                };
                self.close(run, connection_id, true, progress.snapshot(), Some(failure));
                Err(ProxyError::new(ErrorCode::Internal, error.message))
            }
            raw_exchange::RawExchangeOutcome::Preparation(_) => {
                unreachable!("LocalRawServer does not prepare a remote connection")
            }
        }
    }
}
