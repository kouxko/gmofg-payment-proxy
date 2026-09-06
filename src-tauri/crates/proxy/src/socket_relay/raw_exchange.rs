//! 透明 Socket 到 `TransparentExchange` raw 端口的 transport 适配。
//!
//! 每次 Reader 只返回一次 OS read 的非空字节；Writer 原样完整 write + flush；EOF 通过
//! `finish` 半关闭传播。第一段 App 数据由核心读取后才触发这里的 `RemoteRawServer` connect。

use std::sync::{
    Arc, Mutex,
    atomic::{AtomicBool, Ordering},
};
use std::time::Duration;

use async_trait::async_trait;
use intercept_proxy_exchange::{
    Error, Exchange, ExchangeId, LocalRawServer, RawConnection, RawReader, RawServer, RawWriter,
    Socket, TransparentExchange,
};
use tokio::io::{AsyncRead, AsyncWrite};
use tokio_util::sync::CancellationToken;
use tracing::Instrument;

use crate::transport::BoxIo;
use crate::transport::relay::{RelayBytes, RelayDirection, RelayFailure, RelayProgress};

use super::connector::{PreparedSocketSecurity, SocketPreparationFailure};
use super::io as socket_io;
use super::{SocketConnectionIdentity, SocketObservationMetadata, SocketRelayConfig};

use super::remote_endpoint::{OpenCallback, SharedPreparationFailure, connect_remote_endpoint};
type SharedRelayFailure = Arc<Mutex<Option<RelayFailure>>>;

pub(super) enum RawExchangeOutcome {
    Completed { bytes: RelayBytes, opened: bool },
    Preparation(SocketPreparationFailure),
    Relay(RelayFailure),
    Exchange(Error),
}

#[allow(clippy::too_many_arguments)]
pub(super) async fn run_remote_raw_exchange(
    downstream: BoxIo,
    downstream_tls_peer: Option<String>,
    security: PreparedSocketSecurity,
    config: SocketRelayConfig,
    cancellation: CancellationToken,
    progress: Arc<RelayProgress>,
    identity: SocketConnectionIdentity,
    metadata: SocketObservationMetadata,
    on_open: OpenCallback,
) -> RawExchangeOutcome {
    let endpoint = format!("{}:{}", config.upstream.host, config.upstream.port);
    let preparation = Arc::new(Mutex::new(None));
    let relay_failure = Arc::new(Mutex::new(None));
    let opened = Arc::new(AtomicBool::new(false));
    let app = SocketRawConnection::new(
        downstream,
        config.read_timeout,
        config.write_timeout,
        config.read_chunk_bytes,
        cancellation.child_token(),
        Arc::clone(&progress),
        Arc::clone(&relay_failure),
        RelayDirection::ClientToServer,
        RelayDirection::ServerToClient,
    );
    let notify_opened = Arc::clone(&opened);
    let server = RemoteSocketRawServer {
        downstream_tls_peer,
        security,
        config,
        cancellation,
        progress: Arc::clone(&progress),
        preparation: Arc::clone(&preparation),
        relay_failure: Arc::clone(&relay_failure),
        on_open: Some(Box::new(move |evidence| {
            notify_opened.store(true, Ordering::Relaxed);
            on_open(evidence);
        })),
    };
    execute_raw_exchange(
        Box::new(app),
        Box::new(server),
        identity,
        metadata,
        &endpoint,
        progress,
        preparation,
        relay_failure,
        opened,
    )
    .await
}

#[allow(clippy::too_many_arguments)]
pub(super) async fn run_local_raw_exchange(
    downstream: BoxIo,
    read_timeout: Duration,
    write_timeout: Duration,
    read_chunk_bytes: usize,
    cancellation: CancellationToken,
    progress: Arc<RelayProgress>,
    identity: SocketConnectionIdentity,
    metadata: SocketObservationMetadata,
) -> RawExchangeOutcome {
    let relay_failure = Arc::new(Mutex::new(None));
    let app = SocketRawConnection::new(
        downstream,
        read_timeout,
        write_timeout,
        read_chunk_bytes,
        cancellation,
        Arc::clone(&progress),
        Arc::clone(&relay_failure),
        RelayDirection::ClientToServer,
        RelayDirection::ServerToClient,
    );
    execute_raw_exchange(
        Box::new(app),
        Box::new(LocalRawServer::new()),
        identity,
        metadata,
        "local-loopback",
        progress,
        Arc::new(Mutex::new(None)),
        relay_failure,
        Arc::new(AtomicBool::new(true)),
    )
    .await
}

#[allow(clippy::too_many_arguments)]
async fn execute_raw_exchange(
    app: Box<dyn RawConnection>,
    server: Box<dyn RawServer>,
    identity: SocketConnectionIdentity,
    metadata: SocketObservationMetadata,
    endpoint: &str,
    progress: Arc<RelayProgress>,
    preparation: SharedPreparationFailure,
    relay_failure: SharedRelayFailure,
    opened: Arc<AtomicBool>,
) -> RawExchangeOutcome {
    let transparent = TransparentExchange::new(app, server);
    let span = metadata.exchange_span(&identity, endpoint);
    let result = Exchange::<Socket>::transparent(exchange_id(identity.connection_id), transparent)
        .exchange()
        .instrument(span)
        .await;

    if let Some(failure) = preparation
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .take()
    {
        return RawExchangeOutcome::Preparation(failure);
    }
    if let Some(failure) = relay_failure
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .take()
    {
        return RawExchangeOutcome::Relay(failure);
    }
    match result {
        Ok(()) => RawExchangeOutcome::Completed {
            bytes: progress.snapshot(),
            opened: opened.load(Ordering::Relaxed),
        },
        Err(error) => RawExchangeOutcome::Exchange(error),
    }
}

struct RemoteSocketRawServer {
    downstream_tls_peer: Option<String>,
    security: PreparedSocketSecurity,
    config: SocketRelayConfig,
    cancellation: CancellationToken,
    progress: Arc<RelayProgress>,
    preparation: SharedPreparationFailure,
    relay_failure: SharedRelayFailure,
    on_open: Option<OpenCallback>,
}

#[async_trait]
impl RawServer for RemoteSocketRawServer {
    async fn connect(&mut self, _first_app_bytes: &[u8]) -> Result<Box<dyn RawConnection>, Error> {
        let upstream = connect_remote_endpoint(
            &self.security,
            &self.config,
            &self.cancellation,
            &self.preparation,
            self.downstream_tls_peer.clone(),
            &mut self.on_open,
        )
        .await?;
        Ok(Box::new(SocketRawConnection::new(
            upstream,
            self.config.read_timeout,
            self.config.write_timeout,
            self.config.read_chunk_bytes,
            self.cancellation.child_token(),
            Arc::clone(&self.progress),
            Arc::clone(&self.relay_failure),
            RelayDirection::ServerToClient,
            RelayDirection::ClientToServer,
        )))
    }
}

struct SocketRawConnection {
    io: BoxIo,
    read_timeout: Duration,
    write_timeout: Duration,
    read_chunk_bytes: usize,
    cancellation: CancellationToken,
    progress: Arc<RelayProgress>,
    failure: SharedRelayFailure,
    read_direction: RelayDirection,
    write_direction: RelayDirection,
}

impl SocketRawConnection {
    #[allow(clippy::too_many_arguments)]
    fn new(
        io: BoxIo,
        read_timeout: Duration,
        write_timeout: Duration,
        read_chunk_bytes: usize,
        cancellation: CancellationToken,
        progress: Arc<RelayProgress>,
        failure: SharedRelayFailure,
        read_direction: RelayDirection,
        write_direction: RelayDirection,
    ) -> Self {
        Self {
            io,
            read_timeout,
            write_timeout,
            read_chunk_bytes,
            cancellation,
            progress,
            failure,
            read_direction,
            write_direction,
        }
    }
}

impl RawConnection for SocketRawConnection {
    fn into_split(self: Box<Self>) -> (Box<dyn RawReader>, Box<dyn RawWriter>) {
        let (reader, writer) = tokio::io::split(self.io);
        (
            Box::new(SocketRawReader {
                reader,
                timeout: self.read_timeout,
                read_chunk_bytes: self.read_chunk_bytes,
                cancellation: self.cancellation.child_token(),
                progress: Arc::clone(&self.progress),
                failure: Arc::clone(&self.failure),
                direction: self.read_direction,
            }),
            Box::new(SocketRawWriter {
                writer,
                timeout: self.write_timeout,
                cancellation: self.cancellation.child_token(),
                progress: self.progress,
                failure: self.failure,
                direction: self.write_direction,
            }),
        )
    }
}

struct SocketRawReader<R> {
    reader: R,
    timeout: Duration,
    read_chunk_bytes: usize,
    cancellation: CancellationToken,
    progress: Arc<RelayProgress>,
    failure: SharedRelayFailure,
    direction: RelayDirection,
}

#[async_trait]
impl<R: AsyncRead + Send + Unpin> RawReader for SocketRawReader<R> {
    async fn read(&mut self) -> Result<Option<Vec<u8>>, Error> {
        socket_io::read(
            &mut self.reader,
            self.read_chunk_bytes,
            self.timeout,
            &self.cancellation,
            self.direction,
            &self.progress,
        )
        .await
        .map_err(|failure| self.fail(failure))
    }
}

impl<R> SocketRawReader<R> {
    fn fail(&self, failure: RelayFailure) -> Error {
        let exchange_error = socket_io::exchange_error(&failure);
        record_failure(&self.failure, failure);
        exchange_error
    }
}

struct SocketRawWriter<W> {
    writer: W,
    timeout: Duration,
    cancellation: CancellationToken,
    progress: Arc<RelayProgress>,
    failure: SharedRelayFailure,
    direction: RelayDirection,
}

#[async_trait]
impl<W: AsyncWrite + Send + Unpin> RawWriter for SocketRawWriter<W> {
    async fn write(&mut self, bytes: &[u8]) -> Result<(), Error> {
        socket_io::write_all_and_flush(
            &mut self.writer,
            bytes,
            self.timeout,
            &self.cancellation,
            self.direction,
            &self.progress,
        )
        .await
        .map_err(|failure| self.fail(failure))
    }

    async fn finish(&mut self) -> Result<(), Error> {
        socket_io::shutdown(
            &mut self.writer,
            self.timeout,
            &self.cancellation,
            self.direction,
            &self.progress,
        )
        .await
        .map_err(|failure| self.fail(failure))
    }
}

impl<W: AsyncWrite + Send + Unpin> SocketRawWriter<W> {
    fn fail(&self, failure: RelayFailure) -> Error {
        let exchange_error = socket_io::exchange_error(&failure);
        record_failure(&self.failure, failure);
        exchange_error
    }
}

fn record_failure(slot: &SharedRelayFailure, failure: RelayFailure) {
    let mut slot = slot
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    if slot.is_none() {
        *slot = Some(failure);
    }
}

fn exchange_id(id: uuid::Uuid) -> ExchangeId {
    ExchangeId::new(id.as_u128())
}

#[cfg(test)]
mod tests;
