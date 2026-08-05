//! Android 上实际运行的 TUN 数据面。
//!
//! 这一层故意不依赖 Tauri。它把 Android 交付的 TUN 文件描述符与 `tun2proxy`
//! 隔开：TUN 两个方向的每一个 IP 包都先经过 [`FailOpenEngine`]，再进入或离开
//! `tun2proxy`。`tun2proxy` 产生的外连统一进入进程内 SOCKS5 服务，SOCKS5 在连接
//! 原始目标前回调 `VpnService.protect(fd)`，避免 Companion 自己的连接递归回到 TUN。

#![allow(unsafe_code)]
#![cfg_attr(not(target_os = "android"), allow(dead_code))]

mod packet;
mod pump;
mod scheduler;
mod stats;
mod tun;

#[cfg(test)]
mod tests;

use std::{
    io,
    net::Ipv4Addr,
    os::fd::{AsRawFd, OwnedFd},
    os::unix::net::UnixDatagram as StdUnixDatagram,
    sync::{Arc, mpsc as sync_mpsc},
    thread,
    time::{Duration, Instant},
};

use jni::{
    JValue, JavaVM, jni_sig, jni_str,
    objects::{Global, JObject},
};
use tokio::net::{TcpListener, UnixDatagram};
use tun2proxy::{ArgDns, ArgProxy, Args, CancellationToken};

use crate::{
    FailOpenEngine, ProxyRuntimeConfiguration, ValidatedProfile, routing::ProxyRouteTable,
};

use self::{
    pump::{pump_proxy_to_tun, pump_tun_to_proxy},
    stats::{RUNTIME_STATS, record_runtime_error, record_runtime_error_for_epoch},
    tun::{ManagedTunFile, TunFdRelease},
};

pub(crate) use self::stats::{
    record_socket_protection_failure, record_socks_client, record_socks_connect_attempt,
    record_socks_connect_success, reset_runtime_stats, runtime_stats_json,
};

pub(crate) const MAX_IP_PACKET_SIZE: usize = 65_535;
// Android VpnService 与 tun2proxy 之间的内部虚拟链路必须保持稳定。Profile 中的
// path_mtu 表示“要模拟的远端路径限制”，只允许由 ImpairmentEngine 处理；若把它
// 直接传给 tun2proxy，内核/库会在弱网规则前分段，PMTU 黑洞和 ICMP 故障便永远
// 看不到超长包。
const INTERNAL_TUN_MTU: u16 = 1_280;
const DATA_PLANE_STOP_TIMEOUT: Duration = Duration::from_secs(2);

/// JNI 层持有的运行句柄。停止时先触发取消，再等待运行线程释放 TUN 副本。
#[derive(Debug)]
pub(crate) struct DataPlaneHandle {
    runtime_epoch: u64,
    cancellation: CancellationToken,
    thread: Option<thread::JoinHandle<()>>,
    thread_finished: sync_mpsc::Receiver<()>,
    tun_release: Option<TunFdRelease>,
}

/// 无论运行线程正常返回还是 panic，析构时都通知 JNI 侧它已经不再持有 TUN。
struct ThreadFinishedNotifier(Option<sync_mpsc::SyncSender<()>>);

impl Drop for ThreadFinishedNotifier {
    fn drop(&mut self) {
        if let Some(sender) = self.0.take() {
            let _ = sender.try_send(());
        }
    }
}

impl DataPlaneHandle {
    /// 启动数据面，并且只在本地 SOCKS5 与 TUN 转换任务都已经创建后返回。
    pub(crate) fn start(
        tun_fd: OwnedFd,
        profile: ValidatedProfile,
        proxy_runtime: ProxyRuntimeConfiguration,
        protector: SocketProtector,
    ) -> Result<Self, String> {
        let runtime_epoch = reset_runtime_stats();
        let (tun_file, tun_release) = ManagedTunFile::new(tun_fd)
            .map_err(|error| format!("准备可安全释放的 Android TUN 失败：{error}"))?;

        let cancellation = CancellationToken::new();
        let thread_cancellation = cancellation.clone();
        let (ready_tx, ready_rx) = sync_mpsc::sync_channel(1);
        let (finished_tx, finished_rx) = sync_mpsc::sync_channel(1);
        let runtime_thread = thread::Builder::new()
            .name("intercept-vpn-data-plane".to_owned())
            .spawn(move || {
                let _finished = ThreadFinishedNotifier(Some(finished_tx));
                let runtime = match tokio::runtime::Builder::new_multi_thread()
                    .worker_threads(2)
                    .enable_all()
                    .build()
                {
                    Ok(runtime) => runtime,
                    Err(error) => {
                        let message = format!("创建 Rust 异步运行时失败：{error}");
                        let _ = ready_tx.send(Err(message.clone()));
                        protector.notify_failure(&message);
                        return;
                    }
                };

                let result = runtime.block_on(run_data_plane(
                    tun_file,
                    profile,
                    proxy_runtime,
                    protector.clone(),
                    thread_cancellation.clone(),
                    ready_tx,
                    runtime_epoch,
                ));
                // `tun2proxy` 的内部实现可能短暂持有 Tokio blocking worker。直接析构
                // Runtime 会无限等待这些 worker，进而把 Android Service 主线程卡在
                // `nativeStop()`：后续 START/STOP intent 全部排队，状态永久停在
                // `start_requested`。取消令牌已经先通知所有核心任务退出；这里再给
                // Runtime 一个有限的收尾窗口，超过窗口也必须归还 Service 生命周期。
                //
                // 这不是静默丢弃数据：关闭 VPN 的语义本来就是立即撤销 TUN、让目标
                // 应用 fail-open 回系统网络。新 Profile 启动也不能被旧 Runtime 无限阻塞。
                runtime.shutdown_timeout(Duration::from_secs(1));
                if let Err(error) = result
                    && !thread_cancellation.is_cancelled()
                {
                    protector.notify_failure(&error);
                }
            })
            .map_err(|error| format!("创建 Rust 数据面线程失败：{error}"))?;

        let mut handle = Self {
            runtime_epoch,
            cancellation,
            thread: Some(runtime_thread),
            thread_finished: finished_rx,
            tun_release: Some(tun_release),
        };
        match ready_rx.recv_timeout(Duration::from_secs(5)) {
            Ok(Ok(())) => Ok(handle),
            Ok(Err(error)) => {
                handle.shutdown();
                Err(error)
            }
            Err(error) => {
                handle.shutdown();
                Err(format!("等待 Rust 数据面启动超时：{error}"))
            }
        }
    }

    pub(crate) fn stop(mut self) {
        self.shutdown();
    }

    /// JNI/Android Service 生命周期绝不能被原生线程无限阻塞。
    ///
    /// Kotlin 会先关闭它持有的主 TUN；这里取消任务后还会原子替换 Rust 的 TUN
    /// 副本，确保目标 UID 路由不再被原生线程维持。随后最多等待固定窗口，超时就分离
    /// 已失去 TUN 引用的线程，不能阻塞 Android Service 主线程。
    fn shutdown(&mut self) {
        self.shutdown_with_timeout(DATA_PLANE_STOP_TIMEOUT);
    }

    fn shutdown_with_timeout(&mut self, timeout: Duration) {
        if self.thread.is_none() {
            return;
        }
        self.cancellation.cancel();
        if let Some(tun_release) = self.tun_release.take()
            && let Err(error) = tun_release.release_tun_reference()
        {
            record_runtime_error_for_epoch(
                self.runtime_epoch,
                &format!("强制释放 Rust TUN 引用失败：{error}"),
            );
        }
        let finished = match self.thread_finished.recv_timeout(timeout) {
            Ok(()) | Err(sync_mpsc::RecvTimeoutError::Disconnected) => true,
            Err(sync_mpsc::RecvTimeoutError::Timeout) => false,
        };
        if finished {
            if let Some(runtime_thread) = self.thread.take() {
                let _ = runtime_thread.join();
            }
        } else if self.thread.take().is_some() {
            record_runtime_error_for_epoch(
                self.runtime_epoch,
                "Rust 数据面线程未在 2 秒内退出；已停止等待并保持取消请求，Android 主 TUN 应立即关闭以恢复直连",
            );
        }
    }
}

impl Drop for DataPlaneHandle {
    fn drop(&mut self) {
        self.shutdown();
    }
}

/// 保存在 Rust 线程中的 Java 全局引用。
#[derive(Clone, Debug)]
pub(crate) struct SocketProtector {
    vm: JavaVM,
    object: Arc<Global<JObject<'static>>>,
}

impl SocketProtector {
    pub(crate) fn new(vm: JavaVM, object: Global<JObject<'static>>) -> Self {
        Self {
            vm,
            object: Arc::new(object),
        }
    }
}

/// SOCKS5 创建外连 socket 时唯一允许调用的保护边界。测试使用记录实现验证 TCP/UDP
/// 路径确实在 connect/send 前调用；Android 运行时实现则转发给 `VpnService`。
pub(crate) trait SocketProtection: Clone + Send + Sync + 'static {
    fn protect(&self, fd: i32) -> io::Result<()>;
    fn notify_failure(&self, message: &str);
}

impl SocketProtection for SocketProtector {
    fn protect(&self, fd: i32) -> io::Result<()> {
        let protected = self
            .vm
            .attach_current_thread(|env| {
                env.call_method(
                    self.object.as_ref(),
                    jni_str!("protectSocket"),
                    jni_sig!("(I)Z"),
                    &[JValue::Int(fd)],
                )?
                .into_bool()
            })
            .map_err(|error| io::Error::other(error.to_string()))?;
        if protected {
            Ok(())
        } else {
            Err(io::Error::other("VpnService.protect(fd) 返回 false"))
        }
    }

    fn notify_failure(&self, message: &str) {
        let _ = self.vm.attach_current_thread(|env| {
            let message = env.new_string(message)?;
            env.call_method(
                self.object.as_ref(),
                jni_str!("onNativeFailure"),
                jni_sig!("(Ljava/lang/String;)V"),
                &[JValue::Object(message.as_ref())],
            )?;
            Ok::<(), jni::errors::Error>(())
        });
    }
}

#[allow(clippy::too_many_lines)]
async fn run_data_plane(
    tun_file: ManagedTunFile,
    profile: ValidatedProfile,
    proxy_runtime: ProxyRuntimeConfiguration,
    protector: SocketProtector,
    cancellation: CancellationToken,
    ready_tx: sync_mpsc::SyncSender<Result<(), String>>,
    runtime_epoch: u64,
) -> Result<(), String> {
    let proxy_routes = Arc::new(
        ProxyRouteTable::compile(&profile, &proxy_runtime)
            .await
            .map_err(|error| format!("编译 Android 透明代理路由失败：{error}"))?,
    );
    let tun_file = Arc::new(
        tokio::io::unix::AsyncFd::new(tun_file)
            .map_err(|error| format!("注册 Android TUN 失败：{error}"))?,
    );

    let (tun2proxy_end, impairment_end) = StdUnixDatagram::pair()
        .map_err(|error| format!("创建 TUN 数据面桥接 socket 失败：{error}"))?;
    tun2proxy_end
        .set_nonblocking(true)
        .map_err(|error| format!("设置 tun2proxy 桥接 socket 失败：{error}"))?;
    impairment_end
        .set_nonblocking(true)
        .map_err(|error| format!("设置弱网桥接 socket 失败：{error}"))?;
    let impairment_end = Arc::new(
        UnixDatagram::from_std(impairment_end)
            .map_err(|error| format!("注册弱网桥接 socket 失败：{error}"))?,
    );

    let socks_listener = TcpListener::bind((Ipv4Addr::LOCALHOST, 0))
        .await
        .map_err(|error| format!("启动进程内 SOCKS5 失败：{error}"))?;
    let socks_address = socks_listener
        .local_addr()
        .map_err(|error| format!("读取 SOCKS5 地址失败：{error}"))?;

    let mtu = INTERNAL_TUN_MTU;
    let mut args = Args::default();
    args.proxy(
        ArgProxy::try_from(format!("socks5://{socks_address}").as_str())
            .map_err(|error| format!("配置本地 SOCKS5 失败：{error}"))?,
    )
    .dns(ArgDns::OverTcp)
    .tun_fd(Some(tun2proxy_end.as_raw_fd()))
    .close_fd_on_drop(false)
    .ipv6_enabled(true);
    args.mtu = mtu;
    // MSS Clamp 同样由下方 Rust 包处理器完成并重算校验和，不能让 tun2proxy
    // 提前消费配置，否则计数、方向和第 N 个包语义都不可验证。
    args.tcp_mss = None;

    let engine = Arc::new(FailOpenEngine::new(&profile));
    let started_at = Instant::now();
    let mut upload = tokio::spawn(pump_tun_to_proxy(
        tun_file.clone(),
        impairment_end.clone(),
        engine.clone(),
        started_at,
        cancellation.child_token(),
    ));
    let mut download = tokio::spawn(pump_proxy_to_tun(
        impairment_end,
        tun_file,
        engine,
        started_at,
        cancellation.child_token(),
    ));
    let mut socks = tokio::spawn(crate::socks5::run_server(
        socks_listener,
        protector,
        proxy_routes,
        cancellation.child_token(),
        runtime_epoch,
    ));
    let mut tun2proxy = tokio::spawn(tun2proxy::general_run_async(
        args,
        mtu,
        false,
        cancellation.child_token(),
    ));

    // spawn 只登记任务；至少让调度器运行一次，才能避免“任务尚未首次 poll 就上报就绪”。
    tokio::task::yield_now().await;
    let early_failure = if upload.is_finished() {
        Some(join_io_task(&mut upload, "读取 Android TUN").await)
    } else if download.is_finished() {
        Some(join_io_task(&mut download, "写回 Android TUN").await)
    } else if socks.is_finished() {
        Some(join_io_task(&mut socks, "SOCKS5 服务").await)
    } else if tun2proxy.is_finished() {
        Some(join_tun2proxy_task(&mut tun2proxy).await)
    } else {
        None
    };
    if let Some(result) = early_failure {
        upload.abort();
        download.abort();
        socks.abort();
        tun2proxy.abort();
        let error = result
            .err()
            .unwrap_or_else(|| "核心数据面任务在就绪前意外结束".to_owned());
        let _ = ready_tx.send(Err(error.clone()));
        return Err(error);
    }

    ready_tx
        .send(Ok(()))
        .map_err(|error| format!("通知 Kotlin 数据面已就绪失败：{error}"))?;

    // `tun2proxy_end` 必须存活到 `general_run_async` 结束，因为我们要求底层库不关闭
    // 这个借用 fd。任一核心任务异常退出都使整个 VPN fail-open。
    let _keep_tun2proxy_fd_alive = tun2proxy_end;
    let result = tokio::select! {
        () = cancellation.cancelled() => Ok(()),
        result = &mut upload => flatten_io_join(result, "读取 Android TUN"),
        result = &mut download => flatten_io_join(result, "写回 Android TUN"),
        result = &mut socks => flatten_io_join(result, "SOCKS5 服务"),
        result = &mut tun2proxy => flatten_tun2proxy_join(result),
    };
    // 不在内部故障路径取消根令牌。运行线程通过根令牌区分“Service 主动停止”和
    // “数据面异常退出”，后者必须回调 Kotlin 关闭主 TUN，才能真正 fail-open。
    upload.abort();
    download.abort();
    socks.abort();
    tun2proxy.abort();
    result
}

async fn join_io_task(
    task: &mut tokio::task::JoinHandle<io::Result<()>>,
    name: &str,
) -> Result<(), String> {
    flatten_io_join(task.await, name)
}

fn flatten_io_join(
    result: Result<io::Result<()>, tokio::task::JoinError>,
    name: &str,
) -> Result<(), String> {
    result
        .map_err(|error| format!("{name}任务异常：{error}"))?
        .map_err(|error| format!("{name}失败：{error}"))
}

async fn join_tun2proxy_task(
    task: &mut tokio::task::JoinHandle<io::Result<usize>>,
) -> Result<(), String> {
    flatten_tun2proxy_join(task.await)
}

fn flatten_tun2proxy_join(
    result: Result<io::Result<usize>, tokio::task::JoinError>,
) -> Result<(), String> {
    result
        .map_err(|error| format!("tun2proxy 任务异常：{error}"))?
        .map(|_| ())
        .map_err(|error| format!("tun2proxy 运行失败：{error}"))
}
