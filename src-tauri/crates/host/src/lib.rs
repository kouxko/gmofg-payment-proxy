//! UI-neutral composition root for the payment proxy.
//!
//! Desktop, future TUI/CLI, and headless integration tests all construct the
//! same [`ApplicationHost`]. Presentation adapters provide only platform
//! services such as file selection; use-case, persistence, certificate, rule,
//! and network assembly stays here.

use std::{
    collections::BTreeMap,
    io,
    path::{Path, PathBuf},
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
};

use gmofg_proxy_application::{
    AppError, AppResult, Application, BreakpointCoordinator, BreakpointValidator, CapacityLedger,
    EventHub, ProxyStatusViewModel, ProxySupervisorPort, SettingsRepositoryPort,
};
#[cfg(not(target_os = "macos"))]
use gmofg_proxy_infrastructure::DpapiProtector;
#[cfg(target_os = "macos")]
use gmofg_proxy_infrastructure::MacKeychainProtector;
use gmofg_proxy_infrastructure::{
    ApplicationProxyAdapter, InfrastructureError, InfrastructureServiceBundle, NativeFileDialog,
    RuntimePipelineAdapter, RuntimePipelineProductHooks, SecretProtector, SqliteStore,
};
use gmofg_proxy_product_api::{
    ProductError, ProductProfile, ProductStorageNamespace, validate_product_profile,
};
use gmofg_proxy_runtime::{
    ProxySupervisor, RustlsRuntimeServiceFactory, SystemClock, TokioListenerBinder,
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

/// Platform services that are intentionally supplied by the outer adapter.
///
/// The file dialog can be backed by Tauri, a scripted headless test, or a
/// future terminal prompt without changing any application use case.
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
    /// Creates a platform boundary with an explicit secret protector.
    ///
    /// This is intended for deterministic tests and advanced embedders. Normal
    /// production hosts should use [`Self::production`] so the protector
    /// namespace is derived from the same product profile as the database.
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

    /// Creates production platform services without accepting a second,
    /// independently supplied product namespace.
    ///
    /// The builder derives the protector from `ProductProfile::storage`,
    /// preventing a host from opening one product database while encrypting
    /// its secrets under another product's keychain/DPAPI namespace.
    #[must_use]
    pub fn production(file_dialog: Arc<dyn NativeFileDialog>) -> Self {
        Self {
            secret_protector_override: None,
            file_dialog,
        }
    }
}

/// Builds the complete Rust application from a data directory and platform
/// boundary implementations. No Tauri or `WebView` type crosses this boundary.
#[derive(Debug)]
pub struct ApplicationHostBuilder {
    data_dir: PathBuf,
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
            platform,
            product,
            proxy_override: None,
            breakpoint_coordinator: None,
        }
    }

    /// Replaces the network supervisor port while preserving every real
    /// application, repository, rule, certificate, and settings adapter.
    ///
    /// This is intended for deterministic Rust-only integration tests and
    /// alternate process hosts; production callers use the default runtime.
    #[must_use]
    pub fn with_proxy_supervisor(mut self, proxy: Arc<dyn ProxySupervisorPort>) -> Self {
        self.proxy_override = Some(proxy);
        self
    }

    /// Injects the coordinator shared by the application and runtime pipeline.
    ///
    /// Tests can retain their `Arc` to seed an in-flight breakpoint without
    /// exposing application internals through the host after construction.
    #[must_use]
    pub fn with_breakpoint_coordinator(mut self, breakpoints: Arc<BreakpointCoordinator>) -> Self {
        self.breakpoint_coordinator = Some(breakpoints);
        self
    }

    pub async fn build(self) -> Result<ApplicationHost, HostBuildError> {
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
        let services = InfrastructureServiceBundle::new(
            store,
            secret_protector,
            self.platform.file_dialog,
            Arc::clone(&self.product),
            Arc::clone(&capacity),
        );
        let settings = services.settings.get().await?;
        services.sessions.set_limits(
            settings.stored.max_sessions,
            settings.stored.max_memory_bytes,
        )?;

        let breakpoints = self
            .breakpoint_coordinator
            .unwrap_or_else(|| Arc::new(BreakpointCoordinator::default()));
        let events = Arc::new(EventHub::with_capacity_ledger(
            EventHub::DEFAULT_CAPACITY,
            Arc::clone(&services.capacity),
        ));
        let channel_labels = self
            .product
            .channels()
            .iter()
            .map(|channel| (channel.id.to_owned(), channel.display_name.to_owned()))
            .collect::<BTreeMap<_, _>>();
        let pipeline = Arc::new(RuntimePipelineAdapter::new(
            RuntimePipelineProductHooks {
                body_codec: self.product.body_codec(),
                request_classifier: self.product.request_classifier(),
                channel_labels,
            },
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
        let proxy: Arc<dyn ProxySupervisorPort> = self.proxy_override.unwrap_or_else(|| {
            Arc::new(ApplicationProxyAdapter::new(
                supervisor,
                settings.stored,
                pipeline,
                self.product.labels(),
            ))
        });
        let application = Arc::new(Application::new(
            self.product.name().to_owned(),
            proxy,
            services.capture,
            services.sessions,
            breakpoints,
            Arc::new(BreakpointValidator::new(self.product.body_codec())),
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
mod tests {
    use std::path::PathBuf;

    use gmofg_proxy_application::{AppResult, ProxyState};
    use gmofg_proxy_infrastructure::{
        InfrastructureError, NativeFileDialog, SecretProtector, adapters::FileSelection,
    };
    use gmofg_proxy_product_payment::PaymentProductProfile;

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
            Arc::new(PaymentProductProfile::isolated_test_tool()),
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
