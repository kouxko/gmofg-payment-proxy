use std::sync::Arc;

use gmofg_proxy_application::Application;
use gmofg_proxy_host::ApplicationHost;

/// Tauri exposes only the application facade to commands.
///
/// Database connections, listeners, certificate private keys and domain
/// collections stay behind application-owned ports.
#[derive(Debug)]
pub struct AppState {
    pub application: Arc<Application>,
    host: Arc<ApplicationHost>,
}

impl AppState {
    pub fn new(host: ApplicationHost) -> Self {
        let host = Arc::new(host);
        Self {
            application: host.application(),
            host,
        }
    }

    pub fn begin_shutdown(&self) -> bool {
        self.host.begin_shutdown()
    }

    pub fn shutdown(&self) {
        self.host.cancel_background_tasks();
    }

    pub fn host(&self) -> Arc<ApplicationHost> {
        Arc::clone(&self.host)
    }
}

impl Drop for AppState {
    fn drop(&mut self) {
        self.shutdown();
    }
}
