//! Remote connection preparation shared by raw and protocol Socket adapters.

use std::sync::{Arc, Mutex};

use intercept_proxy_exchange::{DirectionKind, Error, ErrorStage};
use tokio_util::sync::CancellationToken;

use super::connector::{PreparedSocketSecurity, SocketPreparationFailure};
use super::{SocketOpenedEvidence, SocketRelayConfig};
use crate::transport::BoxIo;

pub(super) type SharedPreparationFailure = Arc<Mutex<Option<SocketPreparationFailure>>>;
pub(super) type OpenCallback = Box<dyn FnOnce(SocketOpenedEvidence) + Send>;

pub(super) async fn connect_remote_endpoint(
    security: &PreparedSocketSecurity,
    config: &SocketRelayConfig,
    cancellation: &CancellationToken,
    preparation: &SharedPreparationFailure,
    downstream_tls_peer: Option<String>,
    on_open: &mut Option<OpenCallback>,
) -> Result<BoxIo, Error> {
    let connected = security
        .connect_upstream_endpoint(&config.upstream, config.connect_timeout, cancellation)
        .await
        .map_err(|failure| {
            let error = Error::coded(failure.error.code, failure.error.message.clone())
                .with_stage(ErrorStage::Connect)
                .with_direction(DirectionKind::Upstream);
            *preparation
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner) = Some(failure);
            error
        })?;
    if let Some(on_open) = on_open.take() {
        on_open(SocketOpenedEvidence::Relay {
            resolved_address: connected.resolved_address,
            downstream_tls_peer,
            upstream_tls: connected.upstream_tls,
        });
    }
    Ok(connected.upstream)
}
