//! Android JNI 边界。
//!
//! JSON 反序列化和 Profile 校验仍复用纯 Rust 领域层。JNI 只负责接收 TUN fd、保存
//! `VpnService.protect(fd)` 回调和管理原生运行句柄，不在此处复制弱网业务规则。

#![allow(unsafe_code)]

use std::{
    os::fd::{FromRawFd, OwnedFd},
    sync::Mutex,
};

use jni::{
    Env, EnvUnowned,
    errors::ThrowRuntimeExAndDefault,
    objects::{JClass, JObject, JString},
    sys::{JNI_FALSE, JNI_TRUE, jboolean, jint},
};

use crate::{
    InstalledApplication, NetworkProfile,
    data_plane::{DataPlaneHandle, SocketProtector, runtime_stats_json},
};

static DATA_PLANE: Mutex<Option<DataPlaneHandle>> = Mutex::new(None);

/// 在 JNI 入口最早时刻接管 Kotlin `detachFd()` 交出的描述符。
///
/// 返回后描述符只由 [`OwnedFd`] 持有；后续 JSON、JVM、GlobalRef 或启动准备任一步
/// 失败，局部变量析构都会关闭它，不依赖 Kotlin 补偿关闭。
fn take_owned_tun_fd(tun_fd: jint) -> Result<OwnedFd, String> {
    if tun_fd < 0 {
        return Err("Android TUN 文件描述符无效".to_owned());
    }
    // SAFETY: NativeBridge 的契约要求调用方只传入 `ParcelFileDescriptor.detachFd()`
    // 产生、且尚未被其他所有者接管的描述符。此函数只调用一次并立即建立 RAII 所有权。
    Ok(unsafe { OwnedFd::from_raw_fd(tun_fd) })
}

fn read_json<T: serde::de::DeserializeOwned>(
    env: &Env<'_>,
    value: &JString<'_>,
) -> Result<T, String> {
    let text = value
        .try_to_string(env)
        .map_err(|error| error.to_string())?;
    serde_json::from_str(&text).map_err(|error| error.to_string())
}

fn stop_running_data_plane() {
    // Mutex 中毒只说明之前持锁线程发生过 panic，并不代表槽里的句柄不能安全停止。
    // 恢复 guard 后取出句柄，避免 `.ok()` 静默遗失唯一所有者并让 TUN 线程泄漏。
    let handle = DATA_PLANE
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .take();
    if let Some(handle) = handle {
        handle.stop();
    }
}

/// 返回空字符串表示 Profile 可启动；否则返回中文错误。
#[unsafe(no_mangle)]
pub extern "system" fn Java_com_interceptproxy_vpn_NativeBridge_nativeValidateProfile<'local>(
    mut unowned_env: EnvUnowned<'local>,
    _class: JClass<'local>,
    profile_json: JString<'local>,
    inventory_json: JString<'local>,
) -> JString<'local> {
    unowned_env
        .with_env(|env| {
            let result = (|| {
                let profile: NetworkProfile = read_json(env, &profile_json)?;
                let installed: Vec<InstalledApplication> = read_json(env, &inventory_json)?;
                profile
                    .validate_for_start(&installed)
                    .map(|_| String::new())
                    .map_err(|error| error.to_string())
            })();
            JString::from_str(env, result.unwrap_or_else(|error| error))
        })
        .resolve::<ThrowRuntimeExAndDefault>()
}

/// 启动真实 `TUN -> ImpairedTun -> tun2proxy -> SOCKS5 -> protect(fd)` 数据面。
///
/// `tun_fd` 必须来自 Kotlin 的 `ParcelFileDescriptor.detachFd()`；无论成功失败，调用后
/// 该 fd 都由 Rust 持有并关闭。
#[unsafe(no_mangle)]
pub extern "system" fn Java_com_interceptproxy_vpn_NativeBridge_nativeStart<'local>(
    mut unowned_env: EnvUnowned<'local>,
    _class: JClass<'local>,
    tun_fd: jint,
    profile_json: JString<'local>,
    inventory_json: JString<'local>,
    socket_protector: JObject<'local>,
) -> jboolean {
    let Ok(owned_tun_fd) = take_owned_tun_fd(tun_fd) else {
        return JNI_FALSE;
    };
    unowned_env
        .with_env(move |env| {
            let result = (|| {
                let profile: NetworkProfile = read_json(env, &profile_json)?;
                let installed: Vec<InstalledApplication> = read_json(env, &inventory_json)?;
                let profile = profile
                    .validate_for_start(&installed)
                    .map_err(|error| error.to_string())?;
                let vm = env.get_java_vm().map_err(|error| error.to_string())?;
                let protector = env
                    .new_global_ref(&socket_protector)
                    .map_err(|error| error.to_string())?;
                stop_running_data_plane();
                // OwnedFd 直接移交给 DataPlaneHandle，禁止先退化成裸 fd。这样线程创建、
                // 启动握手或全局槽更新任一步失败，都仍有 Rust RAII 负责关闭它。
                let handle = DataPlaneHandle::start(
                    owned_tun_fd,
                    profile,
                    SocketProtector::new(vm, protector),
                )?;
                let mut slot = DATA_PLANE
                    .lock()
                    .unwrap_or_else(std::sync::PoisonError::into_inner);
                *slot = Some(handle);
                Ok::<(), String>(())
            })();
            Ok::<jboolean, jni::errors::Error>(if result.is_ok() { JNI_TRUE } else { JNI_FALSE })
        })
        .resolve::<ThrowRuntimeExAndDefault>()
}

#[cfg(all(test, unix))]
mod tests {
    use std::{io::Read, os::fd::IntoRawFd, os::unix::net::UnixStream};

    use super::take_owned_tun_fd;

    #[test]
    fn owned_tun_fd_closes_when_pre_start_work_fails() {
        let (detached, mut peer) = UnixStream::pair().expect("unix pair");
        let raw_fd = detached.into_raw_fd();

        let owned = take_owned_tun_fd(raw_fd).expect("fd ownership");
        drop(owned); // 模拟 JSON/JVM/GlobalRef 准备失败。

        let mut byte = [0_u8; 1];
        assert_eq!(peer.read(&mut byte).expect("peer observes close"), 0);
    }

    #[test]
    fn negative_tun_fd_is_rejected_before_jni_work() {
        assert!(take_owned_tun_fd(-1).is_err());
    }
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_com_interceptproxy_vpn_NativeBridge_nativeStop<'local>(
    mut unowned_env: EnvUnowned<'local>,
    _class: JClass<'local>,
) {
    unowned_env
        .with_env(|_| {
            stop_running_data_plane();
            Ok::<(), jni::errors::Error>(())
        })
        .resolve::<ThrowRuntimeExAndDefault>();
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_com_interceptproxy_vpn_NativeBridge_nativeIsDataPlaneAvailable<
    'local,
>(
    mut unowned_env: EnvUnowned<'local>,
    _class: JClass<'local>,
) -> jboolean {
    unowned_env
        .with_env(|_| Ok::<jboolean, jni::errors::Error>(JNI_TRUE))
        .resolve::<ThrowRuntimeExAndDefault>()
}

/// 返回仅含计数器的 JSON；不返回、持久化或复制应用 Payload。
#[unsafe(no_mangle)]
pub extern "system" fn Java_com_interceptproxy_vpn_NativeBridge_nativeStatsJson<'local>(
    mut unowned_env: EnvUnowned<'local>,
    _class: JClass<'local>,
) -> JString<'local> {
    unowned_env
        .with_env(|env| JString::from_str(env, runtime_stats_json()))
        .resolve::<ThrowRuntimeExAndDefault>()
}
