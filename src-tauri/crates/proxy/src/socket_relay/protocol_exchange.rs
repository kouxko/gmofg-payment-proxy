//! Scripted Socket 的生产 `Exchange<Socket>` 装配。
//!
//! 每条 App connection 创建一个 Exchange；Exchange 直接持有 upstream/downstream 两个
//! Pipeline，并严格推动 request 写往固定 Endpoint、读取唯一 response、写回 App。

use std::{panic::AssertUnwindSafe, sync::Arc};

use intercept_proxy_exchange::{
    Direction, Downstream, Error, Exchange, ExchangeId, LocalSocketServer, Pipeline,
    ProtocolExchange, ServerSlot, Socket, SocketRead, Upstream, Write,
};
use tokio_util::sync::CancellationToken;
use tracing::Instrument;

use crate::transport::BoxIo;
use crate::transport::relay::{RelayBytes, RelayDirection, RelayProgress};

use self::capabilities::{BoundedEncode, BoundedFrame, factory_failed, factory_panicked};
use self::io::{SocketConnection, SocketReader, SocketWriter};
use self::server::{RemoteSocketServer, SharedPreparationFailure};
use super::connector::{PreparedSocketSecurity, SocketPreparationFailure};
use super::{
    SocketConnectionIdentity, SocketDirectionCapabilities, SocketOpenedEvidence,
    SocketPayloadDirection, SocketPipelineLimits, SocketProcessingFailure,
    SocketProcessingFailureKind, SocketProtocolCapabilityFactory, SocketRelayConfig,
};

mod capabilities;
mod io;
mod server;

pub(super) enum ProtocolExchangeFailure {
    Preparation(SocketPreparationFailure),
    Processing(SocketProcessingFailure),
}

#[allow(clippy::too_many_arguments)]
pub(super) async fn run_scripted_exchange(
    downstream: BoxIo,
    downstream_tls_peer: Option<String>,
    security: PreparedSocketSecurity,
    config: SocketRelayConfig,
    identity: SocketConnectionIdentity,
    factory: &dyn SocketProtocolCapabilityFactory,
    limits: SocketPipelineLimits,
    cancellation: CancellationToken,
    progress: Arc<RelayProgress>,
    on_open: Box<dyn FnOnce(SocketOpenedEvidence) + Send>,
) -> Result<RelayBytes, ProtocolExchangeFailure> {
    let endpoint = format!("{}:{}", config.upstream.host, config.upstream.port);
    let preparation_failure: SharedPreparationFailure = Arc::new(std::sync::Mutex::new(None));
    let span = observation_span(factory, &identity, &endpoint);
    match Exchange::<Socket>::protocol_with(exchange_id(identity.connection_id), || {
        let upstream = create_upstream(factory, identity.clone())?;
        let downstream_capabilities = create_downstream(factory, identity.clone())?;
        let app = app_connection(
            downstream,
            limits,
            config.read_timeout,
            config.write_timeout,
            &cancellation,
            Arc::clone(&progress),
        );
        let server = RemoteSocketServer::new(
            security,
            config,
            cancellation.clone(),
            Arc::clone(&progress),
            limits.read_chunk_bytes(),
            Arc::clone(&preparation_failure),
            downstream_tls_peer,
            on_open,
        );
        Ok(ProtocolExchange::new(
            Box::new(app),
            ServerSlot::new(Box::new(server)),
            pipeline(upstream, limits),
            pipeline(downstream_capabilities, limits),
        ))
    })
    .instrument(span)
    .await
    {
        Ok(()) => Ok(progress.snapshot()),
        Err(error) => {
            if let Some(failure) = preparation_failure
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner)
                .take()
            {
                Err(ProtocolExchangeFailure::Preparation(failure))
            } else {
                Err(ProtocolExchangeFailure::Processing(
                    exchange_failure(&error).with_bytes(progress.snapshot()),
                ))
            }
        }
    }
}

#[allow(clippy::too_many_arguments)]
pub(super) async fn run_local_exchange(
    downstream: BoxIo,
    identity: SocketConnectionIdentity,
    factory: &dyn SocketProtocolCapabilityFactory,
    limits: SocketPipelineLimits,
    read_timeout: std::time::Duration,
    write_timeout: std::time::Duration,
    cancellation: CancellationToken,
    progress: Arc<RelayProgress>,
) -> Result<RelayBytes, ProtocolExchangeFailure> {
    let span = observation_span(factory, &identity, "local-loopback");
    Exchange::<Socket>::protocol_with(exchange_id(identity.connection_id), || {
        let upstream = create_upstream(factory, identity.clone())?;
        let downstream_capabilities = create_downstream(factory, identity.clone())?;
        let app = app_connection(
            downstream,
            limits,
            read_timeout,
            write_timeout,
            &cancellation,
            Arc::clone(&progress),
        );
        Ok(ProtocolExchange::new(
            Box::new(app),
            ServerSlot::new(Box::new(LocalSocketServer::new())),
            pipeline(upstream, limits),
            pipeline(downstream_capabilities, limits),
        ))
    })
    .instrument(span)
    .await
    .map(|()| progress.snapshot())
    .map_err(|error| {
        ProtocolExchangeFailure::Processing(
            exchange_failure(&error).with_bytes(progress.snapshot()),
        )
    })
}

fn app_connection(
    downstream: BoxIo,
    limits: SocketPipelineLimits,
    read_timeout: std::time::Duration,
    write_timeout: std::time::Duration,
    cancellation: &CancellationToken,
    progress: Arc<RelayProgress>,
) -> SocketConnection<Upstream, Downstream> {
    let (reader, writer) = tokio::io::split(downstream);
    SocketConnection::new(
        SocketReader::new(
            Box::new(reader),
            limits.read_chunk_bytes(),
            read_timeout,
            cancellation.child_token(),
            RelayDirection::ClientToServer,
            Arc::clone(&progress),
        ),
        SocketWriter::new(
            Box::new(writer),
            write_timeout,
            cancellation.child_token(),
            RelayDirection::ServerToClient,
            progress,
        ),
    )
}

fn pipeline<D: intercept_proxy_exchange::Direction>(
    capabilities: SocketDirectionCapabilities<D>,
    limits: SocketPipelineLimits,
) -> Pipeline<Socket, D> {
    Pipeline::new(
        Box::new(SocketRead::new(
            Box::new(BoundedFrame::new(
                capabilities.frame,
                limits.max_buffer_bytes(),
            )),
            capabilities.decode,
            capabilities.display,
        )),
        Box::new(Write::new(
            capabilities.rules,
            Box::new(BoundedEncode::new(
                capabilities.encode,
                limits.max_output_bytes(),
            )),
        )),
    )
}

fn create_upstream(
    factory: &dyn SocketProtocolCapabilityFactory,
    identity: SocketConnectionIdentity,
) -> Result<SocketDirectionCapabilities<Upstream>, Error> {
    create_capabilities(|| factory.create_upstream(identity))
}

fn create_downstream(
    factory: &dyn SocketProtocolCapabilityFactory,
    identity: SocketConnectionIdentity,
) -> Result<SocketDirectionCapabilities<Downstream>, Error> {
    create_capabilities(|| factory.create_downstream(identity))
}

fn create_capabilities<D: Direction>(
    create: impl FnOnce() -> Result<SocketDirectionCapabilities<D>, SocketProcessingFailure>,
) -> Result<SocketDirectionCapabilities<D>, Error> {
    match std::panic::catch_unwind(AssertUnwindSafe(create)) {
        Err(_) => Err(direction_error::<D>(&factory_panicked())),
        Ok(Err(error)) => Err(direction_error::<D>(&factory_failed(&error))),
        Ok(Ok(capabilities)) => Ok(capabilities),
    }
}

fn direction_error<D: Direction>(error: &Error) -> Error {
    let direction = match D::KIND {
        intercept_proxy_exchange::DirectionKind::Upstream => "Upstream",
        intercept_proxy_exchange::DirectionKind::Downstream => "Downstream",
    };
    let mut mapped = Error::new(format!("{direction}|{}", error.message));
    mapped
        .external_package_call
        .clone_from(&error.external_package_call);
    mapped
}

fn exchange_failure(error: &Error) -> SocketProcessingFailure {
    let message = error.message.as_str();
    let direction = if message.starts_with("Upstream|") {
        Some(SocketPayloadDirection::AppToUpstream)
    } else if message.starts_with("Downstream|") {
        Some(SocketPayloadDirection::UpstreamToApp)
    } else {
        None
    };
    let code = message
        .split_once('|')
        .map_or(message, |(_, rest)| rest)
        .split_once(':')
        .map_or(message, |(code, _)| code);
    let mut failure = SocketProcessingFailure::new(kind_from_code(code), message);
    if let Some(external_package_call) = error.external_package_call.clone() {
        failure = failure.with_external_package_call(*external_package_call);
    }
    direction.map_or(failure.clone(), |direction| failure.in_direction(direction))
}

fn kind_from_code(code: &str) -> SocketProcessingFailureKind {
    match code {
        "CANCELLED" => SocketProcessingFailureKind::Cancelled,
        "READ_TIMEOUT" => SocketProcessingFailureKind::ReadTimeout,
        "READ_FAILED" => SocketProcessingFailureKind::ReadFailed,
        "WRITE_TIMEOUT" => SocketProcessingFailureKind::WriteTimeout,
        "WRITE_FAILED" => SocketProcessingFailureKind::WriteFailed,
        "PROCESSOR_PANICKED" => SocketProcessingFailureKind::ProcessorPanicked,
        "PROCESSING_TIMEOUT" => SocketProcessingFailureKind::ProcessingTimeout,
        "BUFFER_LIMIT_EXCEEDED" => SocketProcessingFailureKind::BufferLimitExceeded,
        "TRUNCATED_FRAME" => SocketProcessingFailureKind::TruncatedFrame,
        "INVALID_FRAME_BOUNDARY" => SocketProcessingFailureKind::InvalidFrameBoundary,
        "EMPTY_OUTPUT" => SocketProcessingFailureKind::EmptyOutput,
        "OUTPUT_LIMIT_EXCEEDED" => SocketProcessingFailureKind::OutputLimitExceeded,
        "FRAME_REJECTED" => SocketProcessingFailureKind::FrameRejected,
        "DECODE_FAILED" => SocketProcessingFailureKind::DecodeFailed,
        "RULE_FAILED" => SocketProcessingFailureKind::RuleFailed,
        "ENCODE_FAILED" => SocketProcessingFailureKind::EncodeFailed,
        _ => SocketProcessingFailureKind::ProcessingFailed,
    }
}

fn exchange_id(id: uuid::Uuid) -> ExchangeId {
    ExchangeId::new(id.as_u128())
}

fn observation_span(
    factory: &dyn SocketProtocolCapabilityFactory,
    identity: &SocketConnectionIdentity,
    endpoint: &str,
) -> tracing::Span {
    let metadata = factory.observation_metadata();
    metadata.exchange_span(identity, endpoint)
}
