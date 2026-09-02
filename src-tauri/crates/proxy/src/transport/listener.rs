use std::{
    fmt::Debug,
    io,
    net::SocketAddr,
    sync::Arc,
    time::{Duration, SystemTime},
};

use async_trait::async_trait;
use tokio::net::TcpListener;

use crate::{
    Result,
    transport::{BoxIo, ConnectionContext},
};

#[async_trait]
pub trait ConnectionAcceptor: Debug + Send + Sync {
    async fn accept(&self, io: BoxIo, context: &ConnectionContext) -> Result<AcceptedConnection>;
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TlsPeerIdentity {
    pub sha256_fingerprint: String,
    pub subject_summary: String,
}

pub struct AcceptedConnection {
    pub io: BoxIo,
    pub tls_peer: Option<TlsPeerIdentity>,
}

impl Debug for AcceptedConnection {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("AcceptedConnection")
            .field("tls_peer", &self.tls_peer)
            .finish_non_exhaustive()
    }
}

#[async_trait]
pub trait BoundListener: Debug + Send + Sync {
    fn local_addr(&self) -> io::Result<SocketAddr>;
    async fn accept(&self) -> io::Result<(BoxIo, SocketAddr)>;
}

#[async_trait]
pub trait ListenerBinder: Debug + Send + Sync {
    async fn bind(&self, address: SocketAddr) -> io::Result<Arc<dyn BoundListener>>;
}

#[derive(Debug, Default)]
pub struct TokioListenerBinder;

#[async_trait]
impl ListenerBinder for TokioListenerBinder {
    async fn bind(&self, address: SocketAddr) -> io::Result<Arc<dyn BoundListener>> {
        Ok(Arc::new(TokioBoundListener(
            TcpListener::bind(address).await?,
        )))
    }
}

#[derive(Debug)]
pub(crate) struct TokioBoundListener(pub(crate) TcpListener);

#[async_trait]
impl BoundListener for TokioBoundListener {
    fn local_addr(&self) -> io::Result<SocketAddr> {
        self.0.local_addr()
    }

    async fn accept(&self) -> io::Result<(BoxIo, SocketAddr)> {
        let (stream, address) = self.0.accept().await?;
        stream.set_nodelay(true)?;
        Ok((Box::new(stream), address))
    }
}

#[async_trait]
pub trait Clock: Debug + Send + Sync {
    fn now(&self) -> SystemTime;
    async fn sleep(&self, duration: Duration);
}

#[derive(Debug, Default)]
pub struct SystemClock;

#[async_trait]
impl Clock for SystemClock {
    fn now(&self) -> SystemTime {
        SystemTime::now()
    }

    async fn sleep(&self, duration: Duration) {
        tokio::time::sleep(duration).await;
    }
}
