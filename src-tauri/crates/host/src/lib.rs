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
    ProtocolPackageApplicationServices, ProxyStatusViewModel, ProxySupervisorPort, SettingsDraft,
    SettingsRepositoryPort, WorkspaceRepositoryPort,
};
#[cfg(not(target_os = "macos"))]
use intercept_proxy_infrastructure::DpapiProtector;
#[cfg(target_os = "macos")]
use intercept_proxy_infrastructure::MacKeychainProtector;
use intercept_proxy_infrastructure::{
    AndroidAdbAdapter, HeaderBodyCodecResolver, InfrastructureError, InfrastructureServiceBundle,
    NativeFileDialog, RetiredProxyAdapter, RuntimePipelineAdapter, RuntimePipelineProductHooks,
    SecretProtector, SqliteStore,
};
use intercept_proxy_product_api::{
    ProductError, ProductProfile, ProductStorageNamespace, validate_product_profile,
};
use parking_lot::Mutex;
use thiserror::Error;
use tokio::task::JoinHandle;
use tokio_util::sync::CancellationToken;

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
#[derive(Debug)]
pub struct ApplicationHostBuilder {
    data_dir: PathBuf,
    android_companion_apk: Option<PathBuf>,
    platform: HostPlatformServices,
    product: Arc<dyn ProductProfile>,
    proxy_override: Option<Arc<dyn ProxySupervisorPort>>,
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
            platform,
            product,
            proxy_override: None,
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

    /// 只替换网络监督器，同时保留真实应用、仓储、规则、证书和设置适配器。
    ///
    /// 用于确定性的纯 Rust 集成测试和其他进程 Host；生产调用者使用默认 runtime。
    #[must_use]
    pub fn with_proxy_supervisor(mut self, proxy: Arc<dyn ProxySupervisorPort>) -> Self {
        self.proxy_override = Some(proxy);
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
        let store = Arc::new(SqliteStore::open(
            &self
                .data_dir
                .join(self.product.storage().database_file_name),
        )?);
        let secret_protector = self
            .platform
            .secret_protector_override
            .unwrap_or_else(|| platform_secret_protector(self.product.storage()));
        let capacity = Arc::new(CapacityLedger::default());
        let android_store = Arc::clone(&store);
        let services = InfrastructureServiceBundle::new(
            store,
            secret_protector,
            self.platform.file_dialog,
            Arc::clone(&self.product),
            Arc::clone(&capacity),
        );
        let stored_settings = initialize_installation_state(&services).await?;

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
        let proxy: Arc<dyn ProxySupervisorPort> = self.proxy_override.unwrap_or_else(|| {
            // 生产环境只能由 ListenerRuntime 管理动态 Workspace 入口。旧端口注入一个
            // 永远停止的兼容适配器，保证任何遗漏调用都不能偷偷启动第二套监听器。
            Arc::new(RetiredProxyAdapter::new(stored_settings))
        });
        let android = Arc::new(AndroidAdbAdapter::new(
            self.android_companion_apk,
            android_store,
        )?);
        let protocol_packages = ProtocolPackageApplicationServices {
            store: services.protocol_packages.clone(),
            compiler: services.protocol_packages.clone(),
            importer: services.protocol_package_import.clone(),
            usage_query: services.protocol_package_usage.clone(),
            portability: services.protocol_packages.clone(),
        };
        let application = Arc::new(Application::new_with_platform_services(
            self.product.name().to_owned(),
            ApplicationDependencies {
                proxy,
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
                workspace_documents: services.workspace_documents,
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
        let event_task = events.spawn_capture_flush_task(background_cancellation.child_token());

        Ok(ApplicationHost {
            application,
            background_cancellation,
            event_task: Mutex::new(Some(event_task)),
            shutdown_started: AtomicBool::new(false),
            shutdown_completed: AtomicBool::new(false),
        })
    }
}

async fn initialize_installation_state(
    services: &InfrastructureServiceBundle,
) -> AppResult<SettingsDraft> {
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
    Ok(stored)
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
        .with_body_codec_resolver(services.workspace_body_codecs.clone())
        .with_workspace_policy_resolver(services.workspace_runtime_policies.clone()),
    )
}

/// 持有与 UI 无关的应用门面及其后台任务生命周期。
///
/// 调用方通过 [`Self::application`] 执行用例；同一对象适用于 Tauri Command、无界面测试
/// 和未来终端适配器。
#[derive(Debug)]
pub struct ApplicationHost {
    application: Arc<Application>,
    background_cancellation: CancellationToken,
    event_task: Mutex<Option<JoinHandle<()>>>,
    shutdown_started: AtomicBool,
    shutdown_completed: AtomicBool,
}

impl ApplicationHost {
    #[must_use]
    pub fn application(&self) -> Arc<Application> {
        Arc::clone(&self.application)
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
    pub async fn shutdown(&self) -> AppResult<ProxyStatusViewModel> {
        let result = self.application.app_shutdown().await;
        self.stop_background_tasks().await;
        self.shutdown_completed.store(true, Ordering::Release);
        result
    }

    pub fn cancel_background_tasks(&self) {
        self.background_cancellation.cancel();
    }

    async fn stop_background_tasks(&self) {
        self.cancel_background_tasks();
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

fn create_data_directory(path: &Path) -> Result<(), HostBuildError> {
    std::fs::create_dir_all(path).map_err(|source| HostBuildError::CreateDataDirectory {
        path: path.to_path_buf(),
        source,
    })
}

fn platform_secret_protector(storage: ProductStorageNamespace) -> Arc<dyn SecretProtector> {
    #[cfg(windows)]
    {
        let _ = storage;
        Arc::new(DpapiProtector)
    }
    #[cfg(target_os = "macos")]
    {
        Arc::new(MacKeychainProtector::for_namespace(storage))
    }
    #[cfg(not(any(windows, target_os = "macos")))]
    {
        let _ = storage;
        Arc::new(DpapiProtector)
    }
}

#[cfg(test)]
#[path = "tests.rs"]
mod tests;
