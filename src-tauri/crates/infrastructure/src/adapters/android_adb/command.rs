//! ADB 可执行文件发现、命令执行与文本输出解析。
//!
//! 本模块只处理“如何调用 ADB”以及“如何解释 ADB 的文本输出”，不保存设备网络
//! 方案，也不决定透明代理路由的生命周期。

use std::{
    collections::HashMap,
    env,
    path::{Path, PathBuf},
    sync::Arc,
    time::Duration,
};

use async_trait::async_trait;
use intercept_proxy_application::{
    AndroidDeviceState, AndroidDeviceViewModel, AndroidPackageViewModel, AppError, AppResult,
};
use tokio::{process::Command, time::timeout};

use super::{AndroidAdbAdapter, COMMAND_TIMEOUT};

impl AndroidAdbAdapter {
    pub(super) fn selected_serial(&self) -> AppResult<String> {
        self.selected_serial
            .read()
            .expect("selected serial lock")
            .clone()
            .ok_or_else(|| {
                AppError::new(
                    "ANDROID_DEVICE_NOT_SELECTED",
                    "请先选择一台在线 Android 设备。",
                )
            })
    }

    pub(super) fn adb(&self) -> AppResult<&Path> {
        self.adb_path.as_deref().ok_or_else(|| {
            AppError::new(
                "ANDROID_ADB_NOT_FOUND",
                "未找到系统 adb；桌面应用不会内置 platform-tools。",
            )
            .retryable("请安装 Android platform-tools 并把 adb 加入 PATH。")
        })
    }

    pub(super) async fn run(&self, args: Vec<String>, duration: Duration) -> AppResult<AdbOutput> {
        let executable = self.adb()?.to_path_buf();
        let runner = Arc::clone(&self.runner);
        let output = timeout(duration, runner.run(&executable, &args))
            .await
            .map_err(|_| AppError::new("ANDROID_ADB_TIMEOUT", "adb 操作超时。"))?
            .map_err(|error| {
                AppError::new("ANDROID_ADB_EXEC_FAILED", format!("无法执行 adb：{error}"))
            })?;
        if !output.success {
            return Err(AppError::new(
                "ANDROID_ADB_COMMAND_FAILED",
                format!(
                    "adb 命令失败：{}",
                    non_empty(&output.stderr, &output.stdout)
                ),
            )
            .retryable("请检查设备是否在线、已授权且 Companion 状态正常。"));
        }
        Ok(output)
    }

    pub(super) async fn run_for_serial(
        &self,
        serial: &str,
        args: &[&str],
        duration: Duration,
    ) -> AppResult<AdbOutput> {
        let mut owned = vec!["-s".into(), serial.to_owned()];
        owned.extend(args.iter().map(|value| (*value).to_owned()));
        self.run(owned, duration).await
    }

    /// 建立或清理 ADB 端口映射时只操作用户明确选择的设备。
    ///
    /// `adb reconnect offline` 会修改 ADB server 中所有离线 transport，因此这里遇到
    /// 陈旧 transport 时返回可观察错误，交由显式的设备刷新流程恢复。
    pub(super) async fn run_forward_for_serial(
        &self,
        serial: &str,
        args: &[&str],
    ) -> AppResult<AdbOutput> {
        match self.run_for_serial(serial, args, COMMAND_TIMEOUT).await {
            Err(error) if is_stale_adb_transport_error(&error) => Err(AppError::new(
                "ANDROID_ADB_SELECTED_TRANSPORT_STALE",
                format!("选中设备 {serial} 的 ADB 转发被陈旧 transport 干扰；未修改其他设备连接。"),
            )
            .retryable("请刷新设备列表或显式清理离线 ADB 连接后重试。")),
            result => result,
        }
    }
}

#[async_trait]
pub(super) trait AdbCommandRunner: Send + Sync + std::fmt::Debug {
    async fn run(&self, executable: &Path, args: &[String]) -> std::io::Result<AdbOutput>;
}

#[derive(Debug)]
pub(super) struct SystemAdbCommandRunner;

#[async_trait]
impl AdbCommandRunner for SystemAdbCommandRunner {
    async fn run(&self, executable: &Path, args: &[String]) -> std::io::Result<AdbOutput> {
        let mut command = Command::new(executable);
        configure_background_process(&mut command);
        command.args(args).kill_on_drop(true);
        let output = command.output().await?;
        Ok(AdbOutput {
            success: output.status.success(),
            stdout: String::from_utf8_lossy(&output.stdout).into_owned(),
            stderr: String::from_utf8_lossy(&output.stderr).into_owned(),
        })
    }
}

/// 让短生命周期的 ADB 调用保持为真正的后台任务。
///
/// Windows GUI 程序如果直接创建控制台子进程，系统会为每一次 `adb` 轮询短暂显示
/// 一个命令行窗口。设备状态与 VPN 状态会定时刷新，因此缺少该标志时窗口会持续闪烁。
/// 这里通过 Tokio 暴露的标准库命令对象设置 `CREATE_NO_WINDOW`；其他平台无需处理。
#[cfg(windows)]
fn configure_background_process(command: &mut Command) {
    use std::os::windows::process::CommandExt;

    const CREATE_NO_WINDOW: u32 = 0x0800_0000;
    command.as_std_mut().creation_flags(CREATE_NO_WINDOW);
}

#[cfg(not(windows))]
fn configure_background_process(_: &mut Command) {}

#[derive(Clone, Debug)]
pub(super) struct AdbOutput {
    pub(super) success: bool,
    pub(super) stdout: String,
    pub(super) stderr: String,
}

pub(super) fn discover_adb() -> Option<PathBuf> {
    let executable = if cfg!(windows) { "adb.exe" } else { "adb" };
    let mut candidates = Vec::new();
    if let Some(path) = env::var_os("PATH") {
        candidates.extend(env::split_paths(&path).map(|directory| directory.join(executable)));
    }
    for variable in ["ANDROID_HOME", "ANDROID_SDK_ROOT"] {
        if let Some(root) = env::var_os(variable) {
            candidates.push(PathBuf::from(root).join("platform-tools").join(executable));
        }
    }
    if let Some(home) = env::var_os("HOME") {
        candidates.push(
            PathBuf::from(home)
                .join("Library/Android/sdk/platform-tools")
                .join(executable),
        );
    }
    candidates.into_iter().find(|path| path.is_file())
}

pub(super) fn discover_companion_apk() -> Option<PathBuf> {
    let mut candidates = Vec::new();
    if let Some(path) = env::var_os("INTERCEPT_PROXY_ANDROID_COMPANION_APK") {
        candidates.push(PathBuf::from(path));
    }
    if let Ok(executable) = env::current_exe() {
        candidates.extend(bundled_companion_apk_candidates(&executable));
    }
    candidates.push(PathBuf::from(
        "android-companion/app/build/outputs/apk/release/app-release.apk",
    ));
    candidates.push(PathBuf::from(
        "android-companion/app/build/outputs/apk/debug/app-debug.apk",
    ));
    candidates.into_iter().find(|path| path.is_file())
}

/// 返回桌面安装包中 Companion APK 的平台候选位置。
pub(super) fn bundled_companion_apk_candidates(executable: &Path) -> Vec<PathBuf> {
    let Some(directory) = executable.parent() else {
        return Vec::new();
    };
    vec![
        directory.join("resources/android-companion.apk"),
        directory.join("../Resources/resources/android-companion.apk"),
        directory.join("../Resources/android-companion.apk"),
        directory.join("android-companion.apk"),
    ]
}

pub(super) fn parse_devices(output: &str, selected: Option<&str>) -> Vec<AndroidDeviceViewModel> {
    output
        .lines()
        .skip(1)
        .filter_map(|line| {
            let mut fields = line.split_whitespace();
            let serial = fields.next()?.to_owned();
            let raw_state = fields.next()?;
            if serial.starts_with('*') {
                return None;
            }
            let properties = fields
                .filter_map(|field| field.split_once(':'))
                .collect::<HashMap<_, _>>();
            Some(AndroidDeviceViewModel {
                selected: selected == Some(serial.as_str()),
                serial,
                state: match raw_state {
                    "device" => AndroidDeviceState::Device,
                    "offline" => AndroidDeviceState::Offline,
                    "unauthorized" => AndroidDeviceState::Unauthorized,
                    _ => AndroidDeviceState::Other,
                },
                product: properties.get("product").map(|value| (*value).to_owned()),
                model: properties.get("model").map(|value| (*value).to_owned()),
                device: properties.get("device").map(|value| (*value).to_owned()),
                transport_id: properties
                    .get("transport_id")
                    .map(|value| (*value).to_owned()),
            })
        })
        .collect()
}

pub(super) fn parse_packages(output: &str) -> Vec<AndroidPackageViewModel> {
    output
        .lines()
        .filter_map(|line| {
            let line = line.strip_prefix("package:")?;
            let (package_name, uid) = line.rsplit_once(" uid:")?;
            Some(AndroidPackageViewModel {
                package_name: package_name.to_owned(),
                uid: uid.trim().parse().ok()?,
                shared_uid: None,
            })
        })
        .collect()
}

pub(super) fn parse_package_version(output: &str) -> (Option<String>, Option<String>) {
    let version_name = output
        .lines()
        .find_map(|line| line.trim().strip_prefix("versionName=").map(str::to_owned));
    let version_code = output.lines().find_map(|line| {
        line.trim()
            .strip_prefix("versionCode=")
            .and_then(|value| value.split_whitespace().next())
            .map(str::to_owned)
    });
    (version_name, version_code)
}

pub(super) fn is_stale_adb_transport_error(error: &AppError) -> bool {
    let message = error.view_model.message.to_ascii_lowercase();
    message.contains("more than one device/emulator")
        || message.contains("more than one device or emulator")
}

/// 判断 ADB 端口映射是否已经不存在。
///
/// `adb forward/reverse --remove` 不是严格幂等的：映射已经被 ADB、设备重连或前一次
/// 清理删除时，命令会以非零状态退出。对停止流程而言，这个结果等价于“清理完成”，
/// 不能因此把已经停止的 VPN 重新标记为失败，也不能继续保留陈旧的端口所有权。
pub(super) fn is_missing_adb_listener_error(error: &AppError) -> bool {
    let message = error.view_model.message.to_ascii_lowercase();
    message.contains("listener 'tcp:") && message.contains("not found")
}

fn non_empty<'a>(preferred: &'a str, fallback: &'a str) -> &'a str {
    if preferred.trim().is_empty() {
        fallback.trim()
    } else {
        preferred.trim()
    }
}
