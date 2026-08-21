use async_trait::async_trait;
use intercept_proxy_application::{
    AppResult, ListenerRuntimePort, ListenerStatusViewModel,
    ListenerUpstreamConnectionTestViewModel, ListenerUpstreamTlsTestViewModel,
};
use intercept_proxy_domain::{ListenerId, ProxyListener, ProxyWorkspace};

use crate::adapters::external_package_server::ExternalPackageListenerRuntime;

#[derive(Debug, Default)]
pub(super) struct UnusedListenerRuntime;

#[async_trait]
impl ListenerRuntimePort for UnusedListenerRuntime {
    async fn statuses(&self) -> AppResult<Vec<ListenerStatusViewModel>> {
        Ok(Vec::new())
    }

    async fn start(
        &self,
        _workspace: ProxyWorkspace,
        _listener: ProxyListener,
    ) -> AppResult<ListenerStatusViewModel> {
        panic!("network E2E does not start an application listener")
    }

    async fn stop(&self, _listener_id: ListenerId) -> AppResult<ListenerStatusViewModel> {
        panic!("empty usage set must not stop a listener")
    }

    async fn replace_protocol_rules(
        &self,
        _workspace: ProxyWorkspace,
        _listener_id: ListenerId,
    ) -> AppResult<()> {
        panic!("network E2E does not replace rules")
    }

    async fn test_upstream_connection(
        &self,
        _workspace: ProxyWorkspace,
        _listener: ProxyListener,
    ) -> AppResult<ListenerUpstreamConnectionTestViewModel> {
        panic!("network E2E does not test an upstream")
    }

    async fn test_upstream_tls(
        &self,
        _workspace: ProxyWorkspace,
        _listener: ProxyListener,
    ) -> AppResult<ListenerUpstreamTlsTestViewModel> {
        panic!("network E2E does not test upstream TLS")
    }
}

#[async_trait]
impl ExternalPackageListenerRuntime for UnusedListenerRuntime {
    async fn current_run_token(&self, _listener_id: ListenerId) -> Option<uuid::Uuid> {
        None
    }

    async fn stop_if_run_token(
        &self,
        _listener_id: ListenerId,
        _expected_run_token: uuid::Uuid,
    ) -> AppResult<Option<ListenerStatusViewModel>> {
        Ok(None)
    }
}
