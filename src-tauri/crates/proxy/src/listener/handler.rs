use std::{fmt::Debug, net::SocketAddr, sync::Arc};

use async_trait::async_trait;
use tokio_util::sync::CancellationToken;

use crate::transport::{BoxIo, ConnectionContext};

use super::{ConnectionTaskScope, PrimaryConnectionOutcome, TerminalConnectionOutcome};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ListenerRejection {
    CapacityExhausted,
}

pub(crate) mod sealed {
    pub trait Sealed {}
}

#[async_trait]
pub(crate) trait ConnectionHandler: sealed::Sealed + Debug + Send + Sync {
    async fn handle(
        &self,
        io: BoxIo,
        context: ConnectionContext,
        child_tasks: ConnectionTaskScope,
        cancellation: CancellationToken,
    ) -> PrimaryConnectionOutcome;
}

pub(crate) trait ConnectionLifecycleObserver: Debug + Send + Sync {
    fn connection_rejected(&self, _peer_addr: SocketAddr, _reason: ListenerRejection) {}

    fn connection_admitted(&self, _context: &ConnectionContext) {}

    fn connection_terminal(
        &self,
        _context: &ConnectionContext,
        _outcome: &TerminalConnectionOutcome,
    ) {
    }
}

pub(crate) type SharedConnectionObserver = Arc<dyn ConnectionLifecycleObserver>;
