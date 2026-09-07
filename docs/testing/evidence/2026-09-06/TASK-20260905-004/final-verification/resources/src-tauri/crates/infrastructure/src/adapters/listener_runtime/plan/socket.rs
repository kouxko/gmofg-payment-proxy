//! Direct Socket runtime assembly for `RemoteServer` and `LocalServer` topologies.

use std::{net::SocketAddr, sync::Arc};

use intercept_proxy_application::AppResult;
use intercept_proxy_domain::{
    ProxyListener, ProxyWorkspace, SocketPayloadProcessing, SocketRelaySettings,
};
use intercept_proxy_runtime::SocketRelayService;

use super::{
    ListenerRuntimePlanBuilder, PreparedListenerRuntime, runtime_error,
    socket_config::SocketServiceConfig,
};

impl ListenerRuntimePlanBuilder<'_> {
    pub(super) async fn build_socket(
        &self,
        workspace: &ProxyWorkspace,
        listener: &ProxyListener,
        socket: &SocketRelaySettings,
        bind_addr: SocketAddr,
        full_runtime: bool,
    ) -> AppResult<PreparedListenerRuntime> {
        if !full_runtime {
            return self
                .build_socket_probe(workspace, listener, socket, bind_addr)
                .await;
        }
        if matches!(&socket.processing, SocketPayloadProcessing::Scripted(_)) {
            return self
                .build_scripted_socket(workspace, listener, socket, bind_addr)
                .await;
        }
        let observer = self.socket_observer(listener, socket)?;
        let service = match self
            .socket_service_config(workspace, listener, socket, bind_addr)
            .await?
        {
            SocketServiceConfig::Relay(config) => {
                SocketRelayService::build_with_observer(config, observer)
            }
            SocketServiceConfig::LocalResponder(config) => {
                SocketRelayService::build_local_raw_responder_with_observer(config, observer)
            }
        }
        .map_err(|error| runtime_error(listener, error.code, error.message))?;
        Ok(PreparedListenerRuntime::Socket {
            bind_addr,
            service: Arc::new(service),
        })
    }
}
