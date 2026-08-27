use std::sync::Arc;

use intercept_proxy_application::{
    AppError, AppResult, ListenerId, ListenerStatusViewModel, ProxyListener, ProxyWorkspace,
};
use intercept_proxy_domain::SocketTopology;
use parking_lot::RwLock;
use tokio_util::sync::CancellationToken;

use super::{
    ListenerRuntimeAdapter, ListenerRuntimePlanBuilder, PreparedListenerRuntime, RunningListener,
    bind_tcp_listener, running_status,
};

impl ListenerRuntimeAdapter {
    async fn prepare_start(
        &self,
        workspace: &ProxyWorkspace,
        listener: &ProxyListener,
        runtime_epoch: uuid::Uuid,
    ) -> AppResult<(PreparedListenerRuntime, tokio::net::TcpListener)> {
        let plan = ListenerRuntimePlanBuilder::new(self)
            .build(workspace, listener, runtime_epoch)
            .await?;
        if let Some(snapshot) = plan.scripted_snapshot()
            && matches!(snapshot.topology(), SocketTopology::LocalResponder(_))
            && matches!(
                &plan,
                PreparedListenerRuntime::ScriptedSocket { service: None, .. }
            )
        {
            return Err(AppError::new(
                "LOCAL_RESPONDER_PLAN_INVALID",
                "LocalResponder 未能构造本地应答运行服务。",
            )
            .entity(listener.id.to_string()));
        }
        let tcp_listener = bind_tcp_listener(plan.bind_addr(), listener.id).await?;
        Ok((plan, tcp_listener))
    }

    pub(super) async fn start_reserved(
        &self,
        workspace: ProxyWorkspace,
        listener: ProxyListener,
        runtime_epoch: uuid::Uuid,
        caller_cancellation: CancellationToken,
    ) -> AppResult<ListenerStatusViewModel> {
        let listener_id = listener.id;
        #[cfg(test)]
        let start_barrier = { self.start_barriers.lock().await.remove(&listener_id) };
        #[cfg(test)]
        let _completion = start_barrier
            .as_ref()
            .map(|barrier| StartBarrierCompletion(Arc::clone(&barrier.completed)));
        #[cfg(test)]
        if let Some(barrier) = start_barrier {
            barrier.reached.notify_one();
            tokio::select! {
                biased;
                () = caller_cancellation.cancelled() => {
                    self.release_start(listener_id).await;
                    return Err(start_cancelled(listener_id));
                }
                () = barrier.release.notified() => {}
            }
        }
        let prepared = tokio::select! {
            biased;
            () = caller_cancellation.cancelled() => {
                self.release_start(listener_id).await;
                return Err(start_cancelled(listener_id));
            }
            result = self.prepare_start(&workspace, &listener, runtime_epoch) => result,
        };
        let (plan, tcp_listener) = match prepared {
            Ok(prepared) => prepared,
            Err(error) => {
                self.release_start(listener_id).await;
                return Err(error);
            }
        };
        self.commit_prepared_start(
            workspace,
            listener,
            runtime_epoch,
            caller_cancellation,
            plan,
            tcp_listener,
        )
        .await
    }

    async fn commit_prepared_start(
        &self,
        workspace: ProxyWorkspace,
        listener: ProxyListener,
        runtime_epoch: uuid::Uuid,
        caller_cancellation: CancellationToken,
        plan: PreparedListenerRuntime,
        tcp_listener: tokio::net::TcpListener,
    ) -> AppResult<ListenerStatusViewModel> {
        let listener_id = listener.id;
        let scripted_snapshot = plan.scripted_snapshot();
        let external_socket_snapshot = plan.external_socket_snapshot();
        let http_protocol_snapshot = plan.http_protocol_snapshot();
        let listen_address = plan.bind_addr().to_string();
        let cancellation = CancellationToken::new();
        let task_cancellation = cancellation.clone();
        let workspace_id = workspace.id.to_string();
        let fault = Arc::new(RwLock::new(None));
        let task_fault = Arc::clone(&fault);
        let socket_service = match &plan {
            PreparedListenerRuntime::Socket { service, .. }
            | PreparedListenerRuntime::ExternalScriptedSocket { service, .. }
            | PreparedListenerRuntime::ScriptedSocket {
                service: Some(service),
                ..
            } => Some(Arc::clone(service)),
            _ => None,
        };
        let pipeline_services = self.pipeline_services.read().clone();
        if let Some(services) = pipeline_services {
            services.ports.runtime_started(runtime_epoch).await;
        }
        #[cfg(test)]
        self.wait_at_activation_barrier(listener_id, &caller_cancellation)
            .await?;
        let mut running = tokio::select! {
            biased;
            () = caller_cancellation.cancelled() => {
                self.release_start(listener_id).await;
                return Err(start_cancelled(listener_id));
            }
            running = self.running.lock() => running,
        };
        if caller_cancellation.is_cancelled() {
            drop(running);
            self.release_start(listener_id).await;
            return Err(start_cancelled(listener_id));
        }
        if running.contains_key(&listener_id) {
            drop(running);
            self.release_start(listener_id).await;
            return Err(
                AppError::new("LISTENER_ALREADY_RUNNING", "Listener 已在运行。")
                    .entity(listener_id.to_string()),
            );
        }
        self.pending_starts.write().remove(&listener_id);
        let run_token = uuid::Uuid::new_v4();
        if let Some(resolver) = self.body_codec_resolver.read().clone() {
            resolver.install_listener(runtime_epoch, run_token, &listener);
        }
        let task = tokio::spawn(async move {
            let result = serve_prepared_listener(
                plan,
                tcp_listener,
                listener_id,
                workspace_id,
                runtime_epoch,
                task_cancellation,
            )
            .await;
            if let Err(error) = result
                && !is_orderly_stop(error.code)
            {
                *task_fault.write() = Some(error.message);
            }
        });
        running.insert(
            listener_id,
            RunningListener {
                run_token,
                runtime_epoch,
                cancellation,
                task,
                listen_address: listen_address.clone(),
                fault,
                workspace,
                socket_service,
                scripted_snapshot,
                external_socket_snapshot,
                http_protocol_snapshot,
            },
        );
        self.environment_apply_resource_gates
            .publish_listener_projection(
                listener_id.as_uuid(),
                Some(format!(
                    "{:?}:{:?}:0",
                    Some(runtime_epoch),
                    intercept_proxy_application::ListenerRuntimeState::Running
                )),
            );
        Ok(running_status(listener_id, listen_address))
    }

    #[cfg(test)]
    async fn wait_at_activation_barrier(
        &self,
        listener_id: ListenerId,
        caller_cancellation: &CancellationToken,
    ) -> AppResult<()> {
        let Some(barrier) = self.activation_barriers.lock().await.remove(&listener_id) else {
            return Ok(());
        };
        let _completion = StartBarrierCompletion(Arc::clone(&barrier.completed));
        barrier.reached.notify_one();
        tokio::select! {
            biased;
            () = caller_cancellation.cancelled() => {
                self.release_start(listener_id).await;
                Err(start_cancelled(listener_id))
            }
            () = barrier.release.notified() => Ok(())
        }
    }
}

async fn serve_prepared_listener(
    plan: PreparedListenerRuntime,
    tcp_listener: tokio::net::TcpListener,
    listener_id: ListenerId,
    workspace_id: String,
    runtime_epoch: uuid::Uuid,
    cancellation: CancellationToken,
) -> Result<(), intercept_proxy_runtime::ProxyError> {
    match plan {
        PreparedListenerRuntime::HttpForward { service, .. } => {
            service.serve_listener(tcp_listener, cancellation).await
        }
        PreparedListenerRuntime::HttpFixed { service, .. } => {
            service
                .serve_listener_with_epoch(tcp_listener, runtime_epoch, cancellation)
                .await
        }
        PreparedListenerRuntime::Socket { service, .. }
        | PreparedListenerRuntime::ExternalScriptedSocket { service, .. }
        | PreparedListenerRuntime::ScriptedSocket {
            service: Some(service),
            ..
        } => {
            service
                .serve_listener_with_context(
                    tcp_listener,
                    intercept_proxy_runtime::SocketRelayRunContext {
                        workspace_id,
                        listener_id: listener_id.to_string(),
                        workspace_runtime_epoch: runtime_epoch,
                        listener_run_epoch: uuid::Uuid::new_v4(),
                    },
                    cancellation,
                )
                .await
        }
        PreparedListenerRuntime::ScriptedSocket { service: None, .. } => {
            Err(intercept_proxy_runtime::ProxyError::new(
                intercept_proxy_runtime::ErrorCode::Internal,
                "scripted socket plan reached serve without a runtime service",
            ))
        }
    }
}

#[cfg(test)]
struct StartBarrierCompletion(Arc<tokio::sync::Notify>);

#[cfg(test)]
impl Drop for StartBarrierCompletion {
    fn drop(&mut self) {
        self.0.notify_one();
    }
}

fn start_cancelled(listener_id: ListenerId) -> AppError {
    AppError::new(
        "LISTENER_START_CANCELLED",
        "Listener 启动调用已取消，启动预留已释放。",
    )
    .entity(listener_id.to_string())
}

fn is_orderly_stop(code: &'static str) -> bool {
    matches!(
        code,
        "PROXY_STOPPED" | "BREAKPOINT_PROXY_STOPPED" | "SOCKET_RELAY_CANCELLED"
    )
}
