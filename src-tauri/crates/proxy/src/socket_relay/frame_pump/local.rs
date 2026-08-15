//! `LocalResponder` processor 构造、诊断注入与单向 request-response pump。

use std::sync::Arc;

use tokio_util::sync::CancellationToken;

use crate::transport::{
    BoxIo,
    relay::{RelayBytes, RelayProgress},
};

use super::{SocketFramePumpTimeouts, control::create_local_processor, pump_direction};
use crate::socket_relay::{
    LocalResponderDiagnostics, LocalResponderProcessorFactory, SocketConnectionIdentity,
    SocketFrameProcessor, SocketFramePumpLimits, SocketPayloadDirection, SocketProcessingFailure,
};

#[cfg(test)]
pub(crate) async fn respond_framed_locally(
    app: BoxIo,
    connection: SocketConnectionIdentity,
    factory: &dyn LocalResponderProcessorFactory,
    limits: SocketFramePumpLimits,
    timeouts: SocketFramePumpTimeouts,
    cancellation: CancellationToken,
    progress: Arc<RelayProgress>,
) -> Result<RelayBytes, SocketProcessingFailure> {
    let processor = create_local_processor(factory, connection)?;
    respond_with_processor(app, processor, limits, timeouts, cancellation, progress).await
}

#[allow(clippy::too_many_arguments)]
pub(crate) async fn respond_framed_locally_observed(
    app: BoxIo,
    connection: SocketConnectionIdentity,
    factory: &dyn LocalResponderProcessorFactory,
    diagnostics: LocalResponderDiagnostics,
    limits: SocketFramePumpLimits,
    timeouts: SocketFramePumpTimeouts,
    cancellation: CancellationToken,
    progress: Arc<RelayProgress>,
) -> Result<RelayBytes, SocketProcessingFailure> {
    let mut processor = create_local_processor(factory, connection)?;
    processor.set_local_diagnostics(diagnostics);
    respond_with_processor(app, processor, limits, timeouts, cancellation, progress).await
}

async fn respond_with_processor(
    app: BoxIo,
    processor: Box<dyn SocketFrameProcessor>,
    limits: SocketFramePumpLimits,
    timeouts: SocketFramePumpTimeouts,
    cancellation: CancellationToken,
    progress: Arc<RelayProgress>,
) -> Result<RelayBytes, SocketProcessingFailure> {
    let (reader, writer) = tokio::io::split(app);
    let result = pump_direction(
        reader,
        writer,
        processor,
        SocketPayloadDirection::LocalExchange,
        limits,
        timeouts,
        cancellation,
        Arc::clone(&progress),
    )
    .await;
    let bytes = progress.snapshot();
    result
        .map(|()| bytes)
        .map_err(|failure| failure.with_bytes(bytes))
}
