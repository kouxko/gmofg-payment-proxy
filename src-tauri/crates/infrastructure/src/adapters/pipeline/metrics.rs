use async_trait::async_trait;

use super::{
    BTreeMap, ChannelId, ChannelRuntimeMetrics, ProxyResult, RuntimeEpoch, RuntimeMetricsProvider,
    RuntimeMetricsSnapshot, RuntimePipelineAdapter, SessionStore, Uuid, app_to_proxy,
};

#[async_trait]
impl RuntimeMetricsProvider for RuntimePipelineAdapter {
    async fn configure_capacity(&self, max_sessions: usize, max_bytes: u64) -> ProxyResult<()> {
        self.events.reclaim_for_limit(max_bytes);
        self.sessions
            .set_limits(max_sessions, max_bytes)
            .map(|_| ())
            .map_err(app_to_proxy)
    }

    async fn snapshot(&self, runtime_epoch: Option<Uuid>) -> ProxyResult<RuntimeMetricsSnapshot> {
        let state = self.state.lock();
        let channels = match runtime_epoch {
            Some(epoch) => state
                .channels
                .get(&RuntimeEpoch::from_uuid(epoch))
                .cloned()
                .unwrap_or_default(),
            None => aggregate_channel_metrics(state.channels.values()),
        };
        let active_sessions = state
            .live_sessions
            .values()
            .filter(|session| runtime_epoch.is_none_or(|epoch| session.runtime_epoch == epoch))
            .count();
        drop(state);
        Ok(RuntimeMetricsSnapshot {
            channels,
            active_sessions,
            logical_memory_bytes: self.sessions.logical_bytes(),
        })
    }
}

fn aggregate_channel_metrics<'a>(
    epochs: impl Iterator<Item = &'a BTreeMap<ChannelId, ChannelRuntimeMetrics>>,
) -> BTreeMap<ChannelId, ChannelRuntimeMetrics> {
    let mut aggregate = BTreeMap::<ChannelId, ChannelRuntimeMetrics>::new();
    for channels in epochs {
        for (channel, metrics) in channels {
            let total = aggregate.entry(channel.clone()).or_default();
            total.connected_clients = total
                .connected_clients
                .saturating_add(metrics.connected_clients);
            total.request_count = total.request_count.saturating_add(metrics.request_count);
            total.error_count = total.error_count.saturating_add(metrics.error_count);
            total.upstream_response_count = total
                .upstream_response_count
                .saturating_add(metrics.upstream_response_count);
            if metrics.last_upstream_error.is_some() {
                total
                    .last_upstream_error
                    .clone_from(&metrics.last_upstream_error);
            }
        }
    }
    aggregate
}
