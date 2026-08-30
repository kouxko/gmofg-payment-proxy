use std::sync::{
    Arc,
    atomic::{AtomicUsize, Ordering},
};

use chrono::Utc;
use intercept_proxy_domain::{ProtocolPackageId, ProtocolPackageVersion};
use intercept_proxy_package_contract::PackageManifest;
use parking_lot::Mutex;

use super::*;
use crate::SqliteStore;

#[derive(Debug, Default)]
struct FakeLauncher {
    launches: Mutex<Vec<LocalPackageLaunchSpec>>,
    kills: Arc<Mutex<usize>>,
    events: Arc<Mutex<Vec<&'static str>>>,
}

struct FakeProcess {
    kills: Arc<Mutex<usize>>,
    events: Arc<Mutex<Vec<&'static str>>>,
}

#[async_trait]
impl SpawnedLocalPackage for FakeProcess {
    async fn run(self: Box<Self>, kill: oneshot::Receiver<()>) -> std::io::Result<()> {
        let _ = kill.await;
        *self.kills.lock() += 1;
        self.events.lock().push("reap");
        Ok(())
    }
}

#[async_trait]
impl LocalPackageProcessLauncher for FakeLauncher {
    async fn spawn(
        &self,
        spec: &LocalPackageLaunchSpec,
    ) -> std::io::Result<Box<dyn SpawnedLocalPackage>> {
        self.events.lock().push("spawn");
        self.launches.lock().push(spec.clone());
        Ok(Box::new(FakeProcess {
            kills: Arc::clone(&self.kills),
            events: Arc::clone(&self.events),
        }))
    }
}

#[derive(Debug, Default)]
struct ConcurrentLauncher {
    launches: AtomicUsize,
    active: Arc<AtomicUsize>,
    max_active: Arc<AtomicUsize>,
}

struct ConcurrentProcess {
    active: Arc<AtomicUsize>,
    max_active: Arc<AtomicUsize>,
}

#[async_trait]
impl SpawnedLocalPackage for ConcurrentProcess {
    async fn run(self: Box<Self>, kill: oneshot::Receiver<()>) -> std::io::Result<()> {
        let active = self.active.fetch_add(1, Ordering::SeqCst) + 1;
        self.max_active.fetch_max(active, Ordering::SeqCst);
        let _ = kill.await;
        self.active.fetch_sub(1, Ordering::SeqCst);
        Ok(())
    }
}

#[async_trait]
impl LocalPackageProcessLauncher for ConcurrentLauncher {
    async fn spawn(
        &self,
        _: &LocalPackageLaunchSpec,
    ) -> std::io::Result<Box<dyn SpawnedLocalPackage>> {
        self.launches.fetch_add(1, Ordering::SeqCst);
        Ok(Box::new(ConcurrentProcess {
            active: Arc::clone(&self.active),
            max_active: Arc::clone(&self.max_active),
        }))
    }
}

#[derive(Debug)]
struct FailingLauncher;

#[async_trait]
impl LocalPackageProcessLauncher for FailingLauncher {
    async fn spawn(
        &self,
        _: &LocalPackageLaunchSpec,
    ) -> std::io::Result<Box<dyn SpawnedLocalPackage>> {
        Err(std::io::Error::other("spawn failed"))
    }
}

fn package() -> ProtocolPackageRef {
    ProtocolPackageRef {
        id: ProtocolPackageId::new("local.lifecycle").unwrap(),
        version: ProtocolPackageVersion::new("1.0.0").unwrap(),
    }
}

fn manifest() -> PackageManifest {
    serde_json::from_value(serde_json::json!({
        "api": 1,
        "kind": "http",
        "package": {
            "id": "local.lifecycle",
            "name": "Local lifecycle",
            "version": "1.0.0",
            "description": "local lifecycle test package"
        },
        "document": {"upstream": {}, "downstream": {}}
    }))
    .unwrap()
}

#[tokio::test]
async fn restart_kills_and_reaps_old_process_before_next_launch() {
    let store = Arc::new(SqliteStore::in_memory().unwrap());
    store
        .install_local_external_package(&manifest(), b"validated zip", Utc::now())
        .unwrap();
    let registry = Arc::new(ExternalPackageRegistryAdapter::new(Arc::clone(&store)));
    let launcher = Arc::new(FakeLauncher::default());
    let supervisor = LocalPackageSupervisor::with_launcher(
        PathBuf::from("sidecar"),
        "ws://127.0.0.1:8765/packages".to_owned(),
        Duration::from_millis(20),
        registry,
        launcher.clone(),
    );
    let first = supervisor.launch(package(), b"first").await.unwrap_err();
    assert_eq!(first.view_model.code, "EXTERNAL_PACKAGE_TIMEOUT");
    tokio::time::sleep(Duration::from_millis(30)).await;
    assert_eq!(launcher.launches.lock().len(), 1, "timeout must not retry");
    let retained = store.get_external_package(&package()).unwrap().unwrap();
    assert!(retained.enabled, "launch failure must retain enabled state");
    assert_eq!(
        retained.recent_error.unwrap().code,
        "EXTERNAL_PACKAGE_TIMEOUT"
    );
    let second = supervisor.launch(package(), b"second").await.unwrap_err();
    assert_eq!(second.view_model.code, "EXTERNAL_PACKAGE_TIMEOUT");
    assert_eq!(launcher.launches.lock().len(), 2);
    assert_eq!(*launcher.kills.lock(), 2);
    assert_eq!(
        launcher.events.lock().as_slice(),
        &["spawn", "reap", "spawn", "reap"]
    );
}

#[tokio::test]
async fn concurrent_exact_launches_never_own_more_than_one_process() {
    let store = Arc::new(SqliteStore::in_memory().unwrap());
    store
        .install_local_external_package(&manifest(), b"validated zip", Utc::now())
        .unwrap();
    let registry = Arc::new(ExternalPackageRegistryAdapter::new(store));
    let launcher = Arc::new(ConcurrentLauncher::default());
    let supervisor = Arc::new(LocalPackageSupervisor::with_launcher(
        PathBuf::from("sidecar"),
        "ws://127.0.0.1:8765/packages".to_owned(),
        Duration::from_millis(30),
        registry,
        launcher.clone(),
    ));
    let first = {
        let supervisor = Arc::clone(&supervisor);
        tokio::spawn(async move { supervisor.launch(package(), b"first").await })
    };
    let second = {
        let supervisor = Arc::clone(&supervisor);
        tokio::spawn(async move { supervisor.launch(package(), b"second").await })
    };
    let mut error_codes = vec![
        first.await.unwrap().unwrap_err().view_model.code,
        second.await.unwrap().unwrap_err().view_model.code,
    ];
    error_codes.sort();
    assert_eq!(
        error_codes,
        vec![
            "EXTERNAL_PACKAGE_DISCONNECTED".to_string(),
            "EXTERNAL_PACKAGE_TIMEOUT".to_string(),
        ]
    );
    assert_eq!(launcher.launches.load(Ordering::SeqCst), 2);
    assert_eq!(launcher.max_active.load(Ordering::SeqCst), 1);
}

#[tokio::test]
async fn spawn_failure_is_persisted_by_supervisor_without_generic_overwrite() {
    let store = Arc::new(SqliteStore::in_memory().unwrap());
    store
        .install_local_external_package(&manifest(), b"validated zip", Utc::now())
        .unwrap();
    let registry = Arc::new(ExternalPackageRegistryAdapter::new(Arc::clone(&store)));
    let supervisor = LocalPackageSupervisor::with_launcher(
        PathBuf::from("sidecar"),
        "ws://127.0.0.1:8765/packages".to_owned(),
        Duration::from_millis(20),
        registry,
        Arc::new(FailingLauncher),
    );
    let error = supervisor.launch(package(), b"zip").await.unwrap_err();
    assert_eq!(error.view_model.code, "EXTERNAL_PACKAGE_PROCESS_FAILED");
    assert_eq!(
        store
            .get_external_package(&package())
            .unwrap()
            .unwrap()
            .recent_error
            .unwrap()
            .code,
        "EXTERNAL_PACKAGE_PROCESS_FAILED"
    );
}

#[tokio::test]
async fn shutdown_kills_every_owned_process_without_orphans() {
    let registry = Arc::new(ExternalPackageRegistryAdapter::new(Arc::new(
        SqliteStore::in_memory().unwrap(),
    )));
    let launcher = Arc::new(FakeLauncher::default());
    let supervisor = Arc::new(LocalPackageSupervisor::with_launcher(
        PathBuf::from("sidecar"),
        "ws://127.0.0.1:8765/packages".to_owned(),
        Duration::from_secs(30),
        registry,
        launcher.clone(),
    ));
    let task = {
        let supervisor = Arc::clone(&supervisor);
        tokio::spawn(async move { supervisor.launch(package(), b"zip").await })
    };
    while launcher.launches.lock().is_empty() {
        tokio::task::yield_now().await;
    }
    supervisor.shutdown().await;
    let error = task.await.unwrap().unwrap_err();
    assert_eq!(error.view_model.code, "EXTERNAL_PACKAGE_DISCONNECTED");
    assert_eq!(*launcher.kills.lock(), 1);
}
