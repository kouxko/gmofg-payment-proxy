//! Local package Sidecar process ownership.

use std::{
    collections::HashMap,
    path::PathBuf,
    sync::{
        Arc,
        atomic::{AtomicBool, AtomicU64, Ordering},
    },
    time::Duration,
};

use async_trait::async_trait;
use intercept_proxy_application::ExternalPackageApplicationPort;
use intercept_proxy_application::{AppError, AppResult};
use intercept_proxy_domain::ProtocolPackageRef;
use parking_lot::Mutex;
use tempfile::TempDir;
use tokio::{
    process::{Child, Command},
    sync::{Mutex as AsyncMutex, oneshot, watch},
    task::JoinHandle,
};

use super::ExternalPackageRegistryAdapter;

/// Private process arguments shared by production launch and focused tests.
#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct LocalPackageLaunchSpec {
    pub(crate) executable: PathBuf,
    pub(crate) archive: PathBuf,
    pub(crate) packages_url: String,
}

#[async_trait]
trait SpawnedLocalPackage: Send {
    async fn run(self: Box<Self>, kill: oneshot::Receiver<()>) -> std::io::Result<()>;
}

#[async_trait]
trait LocalPackageProcessLauncher: Send + Sync + std::fmt::Debug {
    async fn spawn(
        &self,
        spec: &LocalPackageLaunchSpec,
    ) -> std::io::Result<Box<dyn SpawnedLocalPackage>>;
}

#[derive(Debug)]
struct TokioProcessLauncher;

struct TokioSpawnedLocalPackage(Child);

#[async_trait]
impl SpawnedLocalPackage for TokioSpawnedLocalPackage {
    async fn run(mut self: Box<Self>, mut kill: oneshot::Receiver<()>) -> std::io::Result<()> {
        let _status = tokio::select! {
            status = self.0.wait() => status,
            _ = &mut kill => {
                self.0.kill().await?;
                self.0.wait().await
            }
        }?;
        Ok(())
    }
}

#[async_trait]
impl LocalPackageProcessLauncher for TokioProcessLauncher {
    async fn spawn(
        &self,
        spec: &LocalPackageLaunchSpec,
    ) -> std::io::Result<Box<dyn SpawnedLocalPackage>> {
        let child = Command::new(&spec.executable)
            .arg("--archive")
            .arg(&spec.archive)
            .arg("--packages-url")
            .arg(&spec.packages_url)
            .kill_on_drop(true)
            .spawn()?;
        Ok(Box::new(TokioSpawnedLocalPackage(child)))
    }
}

#[derive(Debug)]
struct OwnedProcess {
    generation: u64,
    kill: Option<oneshot::Sender<()>>,
    expected_stop: Arc<AtomicBool>,
    done: watch::Receiver<bool>,
    task: JoinHandle<()>,
}

impl OwnedProcess {
    async fn kill_and_wait(mut self) {
        self.expected_stop.store(true, Ordering::SeqCst);
        if let Some(kill) = self.kill.take() {
            let _ = kill.send(());
        }
        while !*self.done.borrow() && self.done.changed().await.is_ok() {}
        let _ = self.task.await;
    }
}

/// Owns exactly one local Sidecar process for each exact package version.
#[derive(Debug)]
pub(crate) struct LocalPackageSupervisor {
    executable: PathBuf,
    packages_url: String,
    registration_deadline: Duration,
    registry: Arc<ExternalPackageRegistryAdapter>,
    launcher: Arc<dyn LocalPackageProcessLauncher>,
    processes: AsyncMutex<HashMap<ProtocolPackageRef, OwnedProcess>>,
    next_generation: AtomicU64,
    lifecycle_gates: Mutex<HashMap<ProtocolPackageRef, Arc<AsyncMutex<()>>>>,
    archives: Mutex<Option<TempDir>>,
    shutting_down: AtomicBool,
}

impl LocalPackageSupervisor {
    pub(crate) fn new(
        executable: PathBuf,
        packages_url: String,
        registry: Arc<ExternalPackageRegistryAdapter>,
    ) -> Self {
        Self::with_launcher(
            executable,
            packages_url,
            Duration::from_secs(10),
            registry,
            Arc::new(TokioProcessLauncher),
        )
    }

    fn with_launcher(
        executable: PathBuf,
        packages_url: String,
        registration_deadline: Duration,
        registry: Arc<ExternalPackageRegistryAdapter>,
        launcher: Arc<dyn LocalPackageProcessLauncher>,
    ) -> Self {
        Self {
            executable,
            packages_url,
            registration_deadline,
            registry,
            launcher,
            processes: AsyncMutex::new(HashMap::new()),
            next_generation: AtomicU64::new(1),
            lifecycle_gates: Mutex::new(HashMap::new()),
            archives: Mutex::new(None),
            shutting_down: AtomicBool::new(false),
        }
    }

    fn lifecycle_gate(&self, package: &ProtocolPackageRef) -> Arc<AsyncMutex<()>> {
        self.lifecycle_gates
            .lock()
            .entry(package.clone())
            .or_insert_with(|| Arc::new(AsyncMutex::new(())))
            .clone()
    }

    fn freeze_archive(&self, package: &ProtocolPackageRef, bytes: &[u8]) -> AppResult<PathBuf> {
        let mut directory = self.archives.lock();
        if directory.is_none() {
            *directory = Some(TempDir::new().map_err(|error| process_error(&error))?);
        }
        let directory = directory
            .as_ref()
            .expect("temporary archive owner was initialized");
        let path = directory
            .path()
            .join(format!("{}-{}.zip", package.id, package.version));
        std::fs::write(&path, bytes).map_err(|error| process_error(&error))?;
        Ok(path)
    }

    pub(crate) async fn launch(
        &self,
        package: ProtocolPackageRef,
        archive: &[u8],
    ) -> AppResult<()> {
        let gate = self.lifecycle_gate(&package);
        let lifecycle = gate.lock().await;
        if self.shutting_down.load(Ordering::SeqCst) {
            return Err(AppError::new(
                "EXTERNAL_PACKAGE_PROCESS_FAILED",
                "本地软件包进程监督器正在关闭。",
            ));
        }
        self.stop_owned(&package).await;
        self.registry.disconnect(&package).await?;
        let frozen_archive = match self.freeze_archive(&package, archive) {
            Ok(path) => path,
            Err(error) => {
                self.persist_process_failure(&package).await;
                return Err(error);
            }
        };
        let spec = LocalPackageLaunchSpec {
            executable: self.executable.clone(),
            archive: frozen_archive,
            packages_url: self.packages_url.clone(),
        };
        let (generation, done) = self.spawn_owned(&package, &spec).await?;
        drop(lifecycle);

        let registration = tokio::time::timeout(
            self.registration_deadline,
            self.registry.wait_until_online(&package),
        );
        tokio::select! {
            result = registration => {
                if result.is_err() {
                    if !self.stop_generation(&package, generation).await {
                        return Err(AppError::new(
                            "EXTERNAL_PACKAGE_DISCONNECTED",
                            "本地软件包进程已被后续生命周期操作替换。",
                        ).entity(format!("{}@{}", package.id, package.version)));
                    }
                    let _ = self.registry.record_local_process_failure(
                        &package,
                        "EXTERNAL_PACKAGE_TIMEOUT",
                        "外部软件包调用超时。",
                    ).await;
                    return Err(AppError::new(
                        "EXTERNAL_PACKAGE_TIMEOUT",
                        "本地软件包未在 10 秒内完成注册。",
                    ).entity(format!("{}@{}", package.id, package.version)));
                }
            }
            () = wait_done(done) => {
                self.remove_generation(&package, generation).await;
                return Err(AppError::new(
                    "EXTERNAL_PACKAGE_DISCONNECTED",
                    "本地软件包进程在注册前退出。",
                ).entity(format!("{}@{}", package.id, package.version)));
            }
        }
        Ok(())
    }

    async fn spawn_owned(
        &self,
        package: &ProtocolPackageRef,
        spec: &LocalPackageLaunchSpec,
    ) -> AppResult<(u64, watch::Receiver<bool>)> {
        let process = match self.launcher.spawn(spec).await {
            Ok(process) => process,
            Err(error) => {
                let _ = self
                    .registry
                    .record_local_process_failure(
                        package,
                        "EXTERNAL_PACKAGE_PROCESS_FAILED",
                        "本地软件包进程启动失败。",
                    )
                    .await;
                return Err(process_error(&error));
            }
        };
        let (kill, killed) = oneshot::channel();
        let (completed, done) = watch::channel(false);
        let generation = self.next_generation.fetch_add(1, Ordering::SeqCst);
        let expected_stop = Arc::new(AtomicBool::new(false));
        let process_expected_stop = Arc::clone(&expected_stop);
        let process_registry = Arc::clone(&self.registry);
        let process_package = package.clone();
        let task = tokio::spawn(async move {
            let result = process.run(killed).await;
            if !process_expected_stop.load(Ordering::SeqCst) {
                let (code, message) = if result.is_ok() {
                    ("EXTERNAL_PACKAGE_DISCONNECTED", "外部软件包连接已断开。")
                } else {
                    (
                        "EXTERNAL_PACKAGE_PROCESS_FAILED",
                        "本地软件包进程启动失败。",
                    )
                };
                let _ = process_registry
                    .record_local_process_failure(&process_package, code, message)
                    .await;
            }
            let _ = completed.send(true);
        });
        self.processes.lock().await.insert(
            package.clone(),
            OwnedProcess {
                generation,
                kill: Some(kill),
                expected_stop,
                done: done.clone(),
                task,
            },
        );
        Ok((generation, done))
    }

    pub(crate) fn start_enabled(self: &Arc<Self>, packages: Vec<(ProtocolPackageRef, Vec<u8>)>) {
        for (package, archive) in packages {
            let supervisor = Arc::clone(self);
            tokio::spawn(async move {
                if let Err(error) = supervisor.launch(package.clone(), &archive).await {
                    supervisor.registry.record_package_operation_failure(
                        "local_sidecar_start",
                        &package,
                        &error,
                    );
                }
            });
        }
    }

    pub(crate) async fn stop(&self, package: &ProtocolPackageRef) {
        let gate = self.lifecycle_gate(package);
        let _lifecycle = gate.lock().await;
        self.stop_owned(package).await;
    }

    async fn stop_owned(&self, package: &ProtocolPackageRef) {
        if let Some(process) = self.processes.lock().await.remove(package) {
            process.kill_and_wait().await;
        }
    }

    async fn stop_generation(&self, package: &ProtocolPackageRef, generation: u64) -> bool {
        let gate = self.lifecycle_gate(package);
        let _lifecycle = gate.lock().await;
        let process = {
            let mut processes = self.processes.lock().await;
            if processes
                .get(package)
                .is_some_and(|process| process.generation == generation)
            {
                processes.remove(package)
            } else {
                None
            }
        };
        let Some(process) = process else {
            return false;
        };
        process.kill_and_wait().await;
        true
    }

    async fn remove_generation(&self, package: &ProtocolPackageRef, generation: u64) {
        let mut processes = self.processes.lock().await;
        if processes
            .get(package)
            .is_some_and(|process| process.generation == generation)
        {
            processes.remove(package);
        }
    }

    pub(crate) async fn shutdown(&self) {
        self.shutting_down.store(true, Ordering::SeqCst);
        loop {
            let packages = self
                .processes
                .lock()
                .await
                .keys()
                .cloned()
                .collect::<Vec<_>>();
            if packages.is_empty() {
                break;
            }
            for package in packages {
                self.stop(&package).await;
            }
        }
    }

    async fn persist_process_failure(&self, package: &ProtocolPackageRef) {
        let _ = self
            .registry
            .record_local_process_failure(
                package,
                "EXTERNAL_PACKAGE_PROCESS_FAILED",
                "本地软件包进程启动失败。",
            )
            .await;
    }
}

async fn wait_done(mut done: watch::Receiver<bool>) {
    while !*done.borrow() && done.changed().await.is_ok() {}
}

fn process_error(error: &std::io::Error) -> AppError {
    AppError::new("EXTERNAL_PACKAGE_PROCESS_FAILED", error.to_string())
}

#[cfg(test)]
#[path = "local_package_supervisor/tests.rs"]
mod tests;
