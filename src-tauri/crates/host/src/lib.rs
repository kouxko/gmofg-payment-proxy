//! 与 UI 无关的代理装配根。
//!
//! 桌面、未来 TUI/CLI 和无界面集成测试都创建同一个 [`ApplicationHost`]。展示适配器
//! 只提供文件选择等平台服务；用例、持久化、证书、规则和网络实现统一在这里组装。

use std::{
    io,
    path::{Path, PathBuf},
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
};

use intercept_proxy_application::{
    AppError, AppResult, Application, BreakpointCoordinator, CapacityLedger,
    EnvironmentConfigurationApplicationServices, EventHub, OperationResultViewModel,
};
use intercept_proxy_infrastructure::{
    ApplicationBackupImportPreparer, ExternalPackageServer, InfrastructureError,
    InfrastructureServiceBundle, NativeFileDialog, SecretProtector,
};
use intercept_proxy_product_api::{ProductError, ProductProfile, validate_product_profile};
use parking_lot::Mutex;
use thiserror::Error;
use tokio::task::JoinHandle;
use tokio_util::sync::CancellationToken;

use platform::{create_data_directory, platform_secret_protector};

mod platform;

#[derive(Debug, Error)]
pub enum HostBuildError {
    #[error(transparent)]
    InvalidProductProfile(#[from] ProductError),
    #[error("无法创建应用数据目录 {path}: {source}")]
    CreateDataDirectory {
        path: PathBuf,
        #[source]
        source: io::Error,
    },
    #[error(transparent)]
    Infrastructure(#[from] InfrastructureError),
    #[error(transparent)]
    Application(#[from] AppError),
}

/// 刻意由最外层适配器提供的平台服务。
///
/// 文件对话框可以来自 Tauri、无界面测试脚本或未来终端提示，而无需修改任何应用用例。
#[derive(Clone)]
pub struct HostPlatformServices {
    secret_protector_override: Option<Arc<dyn SecretProtector>>,
    pub file_dialog: Arc<dyn NativeFileDialog>,
}

impl std::fmt::Debug for HostPlatformServices {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("HostPlatformServices")
            .field(
                "secret_protector_override",
                &self
                    .secret_protector_override
                    .as_ref()
                    .map(|_| "<SecretProtector>"),
            )
            .field("file_dialog", &self.file_dialog)
            .finish()
    }
}

impl HostPlatformServices {
    /// 使用显式秘密保护器创建平台边界。
    ///
    /// 仅供确定性测试和高级嵌入场景。正常生产 Host 应使用 [`Self::production`]，确保
    /// 保护器命名空间与数据库来自同一个产品配置。
    #[must_use]
    pub fn new(
        secret_protector: Arc<dyn SecretProtector>,
        file_dialog: Arc<dyn NativeFileDialog>,
    ) -> Self {
        Self {
            secret_protector_override: Some(secret_protector),
            file_dialog,
        }
    }

    /// 创建生产平台服务，不接受第二套独立产品命名空间。
    ///
    /// Builder 从 `ProductProfile::storage` 推导保护器，防止 Host 打开产品 A 的数据库，
    /// 却把秘密加密到产品 B 的 Keychain/DPAPI 命名空间。
    #[must_use]
    pub fn production(file_dialog: Arc<dyn NativeFileDialog>) -> Self {
        Self {
            secret_protector_override: None,
            file_dialog,
        }
    }
}

/// 使用数据目录和平台边界实现装配完整 Rust 应用。
///
/// Tauri 或 `WebView` 类型不允许跨过该边界。
#[derive(Clone, Debug)]
pub struct ApplicationHostBuilder {
    data_dir: PathBuf,
    android_companion_apk: Option<PathBuf>,
    builtin_protocol_package: Option<Arc<[u8]>>,
    platform: HostPlatformServices,
    product: Arc<dyn ProductProfile>,
    breakpoint_coordinator: Option<Arc<BreakpointCoordinator>>,
    environment_configuration_services: Option<EnvironmentConfigurationApplicationServices>,
}

impl ApplicationHostBuilder {
    #[must_use]
    pub fn new(
        data_dir: impl Into<PathBuf>,
        platform: HostPlatformServices,
        product: Arc<dyn ProductProfile>,
    ) -> Self {
        Self {
            data_dir: data_dir.into(),
            android_companion_apk: None,
            builtin_protocol_package: None,
            platform,
            product,
            breakpoint_coordinator: None,
            environment_configuration_services: None,
        }
    }

    /// 提供桌面安装包内 Android Companion APK 的绝对路径。
    ///
    /// Tauri、未来 CLI 或其他外壳各自负责解析自己的资源目录；Host 只把明确路径交给
    /// Android 适配器，不再让基础设施层猜测平台安装目录。
    #[must_use]
    pub fn with_android_companion_apk(mut self, path: impl Into<PathBuf>) -> Self {
        self.android_companion_apk = Some(path.into());
        self
    }

    /// 提供由桌面构建系统生成并嵌入的官方协议包 ZIP。
    #[must_use]
    pub fn with_builtin_protocol_package(mut self, archive: Arc<[u8]>) -> Self {
        self.builtin_protocol_package = Some(archive);
        self
    }

    /// 注入 application 与 runtime 管线共用的断点协调器。
    ///
    /// 测试可保留同一个 `Arc` 来创建处理中断点，无需在 Host 构建后暴露应用内部字段。
    #[must_use]
    pub fn with_breakpoint_coordinator(mut self, breakpoints: Arc<BreakpointCoordinator>) -> Self {
        self.breakpoint_coordinator = Some(breakpoints);
        self
    }

    /// Replaces the complete Application environment-configuration port group.
    ///
    /// The Host still assembles the real Application and all other production adapters. This
    /// seam supports deterministic embedding and lifecycle tests without exposing candidate
    /// registry internals or selecting cfg-specific behavior.
    #[must_use]
    pub fn with_environment_configuration_services(
        mut self,
        services: EnvironmentConfigurationApplicationServices,
    ) -> Self {
        self.environment_configuration_services = Some(services);
        self
    }

    pub async fn build(self) -> Result<ApplicationHost, HostBuildError> {
        // 必须先验证纯静态产品配置，再创建目录和打开数据库。若产品契约错误，构建过程
        // 不应在磁盘留下任何新状态。
        validate_product_profile(self.product.as_ref())?;
        create_data_directory(&self.data_dir)?;
        let database_path = self
            .data_dir
            .join(self.product.storage().database_file_name);

        self.build_once(&database_path).await
    }

    async fn build_once(self, database_path: &Path) -> Result<ApplicationHost, HostBuildError> {
        let persistence =
            intercept_proxy_infrastructure::open_sqlite_persistence(database_path.to_path_buf())
                .await?;
        let secret_protector = self
            .platform
            .secret_protector_override
            .unwrap_or_else(|| platform_secret_protector(self.product.storage()));
        let capacity = Arc::new(CapacityLedger::default());
        let file_dialog = Arc::clone(&self.platform.file_dialog);
        let services = InfrastructureServiceBundle::new(
            persistence,
            secret_protector,
            &file_dialog,
            Arc::clone(&self.product),
            &capacity,
            self.builtin_protocol_package,
        );
        services.initialize_installation_state().await?;

        let breakpoints = self
            .breakpoint_coordinator
            .unwrap_or_else(|| Arc::new(BreakpointCoordinator::default()));
        let events = host_event_hub(&capacity);
        services.configure_runtime(self.product.as_ref(), breakpoints.clone(), events.clone());
        let external_package_server = Arc::new(services.start_external_package_server().await?);
        let application = Arc::new(match self.environment_configuration_services {
            Some(environment) => {
                services
                    .into_application_with_environment_configuration_services(
                        self.product.name().to_owned(),
                        self.android_companion_apk,
                        breakpoints,
                        events.clone(),
                        environment,
                    )
                    .await?
            }
            None => {
                services
                    .into_application(
                        self.product.name().to_owned(),
                        self.android_companion_apk,
                        breakpoints,
                        events.clone(),
                    )
                    .await?
            }
        });
        let (background_cancellation, event_task) = spawn_capture_flush_task(&events);

        Ok(ApplicationHost {
            application,
            capacity,
            events,
            file_dialog,
            application_backup_importer: Arc::new(ApplicationBackupImportPreparer::new()),
            background_cancellation,
            event_task: Mutex::new(Some(event_task)),
            external_package_server,
            shutdown_started: AtomicBool::new(false),
            shutdown_completed: AtomicBool::new(false),
        })
    }
}

fn spawn_capture_flush_task(events: &Arc<EventHub>) -> (CancellationToken, JoinHandle<()>) {
    // 抓包事件按时间合批，需要一个与 UI 无关的后台刷新任务。取消令牌和 JoinHandle
    // 都归 Host 所有，确保不同展示适配器有相同的关闭行为。
    let cancellation = CancellationToken::new();
    let task = Arc::clone(events).spawn_capture_flush_task(cancellation.child_token());
    (cancellation, task)
}

fn host_event_hub(capacity: &Arc<CapacityLedger>) -> Arc<EventHub> {
    Arc::new(EventHub::with_capacity_ledger(
        EventHub::DEFAULT_CAPACITY,
        Arc::clone(capacity),
    ))
}

/// 持有与 UI 无关的应用门面及其后台任务生命周期。
///
/// 调用方通过 [`Self::application`] 执行用例；同一对象适用于 Tauri Command、无界面测试
/// 和未来终端适配器。
#[derive(Debug)]
pub struct ApplicationHost {
    application: Arc<Application>,
    capacity: Arc<CapacityLedger>,
    events: Arc<EventHub>,
    file_dialog: Arc<dyn NativeFileDialog>,
    application_backup_importer: Arc<ApplicationBackupImportPreparer>,
    background_cancellation: CancellationToken,
    event_task: Mutex<Option<JoinHandle<()>>>,
    external_package_server: Arc<ExternalPackageServer>,
    shutdown_started: AtomicBool,
    shutdown_completed: AtomicBool,
}

impl ApplicationHost {
    #[must_use]
    pub fn application(&self) -> Arc<Application> {
        Arc::clone(&self.application)
    }

    /// 返回 Settings 已校验并动态维护的共享运行时容量账本。
    #[must_use]
    pub fn capacity(&self) -> Arc<CapacityLedger> {
        Arc::clone(&self.capacity)
    }

    /// 返回展示适配器共享的有序事件中心；数据面只负责发布，不等待 `WebView` 消费。
    #[must_use]
    pub fn events(&self) -> Arc<EventHub> {
        Arc::clone(&self.events)
    }

    #[must_use]
    pub fn file_dialog(&self) -> Arc<dyn NativeFileDialog> {
        Arc::clone(&self.file_dialog)
    }

    #[must_use]
    pub fn application_backup_importer(&self) -> Arc<ApplicationBackupImportPreparer> {
        Arc::clone(&self.application_backup_importer)
    }

    /// 争抢唯一的优雅关闭执行权。
    ///
    /// 展示运行时可能重复收到退出通知，只有拿到 `true` 的调用者应启动关闭任务。
    pub fn begin_shutdown(&self) -> bool {
        !self.shutdown_started.swap(true, Ordering::AcqRel)
    }

    /// 报告关闭尝试及后台任务回收是否完成，不受应用停止成功与否影响。
    #[must_use]
    pub fn shutdown_completed(&self) -> bool {
        self.shutdown_completed.load(Ordering::Acquire)
    }

    /// 停止网络、完成应用关闭状态，并等待所有与 UI 无关的后台任务退出。
    pub async fn shutdown(&self) -> AppResult<OperationResultViewModel> {
        self.application
            .environment_candidate_shutdown_and_drain()
            .await;
        let result = self.application.app_shutdown().await;
        self.stop_background_tasks().await;
        self.shutdown_completed.store(true, Ordering::Release);
        result
    }

    pub fn cancel_background_tasks(&self) {
        self.background_cancellation.cancel();
        self.external_package_server.cancel();
    }

    async fn stop_background_tasks(&self) {
        self.cancel_background_tasks();
        self.external_package_server.shutdown().await;
        let task = self.event_task.lock().take();
        if let Some(task) = task
            && let Err(error) = task.await
            && !error.is_cancelled()
        {
            tracing::error!(?error, "capture event flush task failed");
        }
    }
}

impl Drop for ApplicationHost {
    fn drop(&mut self) {
        self.background_cancellation.cancel();
        if let Some(task) = self.event_task.get_mut().take() {
            task.abort();
        }
    }
}

#[cfg(test)]
#[path = "tests.rs"]
mod tests;
