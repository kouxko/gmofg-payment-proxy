//! 与 UI 无关的代理装配根。
//!
//! 桌面、未来 TUI/CLI 和无界面集成测试都创建同一个 [`ApplicationHost`]。展示适配器
//! 只提供文件选择等平台服务；用例、持久化、证书、规则和网络实现统一在这里组装。

use std::{
    collections::BTreeMap,
    io,
    path::{Path, PathBuf},
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
};

use intercept_proxy_application::{
    AppError, AppResult, Application, ApplicationDependencies, BreakpointCoordinator,
    BreakpointValidator, CapacityLedger, CertificateServicePort, EventHub,
    OperationResultViewModel, ProtocolPackageApplicationServices, SettingsRepositoryPort,
    WorkspaceRepositoryPort,
};
use intercept_proxy_infrastructure::{
    AndroidAdbAdapter, ApplicationBackupImportPreparer, ExternalPackageServer,
    HeaderBodyCodecResolver, InfrastructureError, InfrastructureServiceBundle, NativeFileDialog,
    RuntimePipelineAdapter, RuntimePipelineProductHooks, SecretProtector, SqliteStore,
};
use intercept_proxy_product_api::{ProductError, ProductProfile, validate_product_profile};
use parking_lot::Mutex;
use thiserror::Error;
use tokio::task::JoinHandle;
use tokio_util::sync::CancellationToken;

use external_server::start_external_package_server;
use platform::{create_data_directory, platform_secret_protector};

mod external_server;
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
    #[error("无法清空 schema 早于 1.0 的应用数据 {path}: {source}")]
    ResetIncompatibleData {
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

    pub async fn build(self) -> Result<ApplicationHost, HostBuildError> {
        // 必须先验证纯静态产品配置，再创建目录和打开数据库。若产品契约错误，构建过程
        // 不应在磁盘留下任何新状态。
        validate_product_profile(self.product.as_ref())?;
        create_data_directory(&self.data_dir)?;
        let database_path = self
            .data_dir
            .join(self.product.storage().database_file_name);

        match self.clone().build_once(&database_path).await {
            Ok(host) => Ok(host),
            Err(error) if incompatible_persisted_data(&error) => {
                tracing::warn!(
                    path = %database_path.display(),
                    "database schema older than the 1.0 baseline was cleared"
                );
                remove_sqlite_database(&database_path)?;
                self.build_once(&database_path).await
            }
            Err(error) => Err(error),
        }
    }

    async fn build_once(self, database_path: &Path) -> Result<ApplicationHost, HostBuildError> {
        let store = Arc::new(SqliteStore::open(database_path)?);
        let secret_protector = self
            .platform
            .secret_protector_override
            .unwrap_or_else(|| platform_secret_protector(self.product.storage()));
        let capacity = Arc::new(CapacityLedger::default());
        let android_store = Arc::clone(&store);
        let file_dialog = Arc::clone(&self.platform.file_dialog);
        let services = InfrastructureServiceBundle::new(
            store,
            secret_protector,
            &file_dialog,
            Arc::clone(&self.product),
            Arc::clone(&capacity),
            self.builtin_protocol_package,
        );
        initialize_installation_state(&services).await?;

        let breakpoints = self
            .breakpoint_coordinator
            .unwrap_or_else(|| Arc::new(BreakpointCoordinator::default()));
        let events = Arc::new(EventHub::with_capacity_ledger(
            EventHub::DEFAULT_CAPACITY,
            Arc::clone(&services.capacity),
        ));
        let pipeline = build_runtime_pipeline(
            self.product.as_ref(),
            &services,
            breakpoints.clone(),
            events.clone(),
        );
        services
            .listener_runtime
            .set_pipeline_ports(pipeline.clone());
        services
            .listener_runtime
            .set_socket_diagnostic_events(events.clone());
        services.external_packages.set_event_hub(events.clone());
        let android = Arc::new(AndroidAdbAdapter::new(
            self.android_companion_apk,
            android_store,
        )?);
        let protocol_packages = ProtocolPackageApplicationServices {
            store: services.protocol_packages.clone(),
            compiler: services.protocol_packages.clone(),
            importer: services.protocol_package_import.clone(),
            builtin: services.protocol_packages.clone(),
            usage_query: services.protocol_package_usage.clone(),
            portability: services.protocol_packages.clone(),
            external: services.external_packages.clone(),
        };
        let external_package_server = Arc::new(start_external_package_server(&services).await?);
        let application = Arc::new(Application::new(
            self.product.name().to_owned(),
            ApplicationDependencies {
                capture: services.capture,
                sessions: services.sessions,
                breakpoints,
                breakpoint_validation: Arc::new(BreakpointValidator::new_with_resolver(Arc::new(
                    HeaderBodyCodecResolver,
                ))),
                rules: services.rules,
                faults: services.faults,
                certificates: services.certificates,
                settings: services.settings,
                workspaces: services.workspaces,
                listener_runtime: services.listener_runtime,
                listener_certificates: services.listener_certificates,
                protocol_packages,
                events: events.clone(),
            },
            android,
            services.protected_secrets,
        ));
        let background_cancellation = CancellationToken::new();
        // 抓包事件按时间合批，需要一个与 UI 无关的后台刷新任务。取消令牌和 JoinHandle
        // 都归 Host 所有，确保不同展示适配器有相同的关闭行为。
        let event_task =
            Arc::clone(&events).spawn_capture_flush_task(background_cancellation.child_token());

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

fn incompatible_persisted_data(error: &HostBuildError) -> bool {
    match error {
        HostBuildError::Infrastructure(InfrastructureError::DatabaseSchemaInvalid {
            current,
            found,
        }) => found.is_empty() || matches!(found.as_slice(), [(1, version)] if version < current),
        HostBuildError::InvalidProductProfile(_)
        | HostBuildError::CreateDataDirectory { .. }
        | HostBuildError::ResetIncompatibleData { .. }
        | HostBuildError::Application(_)
        | HostBuildError::Infrastructure(_) => false,
    }
}

fn remove_sqlite_database(database_path: &Path) -> Result<(), HostBuildError> {
    for path in [
        database_path.to_path_buf(),
        sqlite_sidecar(database_path, "-wal"),
        sqlite_sidecar(database_path, "-shm"),
    ] {
        match std::fs::remove_file(&path) {
            Ok(()) => {}
            Err(error) if error.kind() == io::ErrorKind::NotFound => {}
            Err(source) => return Err(HostBuildError::ResetIncompatibleData { path, source }),
        }
    }
    Ok(())
}

fn sqlite_sidecar(database_path: &Path, suffix: &str) -> PathBuf {
    let mut path = database_path.as_os_str().to_owned();
    path.push(suffix);
    PathBuf::from(path)
}

async fn initialize_installation_state(services: &InfrastructureServiceBundle) -> AppResult<()> {
    // feature marker 与官方精确身份在同一 SQLite 事务提交。marker 已存在时
    // 这一调用不会重建用户删除或损坏的包，只能由显式恢复用例处理。
    services.protocol_packages.ensure_builtin_seeded()?;
    // 每次启动都执行幂等同步：全新安装写入包内固定测试 Root；旧安装如果仍保存
    // 每机器随机 Root，则原子替换 Root 并按原 SAN 重签叶子证书。密钥库拒绝或用户
    // 取消授权仍是可恢复状态，Host 必须继续启动并由 UI 展示证书未就绪原因。
    if let Err(error) = services
        .certificates
        .synchronize_installation_ca(vec!["localhost".into(), "127.0.0.1".into()])
        .await
    {
        if is_recoverable_secret_store_error(&error) {
            tracing::warn!(
                code = %error.view_model.code,
                message = %error.view_model.message,
                "installation certificate synchronization was deferred"
            );
        } else {
            return Err(error);
        }
    }

    // 全新命名空间第一次启动只创建一个通用 Workspace。它仅含禁用的
    // 127.0.0.1:8080 正向代理草稿，不会自动监听端口或携带任何业务配置。
    if services.workspaces.list().await?.is_empty() {
        services.workspaces.create("默认 Workspace".into()).await?;
    }
    let stored = services.settings.get().await?.stored;
    // 会话仓储必须在接收任何 runtime 数据前使用持久化配置的容量限制，不能短暂按
    // 默认值运行后再缩小，否则启动阶段可能已经超额接纳。
    services
        .sessions
        .set_limits(stored.max_sessions, stored.max_memory_bytes)?;
    Ok(())
}

fn is_recoverable_secret_store_error(error: &AppError) -> bool {
    matches!(
        error.view_model.code.as_str(),
        "KEYCHAIN_PROTECT_FAILED"
            | "KEYCHAIN_UNPROTECT_FAILED"
            | "DPAPI_PROTECT_FAILED"
            | "DPAPI_UNPROTECT_FAILED"
    )
}

fn build_runtime_pipeline(
    product: &dyn ProductProfile,
    services: &InfrastructureServiceBundle,
    breakpoints: Arc<BreakpointCoordinator>,
    events: Arc<EventHub>,
) -> Arc<RuntimePipelineAdapter> {
    let channel_labels = product
        .channels()
        .iter()
        .map(|channel| (channel.id.to_owned(), channel.display_name.to_owned()))
        .collect::<BTreeMap<_, _>>();
    Arc::new(
        RuntimePipelineAdapter::new(
            RuntimePipelineProductHooks {
                body_codec: product.body_codec(),
                request_classifier: product.request_classifier(),
                channel_labels,
            },
            services.rules.clone(),
            services.sessions.clone(),
            breakpoints,
            events,
            services.capture.clone(),
        )
        .with_body_codec_resolver(services.workspace_body_codecs.clone()),
    )
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
