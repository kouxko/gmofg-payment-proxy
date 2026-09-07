//! Cancellation-safe Listener stop cleanup.

use std::time::Duration;

#[cfg(test)]
use std::sync::Arc;

use intercept_proxy_application::{
    AppError, AppResult, ListenerId, ListenerStatusViewModel, ProxyListener, ProxyWorkspace,
};
use tokio::sync::OwnedMutexGuard;
use tokio_util::sync::CancellationToken;

use super::{ListenerRuntimeAdapter, RunningListener};

struct CallerCancellation {
    token: CancellationToken,
    armed: bool,
}

impl CallerCancellation {
    fn new(token: CancellationToken) -> Self {
        Self { token, armed: true }
    }

    fn disarm(&mut self) {
        self.armed = false;
    }
}

impl Drop for CallerCancellation {
    fn drop(&mut self) {
        if self.armed {
            self.token.cancel();
        }
    }
}

impl ListenerRuntimeAdapter {
    pub(super) async fn finish_start_owned(
        &self,
        workspace: ProxyWorkspace,
        listener: ProxyListener,
        runtime_epoch: uuid::Uuid,
    ) -> AppResult<ListenerStatusViewModel> {
        let listener_id = listener.id;
        let caller_cancellation = CancellationToken::new();
        let mut cancellation_guard = CallerCancellation::new(caller_cancellation.clone());
        let owner = self.clone();
        let result = tokio::spawn(async move {
            owner
                .start_reserved(workspace, listener, runtime_epoch, caller_cancellation)
                .await
        })
        .await;
        cancellation_guard.disarm();
        match result {
            Ok(result) => result,
            Err(error) => {
                self.release_start(listener_id).await;
                Err(AppError::new(
                    "LISTENER_START_TASK_FAILED",
                    format!("Listener owned start 异常终止：{error}"),
                )
                .entity(listener_id.to_string()))
            }
        }
    }

    async fn finish_stopping(
        &self,
        listener_id: ListenerId,
        handle: RunningListener,
    ) -> AppResult<ListenerStatusViewModel> {
        let run_token = handle.run_token;
        handle.cancellation.cancel();
        let mut task = handle.task;
        let stop_error = match tokio::time::timeout(Duration::from_secs(5), &mut task).await {
            Err(_) => {
                task.abort();
                let _ = task.await;
                Some(
                    AppError::new(
                        "LISTENER_STOP_TIMEOUT",
                        "Listener 任务未能在 5 秒内停止，已强制终止。",
                    )
                    .entity(listener_id.to_string()),
                )
            }
            Ok(Err(error)) if !error.is_cancelled() => Some(
                AppError::new(
                    "LISTENER_STOP_FAILED",
                    format!("Listener 任务停止失败：{error}"),
                )
                .entity(listener_id.to_string()),
            ),
            Ok(_) => None,
        };
        #[cfg(test)]
        let barrier = { self.stop_barriers.lock().await.remove(&run_token) };
        #[cfg(test)]
        let completion = barrier
            .as_ref()
            .map(|barrier| Arc::clone(&barrier.completed));
        #[cfg(test)]
        if let Some(barrier) = barrier {
            barrier.reached.notify_one();
            barrier.release.notified().await;
        }
        if let Some(resolver) = self.body_codec_resolver.read().clone() {
            resolver.remove_listener(handle.runtime_epoch, listener_id, handle.run_token);
        }
        if let Some(epoch) = self.release_stopping(run_token).await {
            self.cleanup_runtime_epoch(epoch).await;
        }
        self.environment_apply_resource_gates
            .publish_listener_projection(listener_id.as_uuid(), None);
        #[cfg(test)]
        if let Some(completion) = completion {
            completion.notify_one();
        }
        if let Some(error) = stop_error {
            return Err(error);
        }
        Ok(Self::stopped(listener_id, handle.listen_address))
    }

    pub(super) async fn finish_stopping_owned(
        &self,
        listener_id: ListenerId,
        handle: RunningListener,
        environment_apply_gate: OwnedMutexGuard<()>,
    ) -> AppResult<ListenerStatusViewModel> {
        let owner = self.clone();
        tokio::spawn(async move {
            let result = owner.finish_stopping(listener_id, handle).await;
            drop(environment_apply_gate);
            result
        })
        .await
        .map_err(|error| {
            AppError::new(
                "LISTENER_STOP_TASK_FAILED",
                format!("Listener owned cleanup 异常终止：{error}"),
            )
            .entity(listener_id.to_string())
        })?
    }
}
