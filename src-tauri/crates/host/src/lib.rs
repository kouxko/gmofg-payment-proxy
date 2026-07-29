//! UI-neutral composition root for the payment proxy.
//!
//! Desktop, future TUI/CLI, and headless integration tests all construct the
//! same [`ApplicationHost`]. Presentation adapters provide only platform
//! services such as file selection; use-case, persistence, certificate, rule,
//! and network assembly stays here.

use std::{
    io,
    path::{Path, PathBuf},
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
};

use gmofg_proxy_application::{
    AppError, AppResult, Application, BreakpointCoordinator, BreakpointValidator, EventHub,
    ProxyStatusViewModel, SettingsRepositoryPort,
};
#[cfg(not(target_os = "macos"))]
use gmofg_proxy_infrastructure::DpapiProtector;
#[cfg(target_os = "macos")]
use gmofg_proxy_infrastructure::MacKeychainProtector;
use gmofg_proxy_infrastructure::{
    InfrastructureError, InfrastructureServiceBundle, NativeFileDialog, RuntimePipelineAdapter,
    SecretProtector, SqliteStore,
};
use gmofg_proxy_runtime::{
    ApplicationProxyAdapter, ProxySupervisor, RustlsRuntimeServiceFactory, SystemClock,
    TokioListenerBinder,
};
use parking_lot::Mutex;
use thiserror::Error;
use tokio::task::JoinHandle;
use tokio_util::sync::CancellationToken;

const DATABASE_FILE_NAME: &str = "gmofg-payment-proxy.sqlite3";

#[derive(Debug, Error)]
pub enum HostBuildError {
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

/// Platform services that are intentionally supplied by the outer adapter.
///
/// The file dialog can be backed by Tauri, a scripted headless test, or a
/// future terminal prompt without changing any application use case.
pub struct HostPlatformServices {
    pub secret_protector: Arc<dyn SecretProtector>,
    pub file_dialog: Arc<dyn NativeFileDialog>,
}

impl std::fmt::Debug for HostPlatformServices {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("HostPlatformServices")
            .field("secret_protector", &"<SecretProtector>")
            .field("file_dialog", &self.file_dialog)
            .finish()
    }
}

impl HostPlatformServices {
    #[must_use]
    pub fn new(
        secret_protector: Arc<dyn SecretProtector>,
        file_dialog: Arc<dyn NativeFileDialog>,
    ) -> Self {
        Self {
            secret_protector,
            file_dialog,
        }
    }

    #[must_use]
    pub fn production(file_dialog: Arc<dyn NativeFileDialog>) -> Self {
        Self::new(platform_secret_protector(), file_dialog)
    }
}

/// Builds the complete Rust application from a data directory and platform
/// boundary implementations. No Tauri or `WebView` type crosses this boundary.
#[derive(Debug)]
pub struct ApplicationHostBuilder {
    data_dir: PathBuf,
    platform: HostPlatformServices,
}

impl ApplicationHostBuilder {
    #[must_use]
    pub fn new(data_dir: impl Into<PathBuf>, platform: HostPlatformServices) -> Self {
        Self {
            data_dir: data_dir.into(),
            platform,
        }
    }

    pub async fn build(self) -> Result<ApplicationHost, HostBuildError> {
        create_data_directory(&self.data_dir)?;
        let store = Arc::new(SqliteStore::open(&self.data_dir.join(DATABASE_FILE_NAME))?);
        let services = InfrastructureServiceBundle::new(
            store,
            self.platform.secret_protector,
            self.platform.file_dialog,
        );
        let settings = services.settings.get().await?;
        services.sessions.set_limits(
            settings.stored.max_sessions,
            settings.stored.max_memory_bytes,
        )?;

        let breakpoints = Arc::new(BreakpointCoordinator::default());
        let events = Arc::new(EventHub::new(EventHub::DEFAULT_CAPACITY));
        let pipeline = Arc::new(RuntimePipelineAdapter::new(
            services.rules.clone(),
            services.sessions.clone(),
            breakpoints.clone(),
            events.clone(),
            services.capture.clone(),
        ));
        let service_factory = Arc::new(RustlsRuntimeServiceFactory::new(
            services.certificates.clone(),
            pipeline.clone(),
            Arc::new(SystemClock),
        ));
        let supervisor = Arc::new(ProxySupervisor::with_factory(
            Arc::new(TokioListenerBinder),
            service_factory,
        ));
        let proxy = Arc::new(ApplicationProxyAdapter::new(
            supervisor,
            settings.stored,
            pipeline,
        ));
        let application = Arc::new(Application::new(
            proxy,
            services.capture,
            services.sessions,
            breakpoints,
            Arc::new(BreakpointValidator),
            services.rules,
            services.faults,
            services.certificates,
            services.settings,
            services.file_export,
            events.clone(),
        ));
        let background_cancellation = CancellationToken::new();
        let event_task = events.spawn_capture_flush_task(background_cancellation.child_token());

        Ok(ApplicationHost {
            application,
            background_cancellation,
            event_task: Mutex::new(Some(event_task)),
            shutdown_started: AtomicBool::new(false),
        })
    }
}

/// Owns the UI-independent application facade and its background lifecycle.
///
/// Callers invoke use cases through [`Self::application`]. The same object is
/// suitable for Tauri commands, headless tests, and future terminal adapters.
#[derive(Debug)]
pub struct ApplicationHost {
    application: Arc<Application>,
    background_cancellation: CancellationToken,
    event_task: Mutex<Option<JoinHandle<()>>>,
    shutdown_started: AtomicBool,
}

impl ApplicationHost {
    #[must_use]
    pub fn application(&self) -> Arc<Application> {
        Arc::clone(&self.application)
    }

    /// Claims the single graceful-shutdown owner.
    ///
    /// Presentation runtimes can receive duplicate exit notifications; only
    /// the caller that receives `true` should spawn graceful shutdown.
    pub fn begin_shutdown(&self) -> bool {
        !self.shutdown_started.swap(true, Ordering::AcqRel)
    }

    /// Stops network activity, resolves application shutdown state, and joins
    /// UI-independent background tasks.
    pub async fn shutdown(&self) -> AppResult<ProxyStatusViewModel> {
        let result = self.application.app_shutdown().await;
        self.stop_background_tasks().await;
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

fn platform_secret_protector() -> Arc<dyn SecretProtector> {
    #[cfg(windows)]
    {
        Arc::new(DpapiProtector)
    }
    #[cfg(target_os = "macos")]
    {
        Arc::new(MacKeychainProtector::default())
    }
    #[cfg(not(any(windows, target_os = "macos")))]
    {
        Arc::new(DpapiProtector)
    }
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use gmofg_proxy_application::{AppResult, ProxyState};
    use gmofg_proxy_infrastructure::{
        InfrastructureError, NativeFileDialog, SecretProtector, adapters::FileSelection,
    };

    use super::*;

    #[derive(Debug)]
    struct NoFileDialog;

    impl NativeFileDialog for NoFileDialog {
        fn choose_open_file(&self, _purpose: &str) -> AppResult<Option<PathBuf>> {
            Ok(None)
        }

        fn choose_save_file(&self, _purpose: &str) -> AppResult<Option<FileSelection>> {
            Ok(None)
        }
    }

    #[derive(Debug)]
    struct TestSecretProtector;

    impl SecretProtector for TestSecretProtector {
        fn protect(&self, plaintext: &[u8]) -> Result<Vec<u8>, InfrastructureError> {
            Ok(plaintext.iter().map(|byte| byte ^ 0xa5).collect())
        }

        fn unprotect(&self, ciphertext: &[u8]) -> Result<Vec<u8>, InfrastructureError> {
            self.protect(ciphertext)
        }
    }

    #[tokio::test]
    async fn builds_and_invokes_application_without_tauri() {
        let temp = tempfile::tempdir().expect("temporary host directory");
        let host = ApplicationHostBuilder::new(
            temp.path(),
            HostPlatformServices::new(Arc::new(TestSecretProtector), Arc::new(NoFileDialog)),
        )
        .build()
        .await
        .expect("build UI-neutral host");

        let application = host.application();
        let status = application
            .proxy_get_status()
            .await
            .expect("query proxy status");
        assert_eq!(status.state, ProxyState::Stopped);

        let settings = application.settings_get().await.expect("query settings");
        assert_eq!(settings.stored.max_sessions, 500);

        let draft = application
            .rule_new_draft()
            .await
            .expect("create rule draft");
        assert_eq!(draft.name, "新建规则");

        host.shutdown().await.expect("shutdown UI-neutral host");
    }
}
