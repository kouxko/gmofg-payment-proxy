use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
};

use gmofg_proxy_application::Application;
use tokio_util::sync::CancellationToken;

/// Tauri exposes only the application facade to commands.
///
/// Database connections, listeners, certificate private keys and domain
/// collections stay behind application-owned ports.
#[derive(Debug)]
pub struct AppState {
    pub application: Arc<Application>,
    shutdown: CancellationToken,
    shutdown_started: AtomicBool,
}

impl AppState {
    pub fn new(application: Arc<Application>, shutdown: CancellationToken) -> Self {
        Self {
            application,
            shutdown,
            shutdown_started: AtomicBool::new(false),
        }
    }

    pub fn begin_shutdown(&self) -> bool {
        !self.shutdown_started.swap(true, Ordering::AcqRel)
    }

    pub fn shutdown_token(&self) -> CancellationToken {
        self.shutdown.clone()
    }

    pub fn shutdown(&self) {
        self.shutdown.cancel();
    }
}

impl Drop for AppState {
    fn drop(&mut self) {
        self.shutdown();
    }
}
