//! Tauri 托管的进程级应用状态。
//!
//! Command 只能借用这一个入口；数据库、监听任务和证书私钥继续由 host/application
//! 拥有。退出时由 host 的原子门闩保证只有一个关闭流程，`Drop` 只是最终取消兜底。

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
        // host 内部使用原子门闩；true 表示当前调用者赢得唯一关闭任务的所有权。
        self.host.begin_shutdown()
    }

    pub fn shutdown_completed(&self) -> bool {
        self.host.shutdown_completed()
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
        // 正常路径会 await host.shutdown；Drop 无法 await，只能作为进程拆卸时的取消兜底。
        self.shutdown();
    }
}
