//! Android 上实际运行的 TUN 数据面。
//!
//! 这一层故意不依赖 Tauri。它把 Android 交付的 TUN 文件描述符与 `tun2proxy`
//! 隔开：TUN 两个方向的每一个 IP 包都先经过 [`FailOpenEngine`]，再进入或离开
//! `tun2proxy`。`tun2proxy` 产生的外连统一进入进程内 SOCKS5 服务，SOCKS5 在连接
//! 原始目标前回调 `VpnService.protect(fd)`，避免 Companion 自己的连接递归回到 TUN。

#![allow(unsafe_code)]
#![cfg_attr(not(target_os = "android"), allow(dead_code))]

use std::{
    cmp::Ordering,
    collections::{BTreeSet, BinaryHeap},
    future::Future,
    io::{self, Read, Write},
    mem::ManuallyDrop,
    net::{IpAddr, Ipv4Addr, Ipv6Addr, SocketAddr},
    os::fd::{AsRawFd, OwnedFd, RawFd},
    os::unix::net::UnixDatagram as StdUnixDatagram,
    sync::{
        Arc, Mutex, OnceLock,
        atomic::{AtomicU64, Ordering as AtomicOrdering},
        mpsc as sync_mpsc,
    },
    thread,
    time::{Duration, Instant},
};

use jni::{
    JValue, JavaVM, jni_sig, jni_str,
    objects::{Global, JObject},
};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::{TcpListener, TcpSocket, TcpStream, UdpSocket, UnixDatagram},
    sync::mpsc,
    time::{Instant as TokioInstant, sleep_until},
};
use tun2proxy::{ArgDns, ArgProxy, Args, CancellationToken};

use crate::{
    Direction, FailOpenEngine, IpVersion, PacketContext, PathMtuAction, ProxyRuntimeConfiguration,
    TcpFlag, TransportProtocol, ValidatedProfile, routing::ProxyRouteTable,
};

const MAX_IP_PACKET_SIZE: usize = 65_535;
// Android VpnService 与 tun2proxy 之间的内部虚拟链路必须保持稳定。Profile 中的
// path_mtu 表示“要模拟的远端路径限制”，只允许由 ImpairmentEngine 处理；若把它
// 直接传给 tun2proxy，内核/库会在弱网规则前分段，PMTU 黑洞和 ICMP 故障便永远
// 看不到超长包。
const INTERNAL_TUN_MTU: u16 = 1_280;
const SOCKS_VERSION: u8 = 5;
const SOCKS_SUCCEEDED: u8 = 0;
const SOCKS_GENERAL_FAILURE: u8 = 1;
const SOCKS_COMMAND_NOT_SUPPORTED: u8 = 7;
const DATA_PLANE_STOP_TIMEOUT: Duration = Duration::from_secs(2);

unsafe extern "C" {
    #[link_name = "dup2"]
    fn c_dup2(old_fd: i32, new_fd: i32) -> i32;
}

#[derive(Debug, Eq, PartialEq)]
enum TunFdState {
    Active,
    ReplacedWithDevNull,
    Closed,
}

/// TUN fd 的共享释放状态。
///
/// 运行线程中的 `File` 与 JNI 句柄共同持有该状态。停止时不能直接 `close(raw_fd)`：
/// 线程稍后继续 I/O 时，数字 fd 可能已经被其他 socket 复用。改用 `dup2(/dev/null)`
/// 原子替换同一个数字 fd，既立即释放 TUN 的内核引用，又让迟到的读写安全地得到 EOF。
#[derive(Debug)]
struct TunFdLease {
    raw_fd: RawFd,
    dev_null: std::fs::File,
    state: Mutex<TunFdState>,
}

#[derive(Clone, Debug)]
struct TunFdRelease(Arc<TunFdLease>);

impl TunFdRelease {
    fn release_tun_reference(&self) -> io::Result<()> {
        let mut state = self
            .0
            .state
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if *state != TunFdState::Active {
            return Ok(());
        }
        // SAFETY: 两个 fd 在 ManagedTunFile 创建时均有效；状态锁保证不会和 File
        // 析构并发。dup2 原子关闭 TUN 引用并让 raw_fd 继续指向预先打开的 /dev/null。
        let result = unsafe { c_dup2(self.0.dev_null.as_raw_fd(), self.0.raw_fd) };
        if result < 0 {
            return Err(io::Error::last_os_error());
        }
        *state = TunFdState::ReplacedWithDevNull;
        Ok(())
    }
}

#[derive(Debug)]
struct ManagedTunFile {
    file: ManuallyDrop<std::fs::File>,
    lease: Arc<TunFdLease>,
}

impl ManagedTunFile {
    fn new(tun_fd: OwnedFd) -> io::Result<(Self, TunFdRelease)> {
        let file = std::fs::File::from(tun_fd);
        let lease = Arc::new(TunFdLease {
            raw_fd: file.as_raw_fd(),
            dev_null: std::fs::OpenOptions::new()
                .read(true)
                .write(true)
                .open("/dev/null")?,
            state: Mutex::new(TunFdState::Active),
        });
        Ok((
            Self {
                file: ManuallyDrop::new(file),
                lease: lease.clone(),
            },
            TunFdRelease(lease),
        ))
    }
}

impl AsRawFd for ManagedTunFile {
    fn as_raw_fd(&self) -> RawFd {
        self.lease.raw_fd
    }
}

impl Read for &ManagedTunFile {
    fn read(&mut self, buffer: &mut [u8]) -> io::Result<usize> {
        (&*self.file).read(buffer)
    }
}

impl Write for &ManagedTunFile {
    fn write(&mut self, buffer: &[u8]) -> io::Result<usize> {
        (&*self.file).write(buffer)
    }

    fn flush(&mut self) -> io::Result<()> {
        (&*self.file).flush()
    }
}

impl Drop for ManagedTunFile {
    fn drop(&mut self) {
        let mut state = self
            .lease
            .state
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        // SAFETY: 状态锁与 TunFdRelease 串行化；无论 fd 仍指向 TUN 还是已替换成
        // /dev/null，ManuallyDrop 中的 File 都只在这里析构一次。
        unsafe { ManuallyDrop::drop(&mut self.file) };
        *state = TunFdState::Closed;
    }
}

/// 设备端控制页和自动化验收共用的最小数据面计数器。
///
/// 这些值只记录包数/字节数和连接阶段，不保存任何应用报文内容。没有这些计数器时，
/// “VPN 已连接但目标应用没有网络”无法区分是 Android 没把包送入 TUN、弱网调度没有
/// 转发、`tun2proxy` 没有建连，还是受保护的外连失败。
#[derive(Debug, Default)]
struct RuntimeStats {
    tun_upload_packets: AtomicU64,
    tun_upload_bytes: AtomicU64,
    proxy_upload_packets: AtomicU64,
    proxy_download_packets: AtomicU64,
    tun_download_packets: AtomicU64,
    tun_download_bytes: AtomicU64,
    socks_clients: AtomicU64,
    socks_connect_attempts: AtomicU64,
    socks_connect_successes: AtomicU64,
    protect_failures: AtomicU64,
    upload_tcp_syn_packets: AtomicU64,
    upload_tcp_ack_packets: AtomicU64,
    download_tcp_syn_ack_packets: AtomicU64,
    download_ip_length_mismatches: AtomicU64,
    download_ip_checksum_failures: AtomicU64,
    download_transport_checksum_failures: AtomicU64,
    impairment_packets_dropped: AtomicU64,
    impairment_packets_duplicated: AtomicU64,
    impairment_packets_reordered: AtomicU64,
    impairment_packets_corrupted: AtomicU64,
    impairment_delay_millis_total: AtomicU64,
    impairment_mss_clamps: AtomicU64,
    impairment_pmtu_fragments: AtomicU64,
    impairment_pmtu_signals: AtomicU64,
    impairment_unimplemented_pmtu_actions: AtomicU64,
}

static RUNTIME_STATS: RuntimeStats = RuntimeStats {
    tun_upload_packets: AtomicU64::new(0),
    tun_upload_bytes: AtomicU64::new(0),
    proxy_upload_packets: AtomicU64::new(0),
    proxy_download_packets: AtomicU64::new(0),
    tun_download_packets: AtomicU64::new(0),
    tun_download_bytes: AtomicU64::new(0),
    socks_clients: AtomicU64::new(0),
    socks_connect_attempts: AtomicU64::new(0),
    socks_connect_successes: AtomicU64::new(0),
    protect_failures: AtomicU64::new(0),
    upload_tcp_syn_packets: AtomicU64::new(0),
    upload_tcp_ack_packets: AtomicU64::new(0),
    download_tcp_syn_ack_packets: AtomicU64::new(0),
    download_ip_length_mismatches: AtomicU64::new(0),
    download_ip_checksum_failures: AtomicU64::new(0),
    download_transport_checksum_failures: AtomicU64::new(0),
    impairment_packets_dropped: AtomicU64::new(0),
    impairment_packets_duplicated: AtomicU64::new(0),
    impairment_packets_reordered: AtomicU64::new(0),
    impairment_packets_corrupted: AtomicU64::new(0),
    impairment_delay_millis_total: AtomicU64::new(0),
    impairment_mss_clamps: AtomicU64::new(0),
    impairment_pmtu_fragments: AtomicU64::new(0),
    impairment_pmtu_signals: AtomicU64::new(0),
    impairment_unimplemented_pmtu_actions: AtomicU64::new(0),
};

static LAST_RUNTIME_ERROR: OnceLock<Mutex<Option<String>>> = OnceLock::new();
// 每次启动数据面都会获得一个新的运行周期。SOCKS 会话是独立 Tokio 任务，旧周期
// 停止时这些任务可能仍在短暂收尾；没有周期校验时，它们会把旧连接错误写进新周期。
static ACTIVE_RUNTIME_EPOCH: AtomicU64 = AtomicU64::new(0);

fn record_runtime_error(error: &str) {
    if let Ok(mut slot) = LAST_RUNTIME_ERROR.get_or_init(|| Mutex::new(None)).lock() {
        *slot = Some(error.to_owned());
    }
}

fn record_runtime_error_for_epoch(epoch: u64, error: &str) {
    if ACTIVE_RUNTIME_EPOCH.load(AtomicOrdering::Acquire) == epoch {
        record_runtime_error(error);
    }
}

fn increment_for_epoch(epoch: u64, counter: &AtomicU64) {
    if ACTIVE_RUNTIME_EPOCH.load(AtomicOrdering::Acquire) == epoch {
        counter.fetch_add(1, AtomicOrdering::Relaxed);
    }
}

pub(crate) fn reset_runtime_stats() -> u64 {
    let epoch = ACTIVE_RUNTIME_EPOCH.fetch_add(1, AtomicOrdering::AcqRel) + 1;
    for value in [
        &RUNTIME_STATS.tun_upload_packets,
        &RUNTIME_STATS.tun_upload_bytes,
        &RUNTIME_STATS.proxy_upload_packets,
        &RUNTIME_STATS.proxy_download_packets,
        &RUNTIME_STATS.tun_download_packets,
        &RUNTIME_STATS.tun_download_bytes,
        &RUNTIME_STATS.socks_clients,
        &RUNTIME_STATS.socks_connect_attempts,
        &RUNTIME_STATS.socks_connect_successes,
        &RUNTIME_STATS.protect_failures,
        &RUNTIME_STATS.upload_tcp_syn_packets,
        &RUNTIME_STATS.upload_tcp_ack_packets,
        &RUNTIME_STATS.download_tcp_syn_ack_packets,
        &RUNTIME_STATS.download_ip_length_mismatches,
        &RUNTIME_STATS.download_ip_checksum_failures,
        &RUNTIME_STATS.download_transport_checksum_failures,
        &RUNTIME_STATS.impairment_packets_dropped,
        &RUNTIME_STATS.impairment_packets_duplicated,
        &RUNTIME_STATS.impairment_packets_reordered,
        &RUNTIME_STATS.impairment_packets_corrupted,
        &RUNTIME_STATS.impairment_delay_millis_total,
        &RUNTIME_STATS.impairment_mss_clamps,
        &RUNTIME_STATS.impairment_pmtu_fragments,
        &RUNTIME_STATS.impairment_pmtu_signals,
        &RUNTIME_STATS.impairment_unimplemented_pmtu_actions,
    ] {
        value.store(0, AtomicOrdering::Relaxed);
    }
    if let Ok(mut slot) = LAST_RUNTIME_ERROR.get_or_init(|| Mutex::new(None)).lock() {
        *slot = None;
    }
    epoch
}

pub(crate) fn runtime_stats_json() -> String {
    let last_error = LAST_RUNTIME_ERROR
        .get_or_init(|| Mutex::new(None))
        .lock()
        .ok()
        .and_then(|value| value.clone());
    serde_json::json!({
        "tun_upload_packets": RUNTIME_STATS.tun_upload_packets.load(AtomicOrdering::Relaxed),
        "tun_upload_bytes": RUNTIME_STATS.tun_upload_bytes.load(AtomicOrdering::Relaxed),
        "proxy_upload_packets": RUNTIME_STATS.proxy_upload_packets.load(AtomicOrdering::Relaxed),
        "proxy_download_packets": RUNTIME_STATS.proxy_download_packets.load(AtomicOrdering::Relaxed),
        "tun_download_packets": RUNTIME_STATS.tun_download_packets.load(AtomicOrdering::Relaxed),
        "tun_download_bytes": RUNTIME_STATS.tun_download_bytes.load(AtomicOrdering::Relaxed),
        "socks_clients": RUNTIME_STATS.socks_clients.load(AtomicOrdering::Relaxed),
        "socks_connect_attempts": RUNTIME_STATS.socks_connect_attempts.load(AtomicOrdering::Relaxed),
        "socks_connect_successes": RUNTIME_STATS.socks_connect_successes.load(AtomicOrdering::Relaxed),
        "protect_failures": RUNTIME_STATS.protect_failures.load(AtomicOrdering::Relaxed),
        "upload_tcp_syn_packets": RUNTIME_STATS.upload_tcp_syn_packets.load(AtomicOrdering::Relaxed),
        "upload_tcp_ack_packets": RUNTIME_STATS.upload_tcp_ack_packets.load(AtomicOrdering::Relaxed),
        "download_tcp_syn_ack_packets": RUNTIME_STATS.download_tcp_syn_ack_packets.load(AtomicOrdering::Relaxed),
        "download_ip_length_mismatches": RUNTIME_STATS.download_ip_length_mismatches.load(AtomicOrdering::Relaxed),
        "download_ip_checksum_failures": RUNTIME_STATS.download_ip_checksum_failures.load(AtomicOrdering::Relaxed),
        "download_transport_checksum_failures": RUNTIME_STATS.download_transport_checksum_failures.load(AtomicOrdering::Relaxed),
        "impairment_packets_dropped": RUNTIME_STATS.impairment_packets_dropped.load(AtomicOrdering::Relaxed),
        "impairment_packets_duplicated": RUNTIME_STATS.impairment_packets_duplicated.load(AtomicOrdering::Relaxed),
        "impairment_packets_reordered": RUNTIME_STATS.impairment_packets_reordered.load(AtomicOrdering::Relaxed),
        "impairment_packets_corrupted": RUNTIME_STATS.impairment_packets_corrupted.load(AtomicOrdering::Relaxed),
        "impairment_delay_millis_total": RUNTIME_STATS.impairment_delay_millis_total.load(AtomicOrdering::Relaxed),
        "impairment_mss_clamps": RUNTIME_STATS.impairment_mss_clamps.load(AtomicOrdering::Relaxed),
        "impairment_pmtu_fragments": RUNTIME_STATS.impairment_pmtu_fragments.load(AtomicOrdering::Relaxed),
        "impairment_pmtu_signals": RUNTIME_STATS.impairment_pmtu_signals.load(AtomicOrdering::Relaxed),
        "impairment_unimplemented_pmtu_actions": RUNTIME_STATS.impairment_unimplemented_pmtu_actions.load(AtomicOrdering::Relaxed),
        "last_error": last_error,
    })
    .to_string()
}

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
trait SocketProtection: Clone + Send + Sync + 'static {
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
    let mut socks = tokio::spawn(run_socks_server(
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

async fn pump_tun_to_proxy(
    tun: Arc<tokio::io::unix::AsyncFd<ManagedTunFile>>,
    proxy: Arc<UnixDatagram>,
    engine: Arc<FailOpenEngine>,
    started_at: Instant,
    cancellation: CancellationToken,
) -> io::Result<()> {
    let (sender, receiver) = mpsc::channel(512);
    let reader_cancellation = cancellation.child_token();
    let reader_tun = tun.clone();
    let reader = async move {
        let mut packet = vec![0_u8; MAX_IP_PACKET_SIZE];
        let mut sequence = 0_u64;
        loop {
            let size = tokio::select! {
                () = reader_cancellation.cancelled() => return Ok(()),
                size = read_tun_packet(&reader_tun, &mut packet) => size?,
            };
            RUNTIME_STATS
                .tun_upload_packets
                .fetch_add(1, AtomicOrdering::Relaxed);
            RUNTIME_STATS
                .tun_upload_bytes
                .fetch_add(size as u64, AtomicOrdering::Relaxed);
            record_upload_packet_diagnostics(&packet[..size]);
            for scheduled in prepare_scheduled_packets(
                packet[..size].to_vec(),
                Direction::Upload,
                &engine,
                started_at,
                sequence,
            )? {
                sequence = sequence.saturating_add(1);
                if sender.send(scheduled).await.is_err() {
                    return Ok(());
                }
            }
        }
    };
    let scheduler = run_packet_scheduler(
        receiver,
        cancellation.child_token(),
        move |route, packet| {
            let proxy = proxy.clone();
            let tun = tun.clone();
            async move {
                match route {
                    PacketRoute::Forward => proxy.send(&packet).await.map(|_| {
                        RUNTIME_STATS
                            .proxy_upload_packets
                            .fetch_add(1, AtomicOrdering::Relaxed);
                    }),
                    PacketRoute::Reverse => {
                        let size = packet.len() as u64;
                        write_tun_packet(&tun, &packet).await?;
                        RUNTIME_STATS
                            .tun_download_packets
                            .fetch_add(1, AtomicOrdering::Relaxed);
                        RUNTIME_STATS
                            .tun_download_bytes
                            .fetch_add(size, AtomicOrdering::Relaxed);
                        Ok(())
                    }
                }
            }
        },
    );
    tokio::select! {
        () = cancellation.cancelled() => Ok(()),
        result = reader => result,
        result = scheduler => result,
    }
}

async fn pump_proxy_to_tun(
    proxy: Arc<UnixDatagram>,
    tun: Arc<tokio::io::unix::AsyncFd<ManagedTunFile>>,
    engine: Arc<FailOpenEngine>,
    started_at: Instant,
    cancellation: CancellationToken,
) -> io::Result<()> {
    let (sender, receiver) = mpsc::channel(512);
    let reader_cancellation = cancellation.child_token();
    let reader_proxy = proxy.clone();
    let reader = async move {
        let mut packet = vec![0_u8; MAX_IP_PACKET_SIZE];
        let mut sequence = 0_u64;
        loop {
            let size = tokio::select! {
                () = reader_cancellation.cancelled() => return Ok(()),
                result = reader_proxy.recv(&mut packet) => result?,
            };
            RUNTIME_STATS
                .proxy_download_packets
                .fetch_add(1, AtomicOrdering::Relaxed);
            record_download_packet_diagnostics(&packet[..size]);
            for scheduled in prepare_scheduled_packets(
                packet[..size].to_vec(),
                Direction::Download,
                &engine,
                started_at,
                sequence,
            )? {
                sequence = sequence.saturating_add(1);
                if sender.send(scheduled).await.is_err() {
                    return Ok(());
                }
            }
        }
    };
    let scheduler = run_packet_scheduler(
        receiver,
        cancellation.child_token(),
        move |route, packet| {
            let tun = tun.clone();
            let proxy = proxy.clone();
            async move {
                match route {
                    PacketRoute::Forward => {
                        let size = packet.len() as u64;
                        write_tun_packet(&tun, &packet).await?;
                        RUNTIME_STATS
                            .tun_download_packets
                            .fetch_add(1, AtomicOrdering::Relaxed);
                        RUNTIME_STATS
                            .tun_download_bytes
                            .fetch_add(size, AtomicOrdering::Relaxed);
                        Ok(())
                    }
                    PacketRoute::Reverse => proxy.send(&packet).await.map(|_| {
                        RUNTIME_STATS
                            .proxy_upload_packets
                            .fetch_add(1, AtomicOrdering::Relaxed);
                    }),
                }
            }
        },
    );
    tokio::select! {
        () = cancellation.cancelled() => Ok(()),
        result = reader => result,
        result = scheduler => result,
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum PacketRoute {
    Forward,
    Reverse,
}

#[derive(Debug, Eq)]
struct ScheduledPacket {
    due: TokioInstant,
    sequence: u64,
    copies: u8,
    route: PacketRoute,
    packet: Vec<u8>,
}

impl Ord for ScheduledPacket {
    fn cmp(&self, other: &Self) -> Ordering {
        // BinaryHeap 默认弹出“最大”元素，故反转时间与序号，使最早到期、最小序号优先。
        other
            .due
            .cmp(&self.due)
            .then_with(|| other.sequence.cmp(&self.sequence))
    }
}

impl PartialOrd for ScheduledPacket {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

impl PartialEq for ScheduledPacket {
    fn eq(&self, other: &Self) -> bool {
        self.due == other.due && self.sequence == other.sequence
    }
}

fn prepare_scheduled_packets(
    mut packet: Vec<u8>,
    direction: Direction,
    engine: &FailOpenEngine,
    started_at: Instant,
    sequence: u64,
) -> io::Result<Vec<ScheduledPacket>> {
    let metadata = ParsedPacket::parse(&packet);
    let payload = metadata
        .as_ref()
        .map_or(&[][..], |parsed| &packet[parsed.payload_range.clone()]);
    let context = PacketContext {
        elapsed_millis: started_at
            .elapsed()
            .as_millis()
            .try_into()
            .unwrap_or(u64::MAX),
        direction,
        ip_version: metadata
            .as_ref()
            .map_or(IpVersion::V4, |value| value.ip_version),
        transport: metadata
            .as_ref()
            .map_or(TransportProtocol::Other, |value| value.transport),
        destination_port: metadata.as_ref().and_then(|value| value.destination_port),
        remote_address: metadata
            .as_ref()
            .map(|value| value.remote_address(direction)),
        remote_port: metadata
            .as_ref()
            .and_then(|value| value.remote_port(direction)),
        tcp_flags: metadata
            .as_ref()
            .map_or_else(BTreeSet::new, |value| value.tcp_flags.clone()),
        packet_len: packet.len(),
        payload,
    };
    let (decision, _engine_error) = engine.evaluate(&context);
    if decision.drop_reason.is_some() || decision.copies == 0 {
        RUNTIME_STATS
            .impairment_packets_dropped
            .fetch_add(1, AtomicOrdering::Relaxed);
        return Ok(Vec::new());
    }

    if decision.copies > 1 {
        RUNTIME_STATS
            .impairment_packets_duplicated
            .fetch_add(1, AtomicOrdering::Relaxed);
    }
    if decision.reorder_hold_millis > 0 {
        RUNTIME_STATS
            .impairment_packets_reordered
            .fetch_add(1, AtomicOrdering::Relaxed);
    }
    if decision.payload.as_slice() != payload {
        RUNTIME_STATS
            .impairment_packets_corrupted
            .fetch_add(1, AtomicOrdering::Relaxed);
    }
    RUNTIME_STATS.impairment_delay_millis_total.fetch_add(
        decision
            .delay_millis
            .saturating_add(decision.reorder_hold_millis),
        AtomicOrdering::Relaxed,
    );

    if metadata.is_none() && !matches!(decision.path_mtu_action, PathMtuAction::None) {
        return Err(path_mtu_error(
            decision.path_mtu_action,
            "触发包不是可解析的 IPv4/IPv6 数据报",
        ));
    }

    if let Some(metadata) = metadata {
        let mut packet_changed = false;
        if decision.payload.len() == metadata.payload_range.len()
            && decision.payload.as_slice() != &packet[metadata.payload_range.clone()]
        {
            packet[metadata.payload_range.clone()].copy_from_slice(&decision.payload);
            packet_changed = true;
        }
        if matches!(decision.path_mtu_action, PathMtuAction::ClampMss(_)) {
            let clamped = clamp_existing_tcp_mss(&mut packet, &metadata, decision.path_mtu_action);
            if clamped {
                RUNTIME_STATS
                    .impairment_mss_clamps
                    .fetch_add(1, AtomicOrdering::Relaxed);
            }
            packet_changed |= clamped;
        } else if let Some(pmtu_packets) = prepare_path_mtu_packets(&packet, &decision, sequence)? {
            return Ok(pmtu_packets);
        }
        // 未修改的 IP 包必须保持字节级透传。部分 Android/内核路径会使用其自身的
        // checksum 表示方式；无条件“规范化”校验和会让原本有效的 SYN-ACK/ACK 被
        // 目标应用 TCP 栈丢弃。
        if packet_changed {
            refresh_checksums(&mut packet, &metadata);
        }
    }

    let delay_millis = decision
        .delay_millis
        .saturating_add(decision.reorder_hold_millis);
    Ok(vec![ScheduledPacket {
        due: TokioInstant::now() + Duration::from_millis(delay_millis),
        sequence,
        copies: decision.copies,
        route: PacketRoute::Forward,
        packet,
    }])
}

fn prepare_path_mtu_packets(
    packet: &[u8],
    decision: &crate::PacketDecision,
    sequence: u64,
) -> io::Result<Option<Vec<ScheduledPacket>>> {
    let delay_millis = decision
        .delay_millis
        .saturating_add(decision.reorder_hold_millis);
    match decision.path_mtu_action {
        PathMtuAction::FragmentIpv4(mtu) => fragment_ipv4_packet(packet, mtu).map(|fragments| {
            RUNTIME_STATS
                .impairment_pmtu_fragments
                .fetch_add(fragments.len() as u64, AtomicOrdering::Relaxed);
            fragments
                .into_iter()
                .enumerate()
                .map(|(index, packet)| ScheduledPacket {
                    due: TokioInstant::now() + Duration::from_millis(delay_millis),
                    sequence: sequence.saturating_add(index as u64),
                    copies: decision.copies,
                    route: PacketRoute::Forward,
                    packet,
                })
                .collect()
        }),
        PathMtuAction::Icmpv4FragmentationNeeded(mtu) => {
            build_icmpv4_fragmentation_needed(packet, mtu).map(|signal| {
                record_pmtu_signal();
                vec![reverse_signal(sequence, signal)]
            })
        }
        PathMtuAction::Icmpv6PacketTooBig(mtu) => {
            build_icmpv6_packet_too_big(packet, mtu).map(|signal| {
                record_pmtu_signal();
                vec![reverse_signal(sequence, signal)]
            })
        }
        PathMtuAction::None | PathMtuAction::ClampMss(_) => return Ok(None),
    }
    .map(Some)
    .ok_or_else(|| path_mtu_error(decision.path_mtu_action, "无法构造所需分片或 ICMP 信号"))
}

fn path_mtu_error(action: PathMtuAction, reason: &str) -> io::Error {
    RUNTIME_STATS
        .impairment_unimplemented_pmtu_actions
        .fetch_add(1, AtomicOrdering::Relaxed);
    let error = format!(
        "无法为当前 IP 包执行路径 MTU 动作 {action:?}：{reason}；已终止数据面并恢复系统直连"
    );
    record_runtime_error(&error);
    io::Error::other(error)
}

fn record_pmtu_signal() {
    RUNTIME_STATS
        .impairment_packets_dropped
        .fetch_add(1, AtomicOrdering::Relaxed);
    RUNTIME_STATS
        .impairment_pmtu_signals
        .fetch_add(1, AtomicOrdering::Relaxed);
}

fn reverse_signal(sequence: u64, packet: Vec<u8>) -> ScheduledPacket {
    ScheduledPacket {
        due: TokioInstant::now(),
        sequence,
        copies: 1,
        route: PacketRoute::Reverse,
        packet,
    }
}

async fn run_packet_scheduler<F, Fut>(
    mut receiver: mpsc::Receiver<ScheduledPacket>,
    cancellation: CancellationToken,
    send: F,
) -> io::Result<()>
where
    F: Fn(PacketRoute, Vec<u8>) -> Fut,
    Fut: Future<Output = io::Result<()>>,
{
    let mut queue = BinaryHeap::new();
    let mut input_closed = false;
    loop {
        if queue.is_empty() {
            if input_closed {
                return Ok(());
            }
            tokio::select! {
                () = cancellation.cancelled() => return Ok(()),
                packet = receiver.recv() => match packet {
                    Some(packet) => queue.push(packet),
                    None => input_closed = true,
                },
            }
            continue;
        }

        let due = queue.peek().expect("队列非空").due;
        tokio::select! {
            () = cancellation.cancelled() => return Ok(()),
            packet = receiver.recv(), if !input_closed => match packet {
                Some(packet) => queue.push(packet),
                None => input_closed = true,
            },
            () = sleep_until(due) => {
                let packet = queue.pop().expect("到期队列非空");
                for _ in 0..packet.copies {
                    send(packet.route, packet.packet.clone()).await?;
                }
            }
        }
    }
}

async fn read_tun_packet(
    tun: &tokio::io::unix::AsyncFd<ManagedTunFile>,
    buffer: &mut [u8],
) -> io::Result<usize> {
    loop {
        let mut guard = tun.readable().await?;
        match guard.try_io(|inner| inner.get_ref().read(buffer)) {
            Ok(result) => return result,
            Err(_would_block) => {}
        }
    }
}

async fn write_tun_packet(
    tun: &tokio::io::unix::AsyncFd<ManagedTunFile>,
    packet: &[u8],
) -> io::Result<()> {
    let mut written = 0;
    while written < packet.len() {
        let mut guard = tun.writable().await?;
        match guard.try_io(|inner| inner.get_ref().write(&packet[written..])) {
            Ok(Ok(size)) => written += size,
            Ok(Err(error)) => return Err(error),
            Err(_would_block) => {}
        }
    }
    Ok(())
}

#[derive(Clone, Debug)]
struct ParsedPacket {
    ip_version: IpVersion,
    transport: TransportProtocol,
    source_address: IpAddr,
    destination_address: IpAddr,
    source_port: Option<u16>,
    destination_port: Option<u16>,
    tcp_flags: BTreeSet<TcpFlag>,
    transport_offset: usize,
    payload_range: std::ops::Range<usize>,
}

impl ParsedPacket {
    fn parse(packet: &[u8]) -> Option<Self> {
        let version = packet.first()? >> 4;
        match version {
            4 => Self::parse_v4(packet),
            6 => Self::parse_v6(packet),
            _ => None,
        }
    }

    fn parse_v4(packet: &[u8]) -> Option<Self> {
        if packet.len() < 20 {
            return None;
        }
        let header_len = usize::from(packet[0] & 0x0f) * 4;
        if header_len < 20 || packet.len() < header_len {
            return None;
        }
        let source_address = IpAddr::V4(Ipv4Addr::new(
            packet[12], packet[13], packet[14], packet[15],
        ));
        let destination_address = IpAddr::V4(Ipv4Addr::new(
            packet[16], packet[17], packet[18], packet[19],
        ));
        Self::parse_transport(
            packet,
            IpVersion::V4,
            source_address,
            destination_address,
            packet[9],
            header_len,
        )
    }

    fn parse_v6(packet: &[u8]) -> Option<Self> {
        if packet.len() < 40 {
            return None;
        }
        // 首版只解析无扩展头的 TCP/UDP；遇到扩展头保持原样 fail-open。
        let source_address = IpAddr::V6(Ipv6Addr::from(<[u8; 16]>::try_from(&packet[8..24]).ok()?));
        let destination_address =
            IpAddr::V6(Ipv6Addr::from(<[u8; 16]>::try_from(&packet[24..40]).ok()?));
        Self::parse_transport(
            packet,
            IpVersion::V6,
            source_address,
            destination_address,
            packet[6],
            40,
        )
    }

    fn parse_transport(
        packet: &[u8],
        ip_version: IpVersion,
        source_address: IpAddr,
        destination_address: IpAddr,
        protocol: u8,
        transport_offset: usize,
    ) -> Option<Self> {
        match protocol {
            6 if packet.len() >= transport_offset + 20 => {
                let tcp_header_len = usize::from(packet[transport_offset + 12] >> 4) * 4;
                let payload_start = transport_offset.checked_add(tcp_header_len)?;
                if tcp_header_len < 20 || payload_start > packet.len() {
                    return None;
                }
                let flags = packet[transport_offset + 13];
                let mut tcp_flags = BTreeSet::new();
                if flags & 0x02 != 0 && flags & 0x10 != 0 {
                    tcp_flags.insert(TcpFlag::SynAck);
                } else if flags & 0x02 != 0 {
                    tcp_flags.insert(TcpFlag::Syn);
                } else if flags & 0x10 != 0 {
                    tcp_flags.insert(TcpFlag::Ack);
                }
                if flags & 0x01 != 0 {
                    tcp_flags.insert(TcpFlag::Fin);
                }
                if flags & 0x04 != 0 {
                    tcp_flags.insert(TcpFlag::Rst);
                }
                Some(Self {
                    ip_version,
                    transport: TransportProtocol::Tcp,
                    source_address,
                    destination_address,
                    source_port: read_u16(packet, transport_offset),
                    destination_port: read_u16(packet, transport_offset + 2),
                    tcp_flags,
                    transport_offset,
                    payload_range: payload_start..packet.len(),
                })
            }
            17 if packet.len() >= transport_offset + 8 => Some(Self {
                ip_version,
                transport: TransportProtocol::Udp,
                source_address,
                destination_address,
                source_port: read_u16(packet, transport_offset),
                destination_port: read_u16(packet, transport_offset + 2),
                tcp_flags: BTreeSet::new(),
                transport_offset,
                payload_range: transport_offset + 8..packet.len(),
            }),
            _ => Some(Self {
                ip_version,
                transport: TransportProtocol::Other,
                source_address,
                destination_address,
                source_port: None,
                destination_port: None,
                tcp_flags: BTreeSet::new(),
                transport_offset,
                payload_range: packet.len()..packet.len(),
            }),
        }
    }

    fn remote_address(&self, direction: Direction) -> IpAddr {
        match direction {
            Direction::Upload => self.destination_address,
            Direction::Download => self.source_address,
        }
    }

    fn remote_port(&self, direction: Direction) -> Option<u16> {
        match direction {
            Direction::Upload => self.destination_port,
            Direction::Download => self.source_port,
        }
    }
}

fn record_upload_packet_diagnostics(packet: &[u8]) {
    let Some(parsed) = ParsedPacket::parse(packet) else {
        return;
    };
    if parsed.tcp_flags.contains(&TcpFlag::Syn) {
        RUNTIME_STATS
            .upload_tcp_syn_packets
            .fetch_add(1, AtomicOrdering::Relaxed);
    }
    if parsed.tcp_flags.contains(&TcpFlag::Ack) {
        RUNTIME_STATS
            .upload_tcp_ack_packets
            .fetch_add(1, AtomicOrdering::Relaxed);
    }
}

fn record_download_packet_diagnostics(packet: &[u8]) {
    let Some(parsed) = ParsedPacket::parse(packet) else {
        return;
    };
    if parsed.tcp_flags.contains(&TcpFlag::SynAck) {
        RUNTIME_STATS
            .download_tcp_syn_ack_packets
            .fetch_add(1, AtomicOrdering::Relaxed);
    }

    let declared_length = match parsed.ip_version {
        IpVersion::V4 if packet.len() >= 20 => {
            usize::from(u16::from_be_bytes([packet[2], packet[3]]))
        }
        IpVersion::V6 if packet.len() >= 40 => {
            40 + usize::from(u16::from_be_bytes([packet[4], packet[5]]))
        }
        _ => packet.len(),
    };
    if declared_length != packet.len() {
        RUNTIME_STATS
            .download_ip_length_mismatches
            .fetch_add(1, AtomicOrdering::Relaxed);
    }

    if parsed.ip_version == IpVersion::V4 {
        let header_length = usize::from(packet[0] & 0x0f) * 4;
        if header_length > packet.len() || checksum(&packet[..header_length]) != 0 {
            RUNTIME_STATS
                .download_ip_checksum_failures
                .fetch_add(1, AtomicOrdering::Relaxed);
        }
    }

    if !transport_checksum_is_valid(packet, &parsed) {
        RUNTIME_STATS
            .download_transport_checksum_failures
            .fetch_add(1, AtomicOrdering::Relaxed);
    }
}

fn transport_checksum_is_valid(packet: &[u8], metadata: &ParsedPacket) -> bool {
    if metadata.transport == TransportProtocol::Other {
        return true;
    }
    let checksum_offset = match metadata.transport {
        TransportProtocol::Tcp => metadata.transport_offset + 16,
        TransportProtocol::Udp => metadata.transport_offset + 6,
        TransportProtocol::Other => return true,
    };
    if checksum_offset + 2 > packet.len() {
        return false;
    }
    if metadata.transport == TransportProtocol::Udp
        && metadata.ip_version == IpVersion::V4
        && packet[checksum_offset..checksum_offset + 2] == [0, 0]
    {
        return true;
    }

    let segment = &packet[metadata.transport_offset..];
    let mut pseudo = Vec::with_capacity(40 + segment.len());
    match metadata.ip_version {
        IpVersion::V4 => {
            pseudo.extend_from_slice(&packet[12..20]);
            pseudo.push(0);
            pseudo.push(packet[9]);
            let Ok(length) = u16::try_from(segment.len()) else {
                return false;
            };
            pseudo.extend_from_slice(&length.to_be_bytes());
        }
        IpVersion::V6 => {
            pseudo.extend_from_slice(&packet[8..40]);
            let Ok(length) = u32::try_from(segment.len()) else {
                return false;
            };
            pseudo.extend_from_slice(&length.to_be_bytes());
            pseudo.extend_from_slice(&[0, 0, 0, packet[6]]);
        }
    }
    pseudo.extend_from_slice(segment);
    checksum(&pseudo) == 0
}

fn clamp_existing_tcp_mss(
    packet: &mut [u8],
    metadata: &ParsedPacket,
    action: PathMtuAction,
) -> bool {
    let PathMtuAction::ClampMss(mss) = action else {
        return false;
    };
    if metadata.transport != TransportProtocol::Tcp {
        return false;
    }
    let header_end = metadata.payload_range.start;
    let mut offset = metadata.transport_offset + 20;
    while offset < header_end {
        match packet[offset] {
            0 => break,
            1 => offset += 1,
            2 if offset + 4 <= header_end && packet[offset + 1] == 4 => {
                let changed = packet[offset + 2..offset + 4] != mss.to_be_bytes();
                if changed {
                    packet[offset + 2..offset + 4].copy_from_slice(&mss.to_be_bytes());
                }
                return changed;
            }
            _ if offset + 1 < header_end => {
                let length = usize::from(packet[offset + 1]);
                if length < 2 || offset + length > header_end {
                    break;
                }
                offset += length;
            }
            _ => break,
        }
    }
    false
}

/// 将一个尚未分片的 IPv4 数据报拆成不超过 `mtu` 的标准分片。
///
/// 除最后一片外，负载长度必须是 8 的倍数；传输层校验和属于重组后的完整报文，
/// 因此这里仅重算各分片的 IPv4 头校验和，不能改写 TCP/UDP 校验和。
fn fragment_ipv4_packet(packet: &[u8], mtu: u16) -> Option<Vec<Vec<u8>>> {
    if packet.len() < 20 || packet[0] >> 4 != 4 {
        return None;
    }
    let header_len = usize::from(packet[0] & 0x0f) * 4;
    let mtu = usize::from(mtu);
    if header_len < 20 || header_len >= mtu || packet.len() <= mtu {
        return None;
    }
    let flags_offset = u16::from_be_bytes([packet[6], packet[7]]);
    // 已经带非零 offset 的包不再次分片；数据面保持 fail-open 并暴露未实施计数。
    if flags_offset & 0x1fff != 0 {
        return None;
    }
    let fragment_payload_len = ((mtu - header_len) / 8) * 8;
    if fragment_payload_len == 0 {
        return None;
    }
    let payload = &packet[header_len..];
    let mut fragments = Vec::new();
    for (index, chunk) in payload.chunks(fragment_payload_len).enumerate() {
        let offset_bytes = index.checked_mul(fragment_payload_len)?;
        let offset_units = u16::try_from(offset_bytes / 8).ok()?;
        let more_fragments = offset_bytes + chunk.len() < payload.len();
        let mut fragment = Vec::with_capacity(header_len + chunk.len());
        fragment.extend_from_slice(&packet[..header_len]);
        fragment.extend_from_slice(chunk);
        let total_len = u16::try_from(fragment.len()).ok()?;
        fragment[2..4].copy_from_slice(&total_len.to_be_bytes());
        // 保留 reserved 位，清除 DF；除最后一片外设置 MF。
        let mut new_flags_offset = flags_offset & 0x8000;
        if more_fragments {
            new_flags_offset |= 0x2000;
        }
        new_flags_offset |= offset_units & 0x1fff;
        fragment[6..8].copy_from_slice(&new_flags_offset.to_be_bytes());
        fragment[10..12].fill(0);
        let header_checksum = checksum(&fragment[..header_len]);
        fragment[10..12].copy_from_slice(&header_checksum.to_be_bytes());
        fragments.push(fragment);
    }
    (fragments.len() > 1).then_some(fragments)
}

/// 构造 `ICMPv4` Destination Unreachable / Fragmentation Needed（Type 3 Code 4）。
fn build_icmpv4_fragmentation_needed(packet: &[u8], mtu: u16) -> Option<Vec<u8>> {
    if packet.len() < 20 || packet[0] >> 4 != 4 {
        return None;
    }
    let original_header_len = usize::from(packet[0] & 0x0f) * 4;
    if original_header_len < 20 || original_header_len > packet.len() {
        return None;
    }
    let quote_len = packet.len().min(original_header_len.saturating_add(8));
    let total_len = 20_usize.checked_add(8)?.checked_add(quote_len)?;
    let total_len_u16 = u16::try_from(total_len).ok()?;
    let mut response = vec![0_u8; total_len];
    response[0] = 0x45;
    response[2..4].copy_from_slice(&total_len_u16.to_be_bytes());
    response[8] = 64;
    response[9] = 1;
    // ICMP 由路径节点返回原发送方；在透明模拟中使用原目的地址作为节点地址。
    response[12..16].copy_from_slice(&packet[16..20]);
    response[16..20].copy_from_slice(&packet[12..16]);
    response[20] = 3;
    response[21] = 4;
    response[26..28].copy_from_slice(&mtu.to_be_bytes());
    response[28..].copy_from_slice(&packet[..quote_len]);
    let icmp_checksum = checksum(&response[20..]);
    response[22..24].copy_from_slice(&icmp_checksum.to_be_bytes());
    let ip_checksum = checksum(&response[..20]);
    response[10..12].copy_from_slice(&ip_checksum.to_be_bytes());
    Some(response)
}

/// 构造 `ICMPv6` Packet Too Big（Type 2 Code 0）。
fn build_icmpv6_packet_too_big(packet: &[u8], mtu: u16) -> Option<Vec<u8>> {
    if packet.len() < 40 || packet[0] >> 4 != 6 {
        return None;
    }
    // IPv6 要求尽量引用触发包，但整个 ICMPv6 报文不得超过最小 IPv6 MTU 1280。
    let quote_len = packet.len().min(1_280 - 40 - 8);
    let payload_len = 8_usize.checked_add(quote_len)?;
    let payload_len_u16 = u16::try_from(payload_len).ok()?;
    let mut response = vec![0_u8; 40 + payload_len];
    response[0] = 0x60;
    response[4..6].copy_from_slice(&payload_len_u16.to_be_bytes());
    response[6] = 58;
    response[7] = 64;
    response[8..24].copy_from_slice(&packet[24..40]);
    response[24..40].copy_from_slice(&packet[8..24]);
    response[40] = 2;
    response[41] = 0;
    response[44..48].copy_from_slice(&u32::from(mtu).to_be_bytes());
    response[48..].copy_from_slice(&packet[..quote_len]);

    let mut pseudo = Vec::with_capacity(40 + payload_len);
    pseudo.extend_from_slice(&response[8..40]);
    pseudo.extend_from_slice(&u32::try_from(payload_len).ok()?.to_be_bytes());
    pseudo.extend_from_slice(&[0, 0, 0, 58]);
    pseudo.extend_from_slice(&response[40..]);
    let icmp_checksum = checksum(&pseudo);
    response[42..44].copy_from_slice(&icmp_checksum.to_be_bytes());
    Some(response)
}

fn refresh_checksums(packet: &mut [u8], metadata: &ParsedPacket) {
    match metadata.ip_version {
        IpVersion::V4 => {
            let header_len = usize::from(packet[0] & 0x0f) * 4;
            packet[10] = 0;
            packet[11] = 0;
            let checksum = checksum(&packet[..header_len]);
            packet[10..12].copy_from_slice(&checksum.to_be_bytes());
            refresh_transport_checksum_v4(packet, metadata);
        }
        IpVersion::V6 => refresh_transport_checksum_v6(packet, metadata),
    }
}

fn refresh_transport_checksum_v4(packet: &mut [u8], metadata: &ParsedPacket) {
    let checksum_offset = match metadata.transport {
        TransportProtocol::Tcp => metadata.transport_offset + 16,
        TransportProtocol::Udp => metadata.transport_offset + 6,
        TransportProtocol::Other => return,
    };
    if checksum_offset + 2 > packet.len() || packet.len() < 20 {
        return;
    }
    packet[checksum_offset] = 0;
    packet[checksum_offset + 1] = 0;
    let segment_len = packet.len().saturating_sub(metadata.transport_offset);
    let Ok(segment_len) = u16::try_from(segment_len) else {
        return;
    };
    let mut pseudo = Vec::with_capacity(12 + usize::from(segment_len));
    pseudo.extend_from_slice(&packet[12..20]);
    pseudo.push(0);
    pseudo.push(packet[9]);
    pseudo.extend_from_slice(&segment_len.to_be_bytes());
    pseudo.extend_from_slice(&packet[metadata.transport_offset..]);
    let value = checksum(&pseudo);
    packet[checksum_offset..checksum_offset + 2].copy_from_slice(&value.to_be_bytes());
}

fn refresh_transport_checksum_v6(packet: &mut [u8], metadata: &ParsedPacket) {
    let checksum_offset = match metadata.transport {
        TransportProtocol::Tcp => metadata.transport_offset + 16,
        TransportProtocol::Udp => metadata.transport_offset + 6,
        TransportProtocol::Other => return,
    };
    if checksum_offset + 2 > packet.len() || packet.len() < 40 {
        return;
    }
    packet[checksum_offset] = 0;
    packet[checksum_offset + 1] = 0;
    let Ok(segment_len) = u32::try_from(packet.len().saturating_sub(metadata.transport_offset))
    else {
        return;
    };
    let mut pseudo = Vec::with_capacity(40 + segment_len as usize);
    pseudo.extend_from_slice(&packet[8..40]);
    pseudo.extend_from_slice(&segment_len.to_be_bytes());
    pseudo.extend_from_slice(&[0, 0, 0, packet[6]]);
    pseudo.extend_from_slice(&packet[metadata.transport_offset..]);
    let value = checksum(&pseudo);
    packet[checksum_offset..checksum_offset + 2].copy_from_slice(&value.to_be_bytes());
}

fn checksum(bytes: &[u8]) -> u16 {
    let mut sum = 0_u32;
    for pair in bytes.chunks(2) {
        let word = if pair.len() == 2 {
            u16::from_be_bytes([pair[0], pair[1]])
        } else {
            u16::from(pair[0]) << 8
        };
        sum = sum.wrapping_add(u32::from(word));
        while sum > 0xffff {
            sum = (sum & 0xffff) + (sum >> 16);
        }
    }
    !u16::try_from(sum).unwrap_or(u16::MAX)
}

async fn run_socks_server<P>(
    listener: TcpListener,
    protector: P,
    proxy_routes: Arc<ProxyRouteTable>,
    cancellation: CancellationToken,
    runtime_epoch: u64,
) -> io::Result<()>
where
    P: SocketProtection,
{
    loop {
        let (stream, _) = tokio::select! {
            () = cancellation.cancelled() => return Ok(()),
            accepted = listener.accept() => accepted?,
        };
        increment_for_epoch(runtime_epoch, &RUNTIME_STATS.socks_clients);
        let protector = protector.clone();
        let proxy_routes = proxy_routes.clone();
        tokio::spawn(async move {
            // 单个目标不可达、远端拒绝连接或客户端主动断开只是该 SOCKS 会话的结果，
            // 不能标记成整个 VPN 数据面故障。CONNECT 会向客户端返回失败状态；运行
            // 统计仍可通过 attempts 与 successes 的差值观察失败会话。
            let _ = handle_socks_client(stream, protector, proxy_routes, runtime_epoch).await;
        });
    }
}

async fn handle_socks_client<P>(
    mut client: TcpStream,
    protector: P,
    proxy_routes: Arc<ProxyRouteTable>,
    runtime_epoch: u64,
) -> io::Result<()>
where
    P: SocketProtection,
{
    let mut greeting = [0_u8; 2];
    client.read_exact(&mut greeting).await?;
    if greeting[0] != SOCKS_VERSION || greeting[1] == 0 {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "SOCKS5 greeting 无效",
        ));
    }
    let mut methods = vec![0_u8; usize::from(greeting[1])];
    client.read_exact(&mut methods).await?;
    if !methods.contains(&0) {
        client.write_all(&[SOCKS_VERSION, 0xff]).await?;
        return Ok(());
    }
    client.write_all(&[SOCKS_VERSION, 0]).await?;

    let mut request = [0_u8; 4];
    client.read_exact(&mut request).await?;
    if request[0] != SOCKS_VERSION || request[2] != 0 {
        write_socks_reply(&mut client, SOCKS_GENERAL_FAILURE, None).await?;
        return Ok(());
    }
    let target = read_socks_target(&mut client, request[3]).await?;
    match request[1] {
        1 => handle_socks_connect(client, target, protector, &proxy_routes, runtime_epoch).await,
        3 => handle_socks_udp_associate(client, protector).await,
        _ => {
            write_socks_reply(&mut client, SOCKS_COMMAND_NOT_SUPPORTED, None).await?;
            Ok(())
        }
    }
}

async fn handle_socks_connect<P>(
    mut client: TcpStream,
    target: SocksTarget,
    protector: P,
    proxy_routes: &ProxyRouteTable,
    runtime_epoch: u64,
) -> io::Result<()>
where
    P: SocketProtection,
{
    let addresses = if let Some(addresses) = target.proxy_addresses(proxy_routes) {
        addresses.to_vec()
    } else {
        target.resolve().await?
    };
    let mut last_error = None;
    for address in addresses {
        increment_for_epoch(runtime_epoch, &RUNTIME_STATS.socks_connect_attempts);
        match connect_protected(address, &protector, runtime_epoch).await {
            Ok(mut upstream) => {
                increment_for_epoch(runtime_epoch, &RUNTIME_STATS.socks_connect_successes);
                write_socks_reply(&mut client, SOCKS_SUCCEEDED, upstream.local_addr().ok()).await?;
                let _ = tokio::io::copy_bidirectional(&mut client, &mut upstream).await?;
                return Ok(());
            }
            Err(error) => last_error = Some(error),
        }
    }
    write_socks_reply(&mut client, SOCKS_GENERAL_FAILURE, None).await?;
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
        increment_for_epoch(runtime_epoch, &RUNTIME_STATS.protect_failures);
        return Err(error);
    }
    socket.connect(address).await
}

async fn handle_socks_udp_associate<P>(mut control: TcpStream, protector: P) -> io::Result<()>
where
    P: SocketProtection,
{
    let standard_v4 = std::net::UdpSocket::bind((Ipv4Addr::UNSPECIFIED, 0))?;
    standard_v4.set_nonblocking(true)?;
    protector.protect(standard_v4.as_raw_fd())?;
    let udp_v4 = UdpSocket::from_std(standard_v4)?;

    // 客户端（tun2proxy）始终通过 loopback IPv4 访问 BND.ADDR；IPv6 外连使用单独的
    // 受保护 socket，避免 0.0.0.0 回包不可达，也避免把 IPv6 UDP 静默降级为 IPv4。
    let standard_v6 = std::net::UdpSocket::bind((Ipv6Addr::UNSPECIFIED, 0))?;
    standard_v6.set_nonblocking(true)?;
    protector.protect(standard_v6.as_raw_fd())?;
    let udp_v6 = UdpSocket::from_std(standard_v6)?;
    let bind_address =
        SocketAddr::new(IpAddr::V4(Ipv4Addr::LOCALHOST), udp_v4.local_addr()?.port());
    write_socks_reply(&mut control, SOCKS_SUCCEEDED, Some(bind_address)).await?;

    let mut client_address = None;
    let mut datagram = vec![0_u8; MAX_IP_PACKET_SIZE];
    let mut datagram_v6 = vec![0_u8; MAX_IP_PACKET_SIZE];
    let mut control_probe = [0_u8; 1];
    loop {
        tokio::select! {
            read = control.read(&mut control_probe) => {
                if read? == 0 {
                    return Ok(());
                }
            }
            received = udp_v4.recv_from(&mut datagram) => {
                let (size, source) = received?;
                if client_address.is_none() || client_address == Some(source) {
                    client_address = Some(source);
                    if let Ok((target, payload_offset)) = parse_socks_udp_request(&datagram[..size]) {
                        for destination in target.resolve().await? {
                            let result = if destination.is_ipv4() {
                                udp_v4.send_to(&datagram[payload_offset..size], destination).await
                            } else {
                                udp_v6.send_to(&datagram[payload_offset..size], destination).await
                            };
                            if result.is_ok() {
                                break;
                            }
                        }
                    }
                } else if let Some(client) = client_address {
                    let response = encode_socks_udp_response(source, &datagram[..size]);
                    udp_v4.send_to(&response, client).await?;
                }
            }
            received = udp_v6.recv_from(&mut datagram_v6) => {
                let (size, source) = received?;
                if let Some(client) = client_address {
                    let response = encode_socks_udp_response(source, &datagram_v6[..size]);
                    udp_v4.send_to(&response, client).await?;
                }
            }
        }
    }
}

#[derive(Clone, Debug)]
enum SocksTarget {
    Address(SocketAddr),
    Domain(String, u16),
}

impl SocksTarget {
    fn proxy_addresses<'a>(&self, routes: &'a ProxyRouteTable) -> Option<&'a [SocketAddr]> {
        match self {
            Self::Address(address) => routes.for_ip(address.ip(), address.port()),
            Self::Domain(domain, port) => routes.for_domain(domain, *port),
        }
    }

    async fn resolve(&self) -> io::Result<Vec<SocketAddr>> {
        match self {
            Self::Address(address) => Ok(vec![*address]),
            Self::Domain(domain, port) => Ok(tokio::net::lookup_host((domain.as_str(), *port))
                .await?
                .collect()),
        }
    }
}

async fn read_socks_target(client: &mut TcpStream, address_type: u8) -> io::Result<SocksTarget> {
    match address_type {
        1 => {
            let mut bytes = [0_u8; 6];
            client.read_exact(&mut bytes).await?;
            Ok(SocksTarget::Address(SocketAddr::new(
                IpAddr::V4(Ipv4Addr::new(bytes[0], bytes[1], bytes[2], bytes[3])),
                u16::from_be_bytes([bytes[4], bytes[5]]),
            )))
        }
        3 => {
            let length = client.read_u8().await?;
            let mut domain = vec![0_u8; usize::from(length)];
            client.read_exact(&mut domain).await?;
            let port = client.read_u16().await?;
            Ok(SocksTarget::Domain(
                String::from_utf8(domain).map_err(|_| {
                    io::Error::new(io::ErrorKind::InvalidData, "SOCKS5 域名非 UTF-8")
                })?,
                port,
            ))
        }
        4 => {
            let mut bytes = [0_u8; 18];
            client.read_exact(&mut bytes).await?;
            let mut ip = [0_u8; 16];
            ip.copy_from_slice(&bytes[..16]);
            Ok(SocksTarget::Address(SocketAddr::new(
                IpAddr::V6(Ipv6Addr::from(ip)),
                u16::from_be_bytes([bytes[16], bytes[17]]),
            )))
        }
        _ => Err(io::Error::new(
            io::ErrorKind::InvalidData,
            format!("SOCKS5 地址类型不支持：{address_type}"),
        )),
    }
}

fn parse_socks_udp_request(datagram: &[u8]) -> io::Result<(SocksTarget, usize)> {
    if datagram.len() < 4 || datagram[0..2] != [0, 0] || datagram[2] != 0 {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "SOCKS5 UDP header 无效",
        ));
    }
    match datagram[3] {
        1 if datagram.len() >= 10 => Ok((
            SocksTarget::Address(SocketAddr::new(
                IpAddr::V4(Ipv4Addr::new(
                    datagram[4],
                    datagram[5],
                    datagram[6],
                    datagram[7],
                )),
                u16::from_be_bytes([datagram[8], datagram[9]]),
            )),
            10,
        )),
        3 if datagram.len() >= 5 => {
            let length = usize::from(datagram[4]);
            let end = 5 + length;
            if datagram.len() < end + 2 {
                return Err(io::Error::new(
                    io::ErrorKind::UnexpectedEof,
                    "SOCKS5 UDP 域名不完整",
                ));
            }
            Ok((
                SocksTarget::Domain(
                    String::from_utf8(datagram[5..end].to_vec()).map_err(|_| {
                        io::Error::new(io::ErrorKind::InvalidData, "SOCKS5 UDP 域名非 UTF-8")
                    })?,
                    u16::from_be_bytes([datagram[end], datagram[end + 1]]),
                ),
                end + 2,
            ))
        }
        4 if datagram.len() >= 22 => {
            let mut ip = [0_u8; 16];
            ip.copy_from_slice(&datagram[4..20]);
            Ok((
                SocksTarget::Address(SocketAddr::new(
                    IpAddr::V6(Ipv6Addr::from(ip)),
                    u16::from_be_bytes([datagram[20], datagram[21]]),
                )),
                22,
            ))
        }
        _ => Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "SOCKS5 UDP 地址类型不支持",
        )),
    }
}

fn encode_socks_udp_response(source: SocketAddr, payload: &[u8]) -> Vec<u8> {
    let mut response = vec![0, 0, 0];
    match source.ip() {
        IpAddr::V4(ip) => {
            response.push(1);
            response.extend_from_slice(&ip.octets());
        }
        IpAddr::V6(ip) => {
            response.push(4);
            response.extend_from_slice(&ip.octets());
        }
    }
    response.extend_from_slice(&source.port().to_be_bytes());
    response.extend_from_slice(payload);
    response
}

async fn write_socks_reply(
    stream: &mut TcpStream,
    status: u8,
    address: Option<SocketAddr>,
) -> io::Result<()> {
    let mut reply = vec![SOCKS_VERSION, status, 0];
    match address.unwrap_or_else(|| SocketAddr::new(IpAddr::V4(Ipv4Addr::UNSPECIFIED), 0)) {
        SocketAddr::V4(address) => {
            reply.push(1);
            reply.extend_from_slice(&address.ip().octets());
            reply.extend_from_slice(&address.port().to_be_bytes());
        }
        SocketAddr::V6(address) => {
            reply.push(4);
            reply.extend_from_slice(&address.ip().octets());
            reply.extend_from_slice(&address.port().to_be_bytes());
        }
    }
    stream.write_all(&reply).await
}

fn read_u16(bytes: &[u8], offset: usize) -> Option<u16> {
    Some(u16::from_be_bytes([
        *bytes.get(offset)?,
        *bytes.get(offset + 1)?,
    ]))
}

#[cfg(test)]
mod tests {
    use std::sync::atomic::{AtomicUsize, Ordering as AtomicOrdering};

    use crate::{
        DestinationTarget, InstalledApplication, NetworkProfile, ProxyRoute,
        ProxyRuntimeConfiguration, ResolvedProxyRoute, TargetApplication, WeakNetworkProfile,
    };

    use super::*;

    #[derive(Clone, Debug, Default)]
    struct RecordingProtection {
        calls: Arc<AtomicUsize>,
    }

    impl SocketProtection for RecordingProtection {
        fn protect(&self, _fd: i32) -> io::Result<()> {
            self.calls.fetch_add(1, AtomicOrdering::SeqCst);
            Ok(())
        }

        fn notify_failure(&self, _message: &str) {}
    }

    #[test]
    fn parses_ipv4_tcp_metadata() {
        let mut packet = vec![0_u8; 40];
        packet[0] = 0x45;
        packet[9] = 6;
        packet[12..16].copy_from_slice(&[10, 0, 0, 2]);
        packet[16..20].copy_from_slice(&[203, 0, 113, 10]);
        packet[20..22].copy_from_slice(&52_000_u16.to_be_bytes());
        packet[20 + 2..20 + 4].copy_from_slice(&443_u16.to_be_bytes());
        packet[20 + 12] = 5 << 4;
        packet[20 + 13] = 0x12;
        let parsed = ParsedPacket::parse(&packet).expect("应解析 IPv4 TCP");
        assert_eq!(parsed.destination_port, Some(443));
        assert_eq!(
            parsed.remote_address(Direction::Upload),
            "203.0.113.10".parse::<IpAddr>().unwrap()
        );
        assert_eq!(parsed.remote_port(Direction::Upload), Some(443));
        assert_eq!(
            parsed.remote_address(Direction::Download),
            "10.0.0.2".parse::<IpAddr>().unwrap()
        );
        assert_eq!(parsed.remote_port(Direction::Download), Some(52_000));
        assert!(parsed.tcp_flags.contains(&TcpFlag::SynAck));
    }

    #[test]
    fn parses_ipv6_udp_remote_metadata_in_both_directions() {
        let mut packet = vec![0_u8; 48];
        packet[0] = 0x60;
        packet[6] = 17;
        packet[8..24].copy_from_slice(&"2001:db8::2".parse::<Ipv6Addr>().unwrap().octets());
        packet[24..40].copy_from_slice(&"2001:db8:1::53".parse::<Ipv6Addr>().unwrap().octets());
        packet[40..42].copy_from_slice(&53_000_u16.to_be_bytes());
        packet[42..44].copy_from_slice(&53_u16.to_be_bytes());
        let parsed = ParsedPacket::parse(&packet).expect("应解析 IPv6 UDP");
        assert_eq!(
            parsed.remote_address(Direction::Upload),
            "2001:db8:1::53".parse::<IpAddr>().unwrap()
        );
        assert_eq!(parsed.remote_port(Direction::Upload), Some(53));
        assert_eq!(
            parsed.remote_address(Direction::Download),
            "2001:db8::2".parse::<IpAddr>().unwrap()
        );
        assert_eq!(parsed.remote_port(Direction::Download), Some(53_000));
    }

    #[test]
    fn encodes_ipv6_udp_response() {
        let source = "[2001:db8::1]:53".parse().expect("地址有效");
        let response = encode_socks_udp_response(source, b"dns");
        assert_eq!(&response[..4], &[0, 0, 0, 4]);
        assert_eq!(&response[response.len() - 3..], b"dns");
    }

    #[tokio::test]
    async fn scheduler_preserves_sequence_without_reorder() {
        let (sender, receiver) = mpsc::channel(8);
        let (observed_sender, mut observed_receiver) = mpsc::channel(8);
        let due = TokioInstant::now();
        for sequence in 0..3_u64 {
            sender
                .send(ScheduledPacket {
                    due,
                    sequence,
                    copies: 1,
                    route: PacketRoute::Forward,
                    packet: vec![u8::try_from(sequence).expect("测试序号可放入 u8")],
                })
                .await
                .expect("调度输入存在");
        }
        drop(sender);

        run_packet_scheduler(receiver, CancellationToken::new(), move |route, packet| {
            let observed_sender = observed_sender.clone();
            async move {
                assert_eq!(route, PacketRoute::Forward);
                observed_sender
                    .send(packet)
                    .await
                    .map_err(|error| io::Error::other(error.to_string()))
            }
        })
        .await
        .expect("调度成功");

        let mut observed = Vec::new();
        while let Some(packet) = observed_receiver.recv().await {
            observed.push(packet[0]);
        }
        assert_eq!(observed, vec![0, 1, 2]);
    }

    #[test]
    fn fragments_ipv4_packet_to_requested_path_mtu() {
        let packet = test_ipv4_packet(1_500);
        let fragments = fragment_ipv4_packet(&packet, 576).expect("IPv4 应完成分片");

        assert_eq!(fragments.len(), 3);
        assert!(fragments.iter().all(|fragment| fragment.len() <= 576));
        assert!(
            fragments
                .iter()
                .all(|fragment| checksum(&fragment[..20]) == 0)
        );
        assert_eq!(
            u16::from_be_bytes([fragments[0][6], fragments[0][7]]),
            0x2000
        );
        assert_eq!(
            u16::from_be_bytes([fragments[1][6], fragments[1][7]]),
            0x2045
        );
        assert_eq!(
            u16::from_be_bytes([fragments[2][6], fragments[2][7]]),
            0x008a
        );
        let reassembled: Vec<u8> = fragments
            .iter()
            .flat_map(|fragment| fragment[20..].iter().copied())
            .collect();
        assert_eq!(reassembled, packet[20..]);
    }

    #[test]
    fn builds_valid_icmpv4_fragmentation_needed() {
        let packet = test_ipv4_packet(1_500);
        let signal = build_icmpv4_fragmentation_needed(&packet, 576)
            .expect("IPv4 应生成 Fragmentation Needed");

        assert_eq!(&signal[12..16], &packet[16..20]);
        assert_eq!(&signal[16..20], &packet[12..16]);
        assert_eq!(&signal[20..22], &[3, 4]);
        assert_eq!(u16::from_be_bytes([signal[26], signal[27]]), 576);
        assert_eq!(checksum(&signal[..20]), 0);
        assert_eq!(checksum(&signal[20..]), 0);
        assert_eq!(&signal[28..], &packet[..28]);
    }

    #[test]
    fn builds_valid_icmpv6_packet_too_big() {
        let packet = test_ipv6_packet(1_500);
        let signal =
            build_icmpv6_packet_too_big(&packet, 1_280).expect("IPv6 应生成 Packet Too Big");

        assert_eq!(&signal[8..24], &packet[24..40]);
        assert_eq!(&signal[24..40], &packet[8..24]);
        assert_eq!(&signal[40..42], &[2, 0]);
        assert_eq!(
            u32::from_be_bytes(signal[44..48].try_into().unwrap()),
            1_280
        );
        let payload_len = u32::try_from(signal.len() - 40).unwrap();
        let mut pseudo = Vec::new();
        pseudo.extend_from_slice(&signal[8..40]);
        pseudo.extend_from_slice(&payload_len.to_be_bytes());
        pseudo.extend_from_slice(&[0, 0, 0, 58]);
        pseudo.extend_from_slice(&signal[40..]);
        assert_eq!(checksum(&pseudo), 0);
    }

    #[test]
    fn pmtu_construction_failure_is_an_explicit_data_plane_error() {
        let packet = test_ipv6_packet(1_500);
        let mut decision = crate::PacketDecision::pass(&[]);
        decision.path_mtu_action = PathMtuAction::FragmentIpv4(576);

        let error = prepare_path_mtu_packets(&packet, &decision, 1)
            .expect_err("IPv4 分片动作不得静默转发 IPv6 原包");

        assert!(
            error
                .to_string()
                .contains("无法为当前 IP 包执行路径 MTU 动作")
        );
        assert!(error.to_string().contains("恢复系统直连"));
    }

    #[test]
    fn data_plane_drop_cancels_and_reaps_cooperative_thread() {
        let cancellation = CancellationToken::new();
        let thread_cancellation = cancellation.clone();
        let (finished_tx, finished_rx) = sync_mpsc::sync_channel(1);
        let (observed_tx, observed_rx) = sync_mpsc::sync_channel(1);
        let runtime_thread = thread::spawn(move || {
            let _finished = ThreadFinishedNotifier(Some(finished_tx));
            while !thread_cancellation.is_cancelled() {
                thread::sleep(Duration::from_millis(1));
            }
            let _ = observed_tx.send(());
        });
        let handle = DataPlaneHandle {
            runtime_epoch: 0,
            cancellation,
            thread: Some(runtime_thread),
            thread_finished: finished_rx,
            tun_release: None,
        };

        drop(handle);

        observed_rx
            .recv_timeout(Duration::from_secs(1))
            .expect("Drop 应取消并回收协作线程");
    }

    #[test]
    fn data_plane_shutdown_releases_tun_before_its_wait_bound() {
        let (tun, mut peer) = std::os::unix::net::UnixStream::pair().expect("创建测试 TUN 对");
        peer.set_read_timeout(Some(Duration::from_secs(1)))
            .expect("设置读取超时");
        let (managed_tun, tun_release) = ManagedTunFile::new(tun.into()).expect("接管测试 TUN");
        let cancellation = CancellationToken::new();
        let (finished_tx, finished_rx) = sync_mpsc::sync_channel(1);
        let (release_tx, release_rx) = sync_mpsc::sync_channel(1);
        let (observed_tx, observed_rx) = sync_mpsc::sync_channel(1);
        let runtime_thread = thread::spawn(move || {
            let _finished = ThreadFinishedNotifier(Some(finished_tx));
            let _tun = managed_tun;
            let _ = release_rx.recv();
            let _ = observed_tx.send(());
        });
        let mut handle = DataPlaneHandle {
            runtime_epoch: 0,
            cancellation,
            thread: Some(runtime_thread),
            thread_finished: finished_rx,
            tun_release: Some(tun_release),
        };
        let started = Instant::now();

        handle.shutdown_with_timeout(Duration::from_millis(20));

        assert!(started.elapsed() < Duration::from_millis(250));
        let mut byte = [0_u8; 1];
        assert_eq!(
            peer.read(&mut byte).expect("停止后对端应观察到 TUN 关闭"),
            0,
            "即使运行线程不退出，Rust 持有的 TUN 引用也必须先释放"
        );
        release_tx.send(()).expect("释放已分离线程");
        observed_rx
            .recv_timeout(Duration::from_secs(1))
            .expect("分离线程仍应自行结束并释放资源");
    }

    fn test_ipv4_packet(total_len: usize) -> Vec<u8> {
        let mut packet = vec![0x5a; total_len];
        packet[0] = 0x45;
        packet[1] = 0;
        packet[2..4].copy_from_slice(&u16::try_from(total_len).unwrap().to_be_bytes());
        packet[4..6].copy_from_slice(&0x1234_u16.to_be_bytes());
        packet[6..8].copy_from_slice(&0x4000_u16.to_be_bytes());
        packet[8] = 64;
        packet[9] = 17;
        packet[10..12].fill(0);
        packet[12..16].copy_from_slice(&[10, 0, 0, 2]);
        packet[16..20].copy_from_slice(&[203, 0, 113, 10]);
        let value = checksum(&packet[..20]);
        packet[10..12].copy_from_slice(&value.to_be_bytes());
        packet
    }

    fn test_ipv6_packet(total_len: usize) -> Vec<u8> {
        let mut packet = vec![0x6b; total_len];
        packet[0] = 0x60;
        packet[4..6].copy_from_slice(&u16::try_from(total_len - 40).unwrap().to_be_bytes());
        packet[6] = 17;
        packet[7] = 64;
        packet[8..24].copy_from_slice(&"2001:db8::2".parse::<Ipv6Addr>().unwrap().octets());
        packet[24..40].copy_from_slice(&"2001:db8::10".parse::<Ipv6Addr>().unwrap().octets());
        packet
    }

    #[tokio::test]
    async fn socks_connect_protects_outbound_socket_before_relay() {
        let upstream = TcpListener::bind((Ipv4Addr::LOCALHOST, 0))
            .await
            .expect("启动 TCP echo");
        let upstream_address = upstream.local_addr().expect("读取 TCP echo 地址");
        let upstream_task = tokio::spawn(async move {
            let (mut stream, _) = upstream.accept().await.expect("接收 TCP");
            let mut payload = [0_u8; 4];
            stream
                .read_exact(&mut payload)
                .await
                .expect("读取 TCP payload");
            stream.write_all(&payload).await.expect("回写 TCP payload");
        });

        let protector = RecordingProtection::default();
        let calls = protector.calls.clone();
        let listener = TcpListener::bind((Ipv4Addr::LOCALHOST, 0))
            .await
            .expect("启动测试 SOCKS5");
        let address = listener.local_addr().expect("读取 SOCKS5 地址");
        let server_task = tokio::spawn(async move {
            let (stream, _) = listener.accept().await.expect("接收 SOCKS5");
            handle_socks_client(stream, protector, Arc::new(ProxyRouteTable::default()), 0)
                .await
                .expect("SOCKS5 CONNECT 成功");
        });

        let mut client = TcpStream::connect(address).await.expect("连接 SOCKS5");
        socks_no_auth(&mut client).await;
        let SocketAddr::V4(upstream_v4) = upstream_address else {
            panic!("测试上游应为 IPv4");
        };
        let mut request = vec![5, 1, 0, 1];
        request.extend_from_slice(&upstream_v4.ip().octets());
        request.extend_from_slice(&upstream_v4.port().to_be_bytes());
        client.write_all(&request).await.expect("发送 CONNECT");
        let mut reply = [0_u8; 10];
        client
            .read_exact(&mut reply)
            .await
            .expect("读取 CONNECT reply");
        assert_eq!(reply[1], SOCKS_SUCCEEDED);
        client.write_all(b"ping").await.expect("写入 relay");
        let mut echoed = [0_u8; 4];
        client.read_exact(&mut echoed).await.expect("读取 relay");
        assert_eq!(&echoed, b"ping");
        assert_eq!(calls.load(AtomicOrdering::SeqCst), 1);
        drop(client);
        upstream_task.await.expect("TCP echo 任务结束");
        server_task.await.expect("SOCKS CONNECT 任务结束");
    }

    async fn route_table(
        original: SocketAddr,
        proxy: SocketAddr,
        destination_targets: Vec<DestinationTarget>,
    ) -> Arc<ProxyRouteTable> {
        let installed = InstalledApplication {
            package_name: "com.example.target".into(),
            signing_sha256: "AA".into(),
            uid: 10_001,
        };
        let profile = NetworkProfile {
            id: "transparent-route-test".into(),
            name: "透明路由".into(),
            target_applications: vec![TargetApplication {
                package_name: installed.package_name.clone(),
                signing_sha256: installed.signing_sha256.clone(),
                uid: installed.uid,
            }],
            destination_targets,
            proxy_routes: vec![ProxyRoute {
                listener_id: "fixture-listener".into(),
                destination: original.ip().to_string(),
                ports: vec![original.port()],
            }],
            confirmed_shared_uids: BTreeSet::new(),
            auto_resume_after_reboot: false,
            weak_network: WeakNetworkProfile::default(),
        }
        .validate_for_start(&[installed])
        .unwrap();
        let runtime = ProxyRuntimeConfiguration {
            routes: vec![ResolvedProxyRoute {
                listener_id: "fixture-listener".into(),
                original_destination: original.ip().to_string(),
                original_ports: vec![original.port()],
                resolved_original_ips: Vec::new(),
                proxy_host: proxy.ip().to_string(),
                proxy_port: proxy.port(),
            }],
        };
        Arc::new(ProxyRouteTable::compile(&profile, &runtime).await.unwrap())
    }

    #[tokio::test]
    async fn matched_original_target_uses_listener_when_original_is_unreachable() {
        let listener_fixture = TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).await.unwrap();
        let listener_address = listener_fixture.local_addr().unwrap();
        let fixture_task = tokio::spawn(async move {
            let (mut stream, _) = listener_fixture.accept().await.unwrap();
            let mut payload = [0_u8; 4];
            stream.read_exact(&mut payload).await.unwrap();
            assert_eq!(&payload, b"D48?");
            stream.write_all(b"D48!").await.unwrap();
        });
        // 该原始地址没有服务；成功响应只能来自透明映射后的 Listener fixture。
        let original = SocketAddr::new(IpAddr::V4(Ipv4Addr::new(127, 0, 0, 2)), 61_627);
        let routes = route_table(
            original,
            listener_address,
            // 与原始地址不匹配，证明 destination_targets 不参与透明路由选择。
            vec![DestinationTarget {
                cidr: "192.0.2.0/24".into(),
                ports: vec![443],
            }],
        )
        .await;

        let protector = RecordingProtection::default();
        let socks_listener = TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).await.unwrap();
        let socks_address = socks_listener.local_addr().unwrap();
        let server_task = tokio::spawn(async move {
            let (stream, _) = socks_listener.accept().await.unwrap();
            handle_socks_client(stream, protector, routes, 0)
                .await
                .unwrap();
        });
        let mut client = TcpStream::connect(socks_address).await.unwrap();
        socks_no_auth(&mut client).await;
        let mut request = vec![5, 1, 0, 1];
        let IpAddr::V4(original_ip) = original.ip() else {
            unreachable!()
        };
        request.extend_from_slice(&original_ip.octets());
        request.extend_from_slice(&original.port().to_be_bytes());
        client.write_all(&request).await.unwrap();
        let mut reply = [0_u8; 10];
        client.read_exact(&mut reply).await.unwrap();
        assert_eq!(reply[1], SOCKS_SUCCEEDED);
        client.write_all(b"D48?").await.unwrap();
        let mut response = [0_u8; 4];
        client.read_exact(&mut response).await.unwrap();
        assert_eq!(&response, b"D48!");
        drop(client);
        fixture_task.await.unwrap();
        server_task.await.unwrap();
    }

    #[tokio::test]
    async fn unmatched_target_still_connects_original_directly() {
        let original_fixture = TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).await.unwrap();
        let original = original_fixture.local_addr().unwrap();
        let original_task = tokio::spawn(async move {
            let (mut stream, _) = original_fixture.accept().await.unwrap();
            let mut payload = [0_u8; 4];
            stream.read_exact(&mut payload).await.unwrap();
            stream.write_all(&payload).await.unwrap();
        });
        let unrelated_original = SocketAddr::new(IpAddr::V4(Ipv4Addr::new(127, 0, 0, 2)), 61_627);
        let routes = route_table(unrelated_original, original, Vec::new()).await;
        let protector = RecordingProtection::default();
        let protection_calls = protector.calls.clone();
        let socks_listener = TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).await.unwrap();
        let socks_address = socks_listener.local_addr().unwrap();
        let server_task = tokio::spawn(async move {
            let (stream, _) = socks_listener.accept().await.unwrap();
            handle_socks_client(stream, protector, routes, 0)
                .await
                .unwrap();
        });
        let mut client = TcpStream::connect(socks_address).await.unwrap();
        socks_no_auth(&mut client).await;
        let SocketAddr::V4(original_v4) = original else {
            unreachable!()
        };
        let mut request = vec![5, 1, 0, 1];
        request.extend_from_slice(&original_v4.ip().octets());
        request.extend_from_slice(&original_v4.port().to_be_bytes());
        client.write_all(&request).await.unwrap();
        let mut reply = [0_u8; 10];
        client.read_exact(&mut reply).await.unwrap();
        assert_eq!(reply[1], SOCKS_SUCCEEDED);
        client.write_all(b"pass").await.unwrap();
        let mut echoed = [0_u8; 4];
        client.read_exact(&mut echoed).await.unwrap();
        assert_eq!(&echoed, b"pass");
        assert_eq!(protection_calls.load(AtomicOrdering::SeqCst), 1);
        drop(client);
        original_task.await.unwrap();
        server_task.await.unwrap();
    }

    #[allow(clippy::too_many_lines)]
    #[tokio::test]
    async fn socks_udp_associate_protects_ipv4_and_ipv6_sockets() {
        let echo = UdpSocket::bind((Ipv4Addr::LOCALHOST, 0))
            .await
            .expect("启动 UDP echo");
        let echo_address = echo.local_addr().expect("读取 UDP echo 地址");
        let echo_task = tokio::spawn(async move {
            let mut payload = [0_u8; 16];
            let (size, peer) = echo.recv_from(&mut payload).await.expect("接收 UDP");
            echo.send_to(&payload[..size], peer)
                .await
                .expect("回写 UDP");
        });
        let echo_v6 = UdpSocket::bind((Ipv6Addr::LOCALHOST, 0))
            .await
            .expect("启动 IPv6 UDP echo");
        let echo_v6_address = echo_v6.local_addr().expect("读取 IPv6 UDP echo 地址");
        let echo_v6_task = tokio::spawn(async move {
            let mut payload = [0_u8; 16];
            let (size, peer) = echo_v6
                .recv_from(&mut payload)
                .await
                .expect("接收 IPv6 UDP");
            echo_v6
                .send_to(&payload[..size], peer)
                .await
                .expect("回写 IPv6 UDP");
        });

        let protector = RecordingProtection::default();
        let calls = protector.calls.clone();
        let listener = TcpListener::bind((Ipv4Addr::LOCALHOST, 0))
            .await
            .expect("启动测试 SOCKS5");
        let socks_address = listener.local_addr().expect("读取 SOCKS5 地址");
        let server_task = tokio::spawn(async move {
            let (stream, _) = listener.accept().await.expect("接收 SOCKS5");
            handle_socks_client(stream, protector, Arc::new(ProxyRouteTable::default()), 0)
                .await
                .expect("SOCKS5 UDP ASSOCIATE 成功");
        });

        let mut control = TcpStream::connect(socks_address)
            .await
            .expect("连接 SOCKS5");
        socks_no_auth(&mut control).await;
        control
            .write_all(&[5, 3, 0, 1, 0, 0, 0, 0, 0, 0])
            .await
            .expect("发送 UDP ASSOCIATE");
        let mut reply = [0_u8; 10];
        control
            .read_exact(&mut reply)
            .await
            .expect("读取 UDP reply");
        assert_eq!(reply[1], SOCKS_SUCCEEDED);
        assert_eq!(&reply[4..8], &[127, 0, 0, 1]);
        let relay_address = SocketAddr::new(
            IpAddr::V4(Ipv4Addr::LOCALHOST),
            u16::from_be_bytes([reply[8], reply[9]]),
        );

        let client_udp = UdpSocket::bind((Ipv4Addr::LOCALHOST, 0))
            .await
            .expect("绑定 UDP 客户端");
        let SocketAddr::V4(echo_v4) = echo_address else {
            panic!("测试 UDP echo 应为 IPv4");
        };
        let mut datagram = vec![0, 0, 0, 1];
        datagram.extend_from_slice(&echo_v4.ip().octets());
        datagram.extend_from_slice(&echo_v4.port().to_be_bytes());
        datagram.extend_from_slice(b"udp");
        client_udp
            .send_to(&datagram, relay_address)
            .await
            .expect("发送 SOCKS5 UDP");
        let mut response = [0_u8; 64];
        let size = client_udp
            .recv(&mut response)
            .await
            .expect("接收 SOCKS5 UDP");
        let (_, payload_offset) =
            parse_socks_udp_request(&response[..size]).expect("解析 UDP reply");
        assert_eq!(&response[payload_offset..size], b"udp");

        let SocketAddr::V6(echo_v6) = echo_v6_address else {
            panic!("测试 IPv6 UDP echo 应为 IPv6");
        };
        let mut datagram_v6 = vec![0, 0, 0, 4];
        datagram_v6.extend_from_slice(&echo_v6.ip().octets());
        datagram_v6.extend_from_slice(&echo_v6.port().to_be_bytes());
        datagram_v6.extend_from_slice(b"v6");
        client_udp
            .send_to(&datagram_v6, relay_address)
            .await
            .expect("发送 IPv6 SOCKS5 UDP");
        let size = client_udp
            .recv(&mut response)
            .await
            .expect("接收 IPv6 SOCKS5 UDP");
        let (_, payload_offset) =
            parse_socks_udp_request(&response[..size]).expect("解析 IPv6 UDP reply");
        assert_eq!(&response[payload_offset..size], b"v6");
        assert_eq!(calls.load(AtomicOrdering::SeqCst), 2);

        drop(control);
        echo_task.await.expect("UDP echo 任务结束");
        echo_v6_task.await.expect("IPv6 UDP echo 任务结束");
        server_task.await.expect("SOCKS UDP 任务结束");
    }

    async fn socks_no_auth(client: &mut TcpStream) {
        client
            .write_all(&[SOCKS_VERSION, 1, 0])
            .await
            .expect("发送 SOCKS5 greeting");
        let mut reply = [0_u8; 2];
        client.read_exact(&mut reply).await.expect("读取 greeting");
        assert_eq!(reply, [SOCKS_VERSION, 0]);
    }
}
