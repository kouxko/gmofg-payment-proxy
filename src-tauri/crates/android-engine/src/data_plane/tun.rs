//! TUN 文件描述符的安全所有权与停止语义。

use std::{
    io::{self, Read, Write},
    mem::ManuallyDrop,
    os::fd::{AsRawFd, OwnedFd, RawFd},
    sync::{Arc, Mutex},
};

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
pub(super) struct TunFdRelease(Arc<TunFdLease>);

impl TunFdRelease {
    pub(super) fn release_tun_reference(&self) -> io::Result<()> {
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
pub(super) struct ManagedTunFile {
    file: ManuallyDrop<std::fs::File>,
    lease: Arc<TunFdLease>,
}

impl ManagedTunFile {
    pub(super) fn new(tun_fd: OwnedFd) -> io::Result<(Self, TunFdRelease)> {
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
