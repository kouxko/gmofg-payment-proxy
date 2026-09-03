//! 进程内 HTTP `LocalServer` 的监听服务装配。

use std::{net::SocketAddr, sync::Arc, time::Duration};

use tokio::net::TcpListener;
use tokio_util::sync::CancellationToken;
use uuid::Uuid;

use super::exchange_runtime::LocalHttpServerConnector;
use super::{
    ChannelId, ConnectionAcceptor, ConnectionAdmission, ConnectionService,
    HttpProtocolCapabilityFactory, MessageLimits, PipelinePorts, Result, SystemClock,
};
use crate::reverse::{DownstreamTlsAcceptor, ReverseConnectionAcceptor, ReverseDownstreamTls};

#[derive(Clone, Debug)]
pub struct LocalHttpServerConfig {
    pub bind_addr: SocketAddr,
    pub downstream_tls: Option<ReverseDownstreamTls>,
    pub read_timeout: Duration,
    pub write_timeout: Duration,
}

#[derive(Clone)]
pub struct LocalHttpServerService {
    bind_addr: SocketAddr,
    connection: ConnectionService,
    channel: ChannelId,
}

impl std::fmt::Debug for LocalHttpServerService {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("LocalHttpServerService")
            .field("bind_addr", &self.bind_addr)
            .finish_non_exhaustive()
    }
}

impl LocalHttpServerService {
    pub fn build(
        config: &LocalHttpServerConfig,
        channel: ChannelId,
        ports: Arc<dyn PipelinePorts>,
        capabilities: Arc<dyn HttpProtocolCapabilityFactory>,
        limits: MessageLimits,
        maximum_connections: usize,
    ) -> Result<Self> {
        let tls = config
            .downstream_tls
            .as_ref()
            .map(DownstreamTlsAcceptor::new)
            .transpose()?;
        let acceptor: Arc<dyn ConnectionAcceptor> = Arc::new(ReverseConnectionAcceptor { tls });
        Ok(Self {
            bind_addr: config.bind_addr,
            channel,
            connection: ConnectionService {
                acceptor,
                upstream: Arc::new(LocalHttpServerConnector),
                ports,
                capabilities,
                endpoint: "local-http-server".into(),
                clock: Arc::new(SystemClock),
                admission: ConnectionAdmission::new(maximum_connections)?,
                limits,
                read_timeout: config.read_timeout,
                write_timeout: config.write_timeout,
            },
        })
    }

    pub async fn serve_listener_with_epoch(
        &self,
        listener: TcpListener,
        runtime_epoch: Uuid,
        cancellation: CancellationToken,
    ) -> Result<()> {
        self.connection
            .run_tcp_listener(listener, self.channel.clone(), runtime_epoch, cancellation)
            .await
    }
}
