use intercept_proxy_runtime::http::ConnectionService;
use intercept_proxy_runtime::{
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
                |(index, id)| intercept_proxy_application::ChannelSettingsDraft {
                    id: intercept_proxy_domain::ChannelId::new(id).unwrap(),
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
use std::collections::BTreeMap;
