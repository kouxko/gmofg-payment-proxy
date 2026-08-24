//! Socket RemoteServer/LocalServer 到统一 `Server<Socket>` 端口的实现。

use std::sync::{Arc, Mutex};

use async_trait::async_trait;
use intercept_proxy_exchange::{
    Downstream, Error, Server, ServerConnection, Socket, SocketContext, Upstream,
};
use tokio_util::sync::CancellationToken;

use crate::transport::relay::{RelayDirection, RelayProgress};

use super::super::connector::{PreparedSocketSecurity, SocketPreparationFailure};
use super::super::{SocketOpenedEvidence, SocketRelayConfig};
use super::io::{SocketConnection, SocketReader, SocketWriter};

pub(super) type SharedPreparationFailure = Arc<Mutex<Option<SocketPreparationFailure>>>;
type OpenCallback = Box<dyn FnOnce(SocketOpenedEvidence) + Send>;

pub(super) struct RemoteSocketServer {
    security: PreparedSocketSecurity,
    config: SocketRelayConfig,
    cancellation: CancellationToken,
    progress: Arc<RelayProgress>,
    read_chunk_bytes: usize,
    preparation_failure: SharedPreparationFailure,
    downstream_tls_peer: Option<String>,
    on_open: Option<OpenCallback>,
}

impl RemoteSocketServer {
    #[allow(clippy::too_many_arguments)]
    pub(super) fn new(
        security: PreparedSocketSecurity,
        config: SocketRelayConfig,
        cancellation: CancellationToken,
        progress: Arc<RelayProgress>,
        read_chunk_bytes: usize,
        preparation_failure: SharedPreparationFailure,
        downstream_tls_peer: Option<String>,
        on_open: OpenCallback,
    ) -> Self {
        Self {
            security,
            config,
            cancellation,
            progress,
            read_chunk_bytes,
            preparation_failure,
            downstream_tls_peer,
            on_open: Some(on_open),
        }
    }
}

#[async_trait]
impl Server<Socket> for RemoteSocketServer {
    async fn connect(
        &mut self,
        _first: &SocketContext,
    ) -> Result<Box<ServerConnection<Socket>>, Error> {
        let connected = self
            .security
            .connect_upstream_endpoint(
                &self.config.upstream,
                self.config.connect_timeout,
                &self.cancellation,
            )
            .await;
        let connected = match connected {
            Ok(connected) => connected,
            Err(failure) => {
                let error =
                    Error::new(format!("{}: {}", failure.error.code, failure.error.message));
                *self
                    .preparation_failure
                    .lock()
                    .unwrap_or_else(std::sync::PoisonError::into_inner) = Some(failure);
                return Err(error);
            }
        };
        if let Some(on_open) = self.on_open.take() {
            on_open(SocketOpenedEvidence::Relay {
                resolved_address: connected.resolved_address,
                downstream_tls_peer: self.downstream_tls_peer.clone(),
                upstream_tls: connected.upstream_tls,
            });
        }
        let (reader, writer) = tokio::io::split(connected.upstream);
        Ok(Box::new(SocketConnection::new(
            SocketReader::<Downstream>::new(
                Box::new(reader),
                self.read_chunk_bytes,
                self.config.read_timeout,
                self.cancellation.child_token(),
                RelayDirection::ServerToClient,
                Arc::clone(&self.progress),
            ),
            SocketWriter::<Upstream>::new(
                Box::new(writer),
                self.config.write_timeout,
                self.cancellation.child_token(),
                RelayDirection::ClientToServer,
                Arc::clone(&self.progress),
            ),
        )))
    }
}
