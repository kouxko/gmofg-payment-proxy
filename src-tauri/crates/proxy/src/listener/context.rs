use std::{net::SocketAddr, sync::Arc};

use uuid::Uuid;

use crate::{
    supervisor::ChannelId,
    transport::{Clock, ConnectionContext},
};

#[derive(Debug, Clone)]
pub(crate) struct ListenerRunContext {
    runtime_epoch: Uuid,
    listener_id: ChannelId,
    clock: Arc<dyn Clock>,
}

impl ListenerRunContext {
    pub(crate) fn new(runtime_epoch: Uuid, listener_id: ChannelId, clock: Arc<dyn Clock>) -> Self {
        Self {
            runtime_epoch,
            listener_id,
            clock,
        }
    }

    pub(crate) fn connection(&self, peer_addr: SocketAddr) -> ConnectionContext {
        ConnectionContext {
            runtime_epoch: self.runtime_epoch,
            connection_id: Uuid::new_v4(),
            channel: self.listener_id.clone(),
            peer_addr,
            accepted_at: self.clock.now(),
            tls_peer: None,
        }
    }
}
