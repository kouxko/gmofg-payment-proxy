use super::{
    ChannelRuntimeMetrics, ChannelState, ChannelStatusViewModel, ConnectionHealthState,
    ConnectionHealthViewModel, ProductLabels, ProxyState, UiTone,
};

pub(super) fn inbound_health(
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

pub(super) fn outbound_health(
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

pub(super) fn upstream_status(
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

pub(super) fn format_iec_bytes(bytes: u64) -> String {
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

pub(super) fn memory_percent(used: u64, capacity: u64) -> u8 {
    if capacity == 0 {
        return 0;
    }
    let percent = u128::from(used)
        .saturating_mul(100)
        .div_ceil(u128::from(capacity))
        .min(100);
    u8::try_from(percent).unwrap_or(100)
}
