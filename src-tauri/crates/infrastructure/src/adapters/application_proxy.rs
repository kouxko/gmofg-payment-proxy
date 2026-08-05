//! 应用代理端口到 Tokio supervisor 的转换层。
//!
//! 它把设置 DTO 组装成运行时配置，并把 epoch、监听地址与指标映射回界面模型；Tauri
//! host 不参与业务转换。设置锁只保护当前快照，启动/停止由 supervisor 自己串行化。

use std::net::{IpAddr, SocketAddr};
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Duration;

use async_trait::async_trait;
use intercept_proxy_application::{
    AppError, AppResult, ChannelState, ChannelStatusViewModel, ConnectionHealthState,
    ConnectionHealthViewModel, DisabledReason, ProxyState as ApplicationProxyState,
    ProxyStatusViewModel, ProxySupervisorPort, SettingsDraft, UiTone,
};
use intercept_proxy_product_api::ProductLabels;
use intercept_proxy_runtime::{
    ChannelConfig, ChannelId as RuntimeChannelId, ChannelRuntimeMetrics, DEFAULT_MAX_CONNECTIONS,
    MessageLimits, ProxyConfig, ProxyError, ProxyState, ProxySupervisor, RuntimeMetricsProvider,
    RuntimeSnapshot,
};
use tokio::sync::RwLock;

mod health;
mod mapping;

use health::{format_iec_bytes, inbound_health, memory_percent, outbound_health, upstream_status};
use mapping::{disabled_reason, map_channel_state, map_error, map_state, proxy_config};

/// Maps application DTOs to the transport supervisor without putting business
/// conversion logic in `src-tauri`.
#[derive(Debug)]
pub struct ApplicationProxyAdapter {
    supervisor: Arc<ProxySupervisor>,
    metrics: Arc<dyn RuntimeMetricsProvider>,
    settings: RwLock<SettingsDraft>,
    labels: ProductLabels,
    revision: AtomicU64,
}

impl ApplicationProxyAdapter {
    pub fn new(
        supervisor: Arc<ProxySupervisor>,
        initial_settings: SettingsDraft,
        metrics: Arc<dyn RuntimeMetricsProvider>,
        labels: ProductLabels,
    ) -> Self {
        Self {
            supervisor,
            metrics,
            settings: RwLock::new(initial_settings),
            labels,
            revision: AtomicU64::new(0),
        }
    }

    async fn view_model(&self, snapshot: RuntimeSnapshot) -> AppResult<ProxyStatusViewModel> {
        let metrics = self
            .metrics
            .snapshot(snapshot.runtime_epoch)
            .await
            .map_err(map_error)?;
        let settings = self.settings.read().await;
        let (state, state_text, ui_tone) = map_state(snapshot.state);
        let channels = settings
            .channels
            .iter()
            .map(|configured| -> AppResult<_> {
                let runtime_channel =
                    RuntimeChannelId::new(configured.id.as_str()).map_err(map_error)?;
                let channel_metrics = metrics
                    .channels
                    .get(&runtime_channel)
                    .cloned()
                    .unwrap_or_default();
                let listen_address = snapshot.listeners.get(&runtime_channel).map_or_else(
                    || format!("{}:{}", settings.bind_address, configured.port),
                    ToString::to_string,
                );
                let channel_state = map_channel_state(snapshot.state, configured.enabled);
                let (channel_text, channel_tone) = channel_state.display_zh();
                let (upstream_state_text, upstream_ui_tone) =
                    upstream_status(snapshot.state, configured.enabled, &channel_metrics);
                Ok(ChannelStatusViewModel {
                    id: configured.id.clone(),
                    display_name: configured.display_name.clone(),
                    state: channel_state,
                    state_text: channel_text.to_owned(),
                    ui_tone: channel_tone,
                    listen_address,
                    mtls_enabled: true,
                    connected_clients: channel_metrics.connected_clients,
                    request_count: channel_metrics.request_count,
                    error_count: channel_metrics.error_count,
                    enabled: configured.enabled,
                    upstream_url: configured.upstream_url.clone(),
                    upstream_state_text,
                    upstream_ui_tone,
                })
            })
            .collect::<AppResult<Vec<_>>>()?;
        let app_to_proxy_health = inbound_health(snapshot.state, &channels, self.labels);
        let proxy_to_server_health = outbound_health(snapshot.state, &channels, self.labels);
        Ok(ProxyStatusViewModel {
            state,
            state_text,
            ui_tone,
            runtime_epoch: snapshot.runtime_epoch,
            revision: self.revision.load(Ordering::Relaxed),
            channels,
            app_to_proxy_health,
            proxy_to_server_health,
            active_sessions: metrics.active_sessions,
            pending_breakpoints: metrics.pending_breakpoints,
            logical_memory_bytes: metrics.logical_memory_bytes,
            logical_memory_text: format_iec_bytes(metrics.logical_memory_bytes),
            memory_capacity_bytes: settings.max_memory_bytes,
            memory_capacity_text: format_iec_bytes(settings.max_memory_bytes),
            memory_usage_percent: memory_percent(
                metrics.logical_memory_bytes,
                settings.max_memory_bytes,
            ),
            session_capacity: settings.max_sessions,
            default_timeout_seconds: settings.read_timeout_seconds,
            can_start: snapshot.state == ProxyState::Stopped,
            start_disabled_reason: disabled_reason(
                snapshot.state != ProxyState::Stopped,
                "当前状态不可启动代理",
            ),
            can_stop: matches!(snapshot.state, ProxyState::Running | ProxyState::Faulted),
            stop_disabled_reason: disabled_reason(
                !matches!(snapshot.state, ProxyState::Running | ProxyState::Faulted),
                "当前状态无需停止代理",
            ),
            can_restart: snapshot.state == ProxyState::Running,
            restart_disabled_reason: disabled_reason(
                snapshot.state != ProxyState::Running,
                "仅运行中代理可重启",
            ),
            fault_reason: snapshot.fault,
        })
    }
}

/*
 * 状态健康度和 DTO 映射分别位于 `application_proxy/health.rs` 与
 * `application_proxy/mapping.rs`，这里仅保留 supervisor 协调职责。
 */

#[async_trait]
impl ProxySupervisorPort for ApplicationProxyAdapter {
    async fn status(&self) -> AppResult<ProxyStatusViewModel> {
        self.view_model(self.supervisor.snapshot().await).await
    }

    async fn start(&self, effective_settings: SettingsDraft) -> AppResult<ProxyStatusViewModel> {
        let config = proxy_config(&effective_settings)?;
        self.metrics
            .configure_capacity(
                effective_settings.max_sessions,
                effective_settings.max_memory_bytes,
            )
            .await
            .map_err(map_error)?;
        let snapshot = self.supervisor.start(config).await.map_err(map_error)?;
        *self.settings.write().await = effective_settings;
        self.revision.fetch_add(1, Ordering::Relaxed);
        self.view_model(snapshot).await
    }

    async fn stop(&self) -> AppResult<ProxyStatusViewModel> {
        let snapshot = self.supervisor.stop().await.map_err(map_error)?;
        self.revision.fetch_add(1, Ordering::Relaxed);
        self.view_model(snapshot).await
    }
}

#[cfg(test)]
#[path = "application_proxy/tests.rs"]
mod tests;
