//! Tauri 托管的进程级应用状态。
//!
//! Command 只能借用这一个入口；数据库、监听任务和证书私钥继续由 host/application
//! 拥有。退出时由 host 的原子门闩保证只有一个关闭流程，`Drop` 只是最终取消兜底。

use std::sync::Arc;

use intercept_proxy_application::Application;
use intercept_proxy_host::ApplicationHost;
use intercept_proxy_infrastructure::ExchangeObservationStore;

use crate::mcp::ReadOnlyMcpServer;
use crate::runtime_logs::RuntimeLogStore;

/// Tauri exposes only the application facade to commands.
///
/// Database connections, listeners, certificate private keys and domain
/// collections stay behind application-owned ports.
#[derive(Debug)]
pub struct AppState {
    pub application: Arc<Application>,
    host: Arc<ApplicationHost>,
    mcp: Option<ReadOnlyMcpServer>,
    runtime_logs: Arc<RuntimeLogStore>,
    exchange_observations: Arc<ExchangeObservationStore>,
}

impl AppState {
    /// Builds command state without outer adapters. Used by command-level tests.
    #[cfg(test)]
    pub fn new(host: ApplicationHost) -> Self {
        let observations = Arc::new(ExchangeObservationStore::new(host.capacity()));
        Self::with_optional_mcp(
            host,
            None,
            Arc::new(RuntimeLogStore::memory(128)),
            observations,
        )
    }

    /// Builds production state after the required IPv4 MCP listener has started successfully.
    /// The optional shape is retained only for command tests that omit outer adapters.
    pub fn production(
        host: ApplicationHost,
        mcp: Option<ReadOnlyMcpServer>,
        runtime_logs: Arc<RuntimeLogStore>,
        exchange_observations: Arc<ExchangeObservationStore>,
    ) -> Self {
        Self::with_optional_mcp(host, mcp, runtime_logs, exchange_observations)
    }

    fn with_optional_mcp(
        host: ApplicationHost,
        mcp: Option<ReadOnlyMcpServer>,
        runtime_logs: Arc<RuntimeLogStore>,
        exchange_observations: Arc<ExchangeObservationStore>,
    ) -> Self {
        let host = Arc::new(host);
        Self {
            application: host.application(),
            host,
            mcp,
            runtime_logs,
            exchange_observations,
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
        if let Some(mcp) = &self.mcp {
            mcp.cancel();
        }
        self.host.cancel_background_tasks();
    }

    pub fn host(&self) -> Arc<ApplicationHost> {
        Arc::clone(&self.host)
    }

    pub fn mcp(&self) -> Option<ReadOnlyMcpServer> {
        self.mcp.clone()
    }

    pub(crate) fn runtime_logs(&self) -> Arc<RuntimeLogStore> {
        Arc::clone(&self.runtime_logs)
    }

    pub(crate) fn exchange_observations(&self) -> Arc<ExchangeObservationStore> {
        Arc::clone(&self.exchange_observations)
    }
}

impl Drop for AppState {
    fn drop(&mut self) {
        // 正常路径会 await host.shutdown；Drop 无法 await，只能作为进程拆卸时的取消兜底。
        self.shutdown();
    }
}
