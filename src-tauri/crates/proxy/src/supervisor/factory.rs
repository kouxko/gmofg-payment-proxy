use super::{
    BTreeMap, ChannelId, ConnectionAdmission, ConnectionService, ProxyConfig, Result, async_trait,
};

/// Start-time composition seam for upstream URLs, timeout policy and TLS
/// certificate snapshots. Implementations are called once per enabled channel
/// before the epoch becomes visible.
#[async_trait]
pub trait RuntimeServiceFactory: std::fmt::Debug + Send + Sync {
    async fn build(&self, config: &ProxyConfig) -> Result<BTreeMap<ChannelId, ConnectionService>>;
}

#[derive(Debug, Clone)]
pub(super) struct StaticRuntimeServiceFactory {
    pub(super) service: ConnectionService,
}

#[async_trait]
impl RuntimeServiceFactory for StaticRuntimeServiceFactory {
    async fn build(&self, config: &ProxyConfig) -> Result<BTreeMap<ChannelId, ConnectionService>> {
        let mut service = self.service.clone();
        service.limits = config.limits;
        service.write_timeout = config.write_timeout;
        service.read_timeout = config.read_timeout;
        service.admission = ConnectionAdmission::new(config.max_connections)?;
        Ok(config
            .channels
            .iter()
            .filter(|channel| channel.enabled)
            .map(|channel| (channel.channel.clone(), service.clone()))
            .collect())
    }
}
