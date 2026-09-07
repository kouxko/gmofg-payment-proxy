mod protocol;
mod udp;

#[cfg(test)]
mod tests;

use std::{io, net::SocketAddr, os::fd::AsRawFd, sync::Arc, time::Duration};

use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::{TcpListener, TcpSocket, TcpStream},
    sync::Semaphore,
    task::JoinSet,
    time::{Instant, sleep, timeout},
};
use tun2proxy::CancellationToken;

use self::protocol::Target;
use crate::data_plane::{
    SocketProtection, record_socket_protection_failure, record_socks_client,
    record_socks_connect_attempt, record_socks_connect_success,
};
use crate::routing::ProxyRouteTable;

const MAX_CONCURRENT_CLIENTS: usize = 128;
const HANDSHAKE_TIMEOUT: Duration = Duration::from_secs(10);
const CONNECT_TIMEOUT: Duration = Duration::from_secs(15);
const IDLE_TIMEOUT: Duration = Duration::from_mins(5);

#[derive(Clone, Copy, Debug)]
struct ServerLimits {
    max_clients: usize,
    handshake_timeout: Duration,
    connect_timeout: Duration,
    idle_timeout: Duration,
}

impl Default for ServerLimits {
    fn default() -> Self {
        Self {
            max_clients: MAX_CONCURRENT_CLIENTS,
            handshake_timeout: HANDSHAKE_TIMEOUT,
            connect_timeout: CONNECT_TIMEOUT,
            idle_timeout: IDLE_TIMEOUT,
        }
    }
}

pub(super) async fn run_server<P>(
    listener: TcpListener,
    protector: P,
    proxy_routes: Arc<ProxyRouteTable>,
    cancellation: CancellationToken,
    runtime_epoch: u64,
) -> io::Result<()>
where
    P: SocketProtection,
{
    run_server_with_limits(
        listener,
        protector,
        proxy_routes,
        cancellation,
        runtime_epoch,
        ServerLimits::default(),
    )
    .await
}

async fn run_server_with_limits<P>(
    listener: TcpListener,
    protector: P,
    proxy_routes: Arc<ProxyRouteTable>,
    cancellation: CancellationToken,
    runtime_epoch: u64,
    limits: ServerLimits,
) -> io::Result<()>
where
    P: SocketProtection,
{
    let permits = Arc::new(Semaphore::new(limits.max_clients.max(1)));
    let mut clients = JoinSet::new();

    loop {
        reap_finished_clients(&mut clients);
        let permit = tokio::select! {
            () = cancellation.cancelled() => break,
            acquired = permits.clone().acquire_owned() => acquired.map_err(|_| {
                io::Error::other("SOCKS5 并发控制器已关闭")
            })?,
        };
        let accepted = tokio::select! {
            () = cancellation.cancelled() => break,
            accepted = listener.accept() => accepted,
        };
        let (stream, _) = accepted?;
        record_socks_client(runtime_epoch);
        let protector = protector.clone();
        let proxy_routes = proxy_routes.clone();
        let session_cancellation = cancellation.child_token();
        clients.spawn(async move {
            let _permit = permit;
            // 单个目标不可达、远端拒绝连接或客户端主动断开只是该会话的结果，不能
            // 标记成整个 VPN 数据面故障。统计可通过 attempts 与 successes 观察。
            let _ = handle_client(
                stream,
                protector,
                proxy_routes,
                session_cancellation,
                runtime_epoch,
                limits,
            )
            .await;
        });
    }

    cancellation.cancel();
    clients.abort_all();
    while clients.join_next().await.is_some() {}
    Ok(())
}

fn reap_finished_clients(clients: &mut JoinSet<()>) {
    while clients.try_join_next().is_some() {}
}

async fn handle_client<P>(
    mut client: TcpStream,
    protector: P,
    proxy_routes: Arc<ProxyRouteTable>,
    cancellation: CancellationToken,
    runtime_epoch: u64,
    limits: ServerLimits,
) -> io::Result<()>
where
    P: SocketProtection,
{
    let (command, target) = tokio::select! {
        () = cancellation.cancelled() => return Ok(()),
        result = timeout(limits.handshake_timeout, protocol::negotiate(&mut client)) => {
            result.map_err(|_| timed_out("SOCKS5 握手超时"))??
        }
    };

    match command {
        1 => {
            handle_connect(
                client,
                target,
                protector,
                &proxy_routes,
                cancellation,
                runtime_epoch,
                limits,
            )
            .await
        }
        3 => udp::associate(client, protector, cancellation).await,
        _ => protocol::write_reply(&mut client, protocol::COMMAND_NOT_SUPPORTED, None).await,
    }
}

async fn handle_connect<P>(
    mut client: TcpStream,
    target: Target,
    protector: P,
    proxy_routes: &ProxyRouteTable,
    cancellation: CancellationToken,
    runtime_epoch: u64,
    limits: ServerLimits,
) -> io::Result<()>
where
    P: SocketProtection,
{
    let addresses = if let Some(addresses) = target.proxy_addresses(proxy_routes) {
        addresses.to_vec()
    } else {
        tokio::select! {
            () = cancellation.cancelled() => return Ok(()),
            result = timeout(limits.connect_timeout, target.resolve()) => {
                result.map_err(|_| timed_out("SOCKS5 DNS 解析超时"))??
            }
        }
    };

    let mut last_error = None;
    for address in addresses {
        record_socks_connect_attempt(runtime_epoch);
        let result = tokio::select! {
            () = cancellation.cancelled() => return Ok(()),
            result = timeout(
                limits.connect_timeout,
                connect_protected(address, &protector, runtime_epoch),
            ) => result.map_err(|_| timed_out("SOCKS5 连接上游超时"))?,
        };
        match result {
            Ok(upstream) => {
                record_socks_connect_success(runtime_epoch);
                protocol::write_reply(&mut client, protocol::SUCCEEDED, upstream.local_addr().ok())
                    .await?;
                return relay_with_idle_timeout(
                    client,
                    upstream,
                    cancellation,
                    limits.idle_timeout,
                )
                .await;
            }
            Err(error) => last_error = Some(error),
        }
    }
    protocol::write_reply(&mut client, protocol::GENERAL_FAILURE, None).await?;
    Err(last_error.unwrap_or_else(|| io::Error::other("SOCKS5 目标无法解析")))
}

async fn connect_protected(
    address: SocketAddr,
    protector: &impl SocketProtection,
    runtime_epoch: u64,
) -> io::Result<TcpStream> {
    let socket = if address.is_ipv4() {
        TcpSocket::new_v4()?
    } else {
        TcpSocket::new_v6()?
    };
    if let Err(error) = protector.protect(socket.as_raw_fd()) {
        record_socket_protection_failure(runtime_epoch);
        return Err(error);
    }
    socket.connect(address).await
}

async fn relay_with_idle_timeout(
    mut client: TcpStream,
    mut upstream: TcpStream,
    cancellation: CancellationToken,
    idle_timeout: Duration,
) -> io::Result<()> {
    let (mut client_read, mut client_write) = client.split();
    let (mut upstream_read, mut upstream_write) = upstream.split();
    // 缓冲区放在堆上，避免每个会话 future 在 JoinSet 中携带 32 KiB 栈状态。
    let mut client_buffer = vec![0_u8; 16 * 1024];
    let mut upstream_buffer = vec![0_u8; 16 * 1024];
    let mut client_closed = false;
    let mut upstream_closed = false;
    let idle = sleep(idle_timeout);
    tokio::pin!(idle);

    while !client_closed || !upstream_closed {
        tokio::select! {
            () = cancellation.cancelled() => return Ok(()),
            () = &mut idle => return Err(timed_out("SOCKS5 relay 空闲超时")),
            read = client_read.read(&mut client_buffer), if !client_closed => {
                let size = read?;
                if size == 0 {
                    client_closed = true;
                    upstream_write.shutdown().await?;
                } else {
                    upstream_write.write_all(&client_buffer[..size]).await?;
                    idle.as_mut().reset(Instant::now() + idle_timeout);
                }
            }
            read = upstream_read.read(&mut upstream_buffer), if !upstream_closed => {
                let size = read?;
                if size == 0 {
                    upstream_closed = true;
                    client_write.shutdown().await?;
                } else {
                    client_write.write_all(&upstream_buffer[..size]).await?;
                    idle.as_mut().reset(Instant::now() + idle_timeout);
                }
            }
        }
    }
    Ok(())
}

fn timed_out(message: &'static str) -> io::Error {
    io::Error::new(io::ErrorKind::TimedOut, message)
}
