//! Application-port adapter. Business mapping stays out of the host shell.

use std::net::{IpAddr, SocketAddr};
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Duration;

use async_trait::async_trait;
use gmofg_proxy_application::{
    AppError, AppResult, ChannelKind, ChannelState, ChannelStatusViewModel, ConnectionHealthState,
    ConnectionHealthViewModel, DisabledReason, ProxyState as ApplicationProxyState,
    ProxyStatusViewModel, ProxySupervisorPort, SettingsDraft, UiTone,
};
use gmofg_proxy_runtime::{
    Channel, ChannelConfig, ChannelRuntimeMetrics, MessageLimits, ProxyConfig, ProxyError,
    ProxyState, ProxySupervisor, RuntimeMetricsProvider, RuntimeSnapshot,
};
use tokio::sync::RwLock;

/// Maps application DTOs to the transport supervisor without putting business
/// conversion logic in `src-tauri`.
#[derive(Debug)]
pub struct ApplicationProxyAdapter {
    supervisor: Arc<ProxySupervisor>,
    metrics: Arc<dyn RuntimeMetricsProvider>,
    settings: RwLock<SettingsDraft>,
    revision: AtomicU64,
}

impl ApplicationProxyAdapter {
    pub fn new(
        supervisor: Arc<ProxySupervisor>,
        initial_settings: SettingsDraft,
        metrics: Arc<dyn RuntimeMetricsProvider>,
    ) -> Self {
        Self {
            supervisor,
            metrics,
            settings: RwLock::new(initial_settings),
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
        let channels = [
            (
                Channel::Transaction,
                ChannelKind::Transaction,
                settings.transaction_enabled,
                settings.transaction_port,
            ),
            (
                Channel::Dll,
                ChannelKind::Dll,
                settings.dll_enabled,
                settings.dll_port,
            ),
        ]
        .into_iter()
        .map(|(runtime_kind, kind, enabled, configured_port)| {
            let channel_metrics = metrics
                .channels
                .get(&runtime_kind)
                .cloned()
                .unwrap_or_default();
            let listen_address = snapshot.listeners.get(&runtime_kind).map_or_else(
                || format!("{}:{configured_port}", settings.bind_address),
                ToString::to_string,
            );
            let channel_state = map_channel_state(snapshot.state, enabled);
            let (channel_text, channel_tone) = channel_state.display_zh();
            let (upstream_state_text, upstream_ui_tone) =
                upstream_status(snapshot.state, enabled, &channel_metrics);
            ChannelStatusViewModel {
                kind,
                display_name: kind.display_zh().to_owned(),
                state: channel_state,
                state_text: channel_text.to_owned(),
                ui_tone: channel_tone,
                listen_address,
                mtls_enabled: true,
                connected_clients: channel_metrics.connected_clients,
                request_count: channel_metrics.request_count,
                error_count: channel_metrics.error_count,
                enabled,
                upstream_url: match runtime_kind {
                    Channel::Transaction => settings.upstream_transaction_url.clone(),
                    Channel::Dll => settings.upstream_dll_url.clone(),
                },
                upstream_state_text,
                upstream_ui_tone,
            }
        })
        .collect::<Vec<_>>();
        let app_to_proxy_health = inbound_health(snapshot.state, &channels);
        let proxy_to_server_health = outbound_health(snapshot.state, &channels);
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

fn inbound_health(
    state: ProxyState,
    channels: &[ChannelStatusViewModel],
) -> ConnectionHealthViewModel {
    match state {
        ProxyState::Running => {
            let listening = channels
                .iter()
                .filter(|channel| channel.enabled)
                .all(|channel| channel.state == ChannelState::Listening && channel.mtls_enabled);
            if listening {
                health(
                    ConnectionHealthState::Healthy,
                    "mTLS 监听正常",
                    "所有已启用 App → Proxy 监听器均已启动并要求客户端证书。",
                    UiTone::Positive,
                )
            } else {
                health(
                    ConnectionHealthState::Degraded,
                    "监听状态不完整",
                    "至少一个已启用 App → Proxy 监听器尚未就绪。",
                    UiTone::Warning,
                )
            }
        }
        ProxyState::Starting => health(
            ConnectionHealthState::Waiting,
            "正在启动",
            "正在建立 App → Proxy mTLS 监听器。",
            UiTone::Info,
        ),
        ProxyState::Faulted => health(
            ConnectionHealthState::Faulted,
            "监听故障",
            "App → Proxy 监听器发生故障，请查看运行错误。",
            UiTone::Danger,
        ),
        ProxyState::Stopped | ProxyState::Stopping => health(
            ConnectionHealthState::Unavailable,
            "未监听",
            "Proxy 当前未接收 Payment App 连接。",
            UiTone::Neutral,
        ),
    }
}

fn outbound_health(
    state: ProxyState,
    channels: &[ChannelStatusViewModel],
) -> ConnectionHealthViewModel {
    if state != ProxyState::Running {
        return health(
            if state == ProxyState::Faulted {
                ConnectionHealthState::Faulted
            } else {
                ConnectionHealthState::Unavailable
            },
            if state == ProxyState::Faulted {
                "上游路径故障"
            } else {
                "尚未连接"
            },
            "Proxy → Server 连接按请求建立，当前没有可报告的运行中路径。",
            if state == ProxyState::Faulted {
                UiTone::Danger
            } else {
                UiTone::Neutral
            },
        );
    }
    let enabled = channels
        .iter()
        .filter(|channel| channel.enabled)
        .collect::<Vec<_>>();
    if enabled
        .iter()
        .any(|channel| channel.upstream_ui_tone == UiTone::Danger)
    {
        health(
            ConnectionHealthState::Faulted,
            "最近上游连接失败",
            "至少一个通道的最近一次 Proxy → Server 连接失败。",
            UiTone::Danger,
        )
    } else if enabled
        .iter()
        .all(|channel| channel.upstream_ui_tone == UiTone::Positive)
    {
        health(
            ConnectionHealthState::Healthy,
            "上游路径已验证",
            "所有已启用通道均收到过有效的上游响应。",
            UiTone::Positive,
        )
    } else {
        health(
            ConnectionHealthState::Waiting,
            "等待上游请求",
            "Proxy → Server 按交易建立连接；尚未收到所有通道的首个上游响应。",
            UiTone::Info,
        )
    }
}

fn upstream_status(
    state: ProxyState,
    enabled: bool,
    metrics: &ChannelRuntimeMetrics,
) -> (String, UiTone) {
    if !enabled {
        return ("通道已禁用".into(), UiTone::Neutral);
    }
    if state != ProxyState::Running {
        return ("尚未连接".into(), UiTone::Neutral);
    }
    if let Some(error) = &metrics.last_upstream_error {
        return (format!("最近失败：{error}"), UiTone::Danger);
    }
    if metrics.upstream_response_count > 0 {
        return (
            format!("已收到 {} 个上游响应", metrics.upstream_response_count),
            UiTone::Positive,
        );
    }
    ("等待首个上游响应".into(), UiTone::Info)
}

fn health(
    state: ConnectionHealthState,
    state_text: &str,
    detail: &str,
    ui_tone: UiTone,
) -> ConnectionHealthViewModel {
    ConnectionHealthViewModel {
        state,
        state_text: state_text.into(),
        detail: detail.into(),
        ui_tone,
    }
}

fn format_iec_bytes(bytes: u64) -> String {
    const MIB: u64 = 1024 * 1024;
    const KIB: u64 = 1024;
    if bytes >= MIB {
        format!(
            "{}.{} MiB",
            bytes / MIB,
            (bytes % MIB).saturating_mul(10) / MIB
        )
    } else if bytes >= KIB {
        format!(
            "{}.{} KiB",
            bytes / KIB,
            (bytes % KIB).saturating_mul(10) / KIB
        )
    } else {
        format!("{bytes} B")
    }
}

fn memory_percent(used: u64, capacity: u64) -> u8 {
    if capacity == 0 {
        return 0;
    }
    let percent = u128::from(used)
        .saturating_mul(100)
        .div_ceil(u128::from(capacity))
        .min(100);
    u8::try_from(percent).unwrap_or(100)
}

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

fn proxy_config(settings: &SettingsDraft) -> AppResult<ProxyConfig> {
    let bind_address = settings
        .bind_address
        .parse::<IpAddr>()
        .map_err(|error| AppError::new("CONFIG_INVALID", format!("绑定地址无效: {error}")))?;
    let max_body_bytes = usize::try_from(settings.max_body_bytes)
        .map_err(|_| AppError::new("CONFIG_INVALID", "Body 上限超出平台范围"))?;
    Ok(ProxyConfig {
        channels: vec![
            ChannelConfig {
                channel: Channel::Transaction,
                enabled: settings.transaction_enabled,
                listen_addr: SocketAddr::new(bind_address, settings.transaction_port),
                upstream_url: settings.upstream_transaction_url.clone(),
            },
            ChannelConfig {
                channel: Channel::Dll,
                enabled: settings.dll_enabled,
                listen_addr: SocketAddr::new(bind_address, settings.dll_port),
                upstream_url: settings.upstream_dll_url.clone(),
            },
        ],
        limits: MessageLimits {
            max_body_bytes,
            ..MessageLimits::default()
        },
        connect_timeout: Duration::from_secs(settings.connect_timeout_seconds),
        write_timeout: Duration::from_secs(settings.write_timeout_seconds),
        read_timeout: Duration::from_secs(settings.read_timeout_seconds),
        rewrite_host: settings.rewrite_host,
        leaf_sans: settings.leaf_sans.clone(),
    })
}

fn map_state(state: ProxyState) -> (ApplicationProxyState, String, UiTone) {
    let state = match state {
        ProxyState::Stopped => ApplicationProxyState::Stopped,
        ProxyState::Starting => ApplicationProxyState::Starting,
        ProxyState::Running => ApplicationProxyState::Running,
        ProxyState::Stopping => ApplicationProxyState::Stopping,
        ProxyState::Faulted => ApplicationProxyState::Faulted,
    };
    let (text, tone) = state.display_zh();
    (state, text.to_owned(), tone)
}

fn map_channel_state(state: ProxyState, enabled: bool) -> ChannelState {
    if !enabled {
        return ChannelState::Disabled;
    }
    match state {
        ProxyState::Stopped => ChannelState::Stopped,
        ProxyState::Starting => ChannelState::Starting,
        ProxyState::Running => ChannelState::Listening,
        ProxyState::Stopping => ChannelState::Stopping,
        ProxyState::Faulted => ChannelState::Faulted,
    }
}

fn disabled_reason(disabled: bool, message: &str) -> Option<DisabledReason> {
    disabled.then(|| DisabledReason {
        code: "OPERATION_NOT_ALLOWED".to_owned(),
        message: message.to_owned(),
    })
}

fn map_error(error: ProxyError) -> AppError {
    AppError::new(error.code, error.message)
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use gmofg_proxy_runtime::transport::ConnectionService;
    use gmofg_proxy_runtime::{
        ChannelRuntimeMetrics, Result, RuntimeMetricsSnapshot, RuntimeServiceFactory,
        TokioListenerBinder,
    };
    use uuid::Uuid;

    use super::*;

    #[derive(Debug)]
    struct StaticMetrics(RuntimeMetricsSnapshot);

    #[async_trait]
    impl RuntimeMetricsProvider for StaticMetrics {
        async fn snapshot(&self, _runtime_epoch: Option<Uuid>) -> Result<RuntimeMetricsSnapshot> {
            Ok(self.0.clone())
        }
    }

    #[derive(Debug)]
    struct UnusedRuntimeServiceFactory;

    #[async_trait]
    impl RuntimeServiceFactory for UnusedRuntimeServiceFactory {
        async fn build(
            &self,
            _config: &ProxyConfig,
        ) -> Result<BTreeMap<Channel, ConnectionService>> {
            unreachable!("status does not build runtime services")
        }
    }

    #[tokio::test]
    async fn maps_runtime_metrics_without_fixed_zeroes() {
        let supervisor = Arc::new(ProxySupervisor::with_factory(
            Arc::new(TokioListenerBinder),
            Arc::new(UnusedRuntimeServiceFactory),
        ));
        let metrics = RuntimeMetricsSnapshot {
            channels: BTreeMap::from([
                (
                    Channel::Transaction,
                    ChannelRuntimeMetrics {
                        connected_clients: 3,
                        request_count: 17,
                        error_count: 2,
                        ..ChannelRuntimeMetrics::default()
                    },
                ),
                (
                    Channel::Dll,
                    ChannelRuntimeMetrics {
                        connected_clients: 1,
                        request_count: 5,
                        error_count: 4,
                        ..ChannelRuntimeMetrics::default()
                    },
                ),
            ]),
            active_sessions: 4,
            pending_breakpoints: 6,
            logical_memory_bytes: 8_192,
        };
        let adapter = ApplicationProxyAdapter::new(
            supervisor,
            SettingsDraft::default(),
            Arc::new(StaticMetrics(metrics)),
        );

        let status = adapter.status().await.unwrap();

        assert_eq!(status.channels[0].connected_clients, 3);
        assert_eq!(status.channels[0].request_count, 17);
        assert_eq!(status.channels[0].error_count, 2);
        assert_eq!(status.channels[1].connected_clients, 1);
        assert_eq!(status.channels[1].request_count, 5);
        assert_eq!(status.channels[1].error_count, 4);
        assert_eq!(status.active_sessions, 4);
        assert_eq!(status.pending_breakpoints, 6);
        assert_eq!(status.logical_memory_bytes, 8_192);
    }
}
