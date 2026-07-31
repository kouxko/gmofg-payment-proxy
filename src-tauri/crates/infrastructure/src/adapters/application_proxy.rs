//! 应用代理端口到 Tokio supervisor 的转换层。
//!
//! 它把设置 DTO 组装成运行时配置，并把 epoch、监听地址与指标映射回界面模型；Tauri
//! host 不参与业务转换。设置锁只保护当前快照，启动/停止由 supervisor 自己串行化。

use std::net::{IpAddr, SocketAddr};
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Duration;

use async_trait::async_trait;
use gmofg_proxy_application::{
    AppError, AppResult, ChannelState, ChannelStatusViewModel, ConnectionHealthState,
    ConnectionHealthViewModel, DisabledReason, ProxyState as ApplicationProxyState,
    ProxyStatusViewModel, ProxySupervisorPort, SettingsDraft, UiTone,
};
use gmofg_proxy_product_api::ProductLabels;
use gmofg_proxy_runtime::{
    ChannelConfig, ChannelId as RuntimeChannelId, ChannelRuntimeMetrics, DEFAULT_MAX_CONNECTIONS,
    MessageLimits, ProxyConfig, ProxyError, ProxyState, ProxySupervisor, RuntimeMetricsProvider,
    RuntimeSnapshot,
};
use tokio::sync::RwLock;

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

fn inbound_health(
    state: ProxyState,
    channels: &[ChannelStatusViewModel],
    labels: ProductLabels,
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
                    &format!(
                        "所有已启用 {} → Proxy 监听器均已启动并要求客户端证书。",
                        labels.client_name
                    ),
                    UiTone::Positive,
                )
            } else {
                health(
                    ConnectionHealthState::Degraded,
                    "监听状态不完整",
                    &format!(
                        "至少一个已启用 {} → Proxy 监听器尚未就绪。",
                        labels.client_name
                    ),
                    UiTone::Warning,
                )
            }
        }
        ProxyState::Starting => health(
            ConnectionHealthState::Waiting,
            "正在启动",
            &format!("正在建立 {} → Proxy mTLS 监听器。", labels.client_name),
            UiTone::Info,
        ),
        ProxyState::Faulted => health(
            ConnectionHealthState::Faulted,
            "监听故障",
            &format!(
                "{} → Proxy 监听器发生故障，请查看运行错误。",
                labels.client_name
            ),
            UiTone::Danger,
        ),
        ProxyState::Stopped | ProxyState::Stopping => health(
            ConnectionHealthState::Unavailable,
            "未监听",
            &format!("Proxy 当前未接收 {} 连接。", labels.client_name),
            UiTone::Neutral,
        ),
    }
}

fn outbound_health(
    state: ProxyState,
    channels: &[ChannelStatusViewModel],
    labels: ProductLabels,
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
            &format!(
                "Proxy → {} 连接按请求建立，当前没有可报告的运行中路径。",
                labels.upstream_name
            ),
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
            &format!(
                "至少一个通道的最近一次 Proxy → {} 连接失败。",
                labels.upstream_name
            ),
            UiTone::Danger,
        )
    } else if enabled
        .iter()
        .all(|channel| channel.upstream_ui_tone == UiTone::Positive)
    {
        health(
            ConnectionHealthState::Healthy,
            "上游路径已验证",
            &format!(
                "所有已启用通道均收到过有效的 {} 响应。",
                labels.upstream_name
            ),
            UiTone::Positive,
        )
    } else {
        health(
            ConnectionHealthState::Waiting,
            "等待上游请求",
            &format!(
                "Proxy → {} 按请求建立连接；尚未收到所有通道的首个上游响应。",
                labels.upstream_name
            ),
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
        channels: settings
            .channels
            .iter()
            .map(|channel| {
                Ok(ChannelConfig {
                    channel: RuntimeChannelId::new(channel.id.as_str()).map_err(map_error)?,
                    enabled: channel.enabled,
                    listen_addr: SocketAddr::new(bind_address, channel.port),
                    upstream_url: channel.upstream_url.clone(),
                })
            })
            .collect::<AppResult<Vec<_>>>()?,
        limits: MessageLimits {
            max_body_bytes,
            ..MessageLimits::default()
        },
        max_connections: DEFAULT_MAX_CONNECTIONS,
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

    const TEST_LABELS: ProductLabels = ProductLabels {
        client_name: "Test Client",
        upstream_name: "Test Upstream",
        fault_rule_name_prefix: "Fault · ",
    };

    fn test_settings() -> SettingsDraft {
        SettingsDraft {
            channels: ["alpha", "beta", "gamma"]
                .into_iter()
                .enumerate()
                .map(
                    |(index, id)| gmofg_proxy_application::ChannelSettingsDraft {
                        id: gmofg_proxy_domain::ChannelId::new(id).unwrap(),
                        display_name: id.to_uppercase(),
                        enabled: true,
                        port: 20_001 + u16::try_from(index).unwrap(),
                        upstream_url: format!("https://{id}.example.test"),
                    },
                )
                .collect(),
            ..SettingsDraft::default()
        }
    }

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
        ) -> Result<BTreeMap<RuntimeChannelId, ConnectionService>> {
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
                    RuntimeChannelId::new("alpha").unwrap(),
                    ChannelRuntimeMetrics {
                        connected_clients: 3,
                        request_count: 17,
                        error_count: 2,
                        ..ChannelRuntimeMetrics::default()
                    },
                ),
                (
                    RuntimeChannelId::new("beta").unwrap(),
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
            test_settings(),
            Arc::new(StaticMetrics(metrics)),
            TEST_LABELS,
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

    #[test]
    fn retained_session_capacity_does_not_change_live_connection_admission() {
        let settings = SettingsDraft {
            max_sessions: 1,
            ..test_settings()
        };

        let config = proxy_config(&settings).expect("proxy config");

        assert_eq!(config.max_connections, DEFAULT_MAX_CONNECTIONS);
    }

    #[test]
    fn arbitrary_channel_ids_flow_into_runtime_config() {
        let config = proxy_config(&test_settings()).expect("proxy config");

        assert_eq!(config.channels[0].channel.as_str(), "alpha");
        assert_eq!(config.channels[1].channel.as_str(), "beta");
        assert_eq!(config.channels[2].channel.as_str(), "gamma");
    }
}
