use super::{
    Arc, AssertUnwindSafe, CancelOnDrop, CancellationToken, ChannelId, ErrorCode, FutureExt,
    JoinHandle, Lifecycle, Ordering, PipelinePorts, PreparedChannel, ProxyError, ProxyState,
    ProxySupervisor, Result, Runtime, RuntimeSnapshot, RwLock, SHUTDOWN_GRACE_PERIOD,
    StoppingNotification, SupervisorCore, Uuid, mpsc,
};

impl Drop for ProxySupervisor {
    fn drop(&mut self) {
        if let Some(cancellation) = self
            .core
            .active_cancellation
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .take()
        {
            cancellation.cancel();
        }
    }
}

impl Drop for SupervisorCore {
    fn drop(&mut self) {
        if let Some(cancellation) = self
            .active_cancellation
            .get_mut()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .take()
        {
            cancellation.cancel();
        }
    }
}

pub(super) fn spawn_listener_task(
    prepared: PreparedChannel,
    epoch: Uuid,
    cancellation: CancellationToken,
    fatal_tx: mpsc::Sender<(ChannelId, ProxyError)>,
    mut listeners_ready: tokio::sync::watch::Receiver<bool>,
) -> JoinHandle<()> {
    tokio::spawn(async move {
        // listener 已经创建，但必须等 supervisor 发布 Running 后再 accept；这道门避免客户端
        // 在 lifecycle/listener 地址尚不可见时抢先进入 pipeline。
        if !*listeners_ready.borrow() && listeners_ready.changed().await.is_err() {
            return;
        }
        if cancellation.is_cancelled() {
            return;
        }
        let outcome = AssertUnwindSafe(prepared.service.run_listener(
            prepared.listener,
            prepared.channel.clone(),
            epoch,
            cancellation.clone(),
        ))
        .catch_unwind()
        .await;
        if cancellation.is_cancelled() {
            return;
        }
        let error = match outcome {
            Ok(Err(error)) => error,
            Ok(Ok(())) => ProxyError::new(
                ErrorCode::Internal,
                format!("{} listener exited unexpectedly", prepared.channel),
            ),
            Err(payload) => ProxyError::new(
                ErrorCode::Internal,
                format!(
                    "{} listener panicked: {}",
                    prepared.channel,
                    panic_message(payload.as_ref())
                ),
            ),
        };
        if !cancellation.is_cancelled() {
            let _ = fatal_tx.send((prepared.channel, error)).await;
        }
    })
}

pub(super) fn spawn_watchdog(
    lifecycle: Arc<RwLock<Lifecycle>>,
    epoch: Uuid,
    cancellation: CancellationToken,
    ports: Arc<dyn PipelinePorts>,
    stopping_notified: Arc<StoppingNotification>,
    mut fatal_rx: mpsc::Receiver<(ChannelId, ProxyError)>,
) -> JoinHandle<()> {
    tokio::spawn(async move {
        // watchdog 与正常取消竞速：取消胜出时安静结束；首个 listener 故障胜出时先通知
        // stopping，再取消兄弟任务、标记同一 epoch Faulted，最后发布 fault 事件。
        tokio::select! {
            () = cancellation.cancelled() => {}
            fault = fatal_rx.recv() => {
                if let Some((channel, error)) = fault {
                    if let Err(stopping_error) = notify_runtime_stopping(
                        &ports,
                        &stopping_notified,
                        epoch,
                    ).await {
                        tracing::error!(
                            runtime_epoch = %epoch,
                            error = %stopping_error,
                            "runtime fault cleanup callback failed"
                        );
                    }
                    cancellation.cancel();
                    let mut lifecycle = lifecycle.write().await;
                    if lifecycle.epoch == Some(epoch) {
                        lifecycle.state = ProxyState::Faulted;
                        lifecycle.fault = Some(error.to_string());
                    }
                    drop(lifecycle);
                    notify_runtime_fault(&ports, epoch, channel, &error).await;
                }
            }
        }
    })
}

pub(super) async fn shutdown_runtime(mut runtime: Runtime) -> Result<()> {
    let mut cancellation_guard = CancelOnDrop::new(runtime.cancellation.clone());
    tracing::debug!(runtime_epoch = %runtime.epoch, "stopping proxy runtime");
    let stopping_result =
        notify_runtime_stopping(&runtime.ports, &runtime.stopping_notified, runtime.epoch).await;
    runtime.cancellation.cancel();
    cancellation_guard.disarm();

    let joined = tokio::time::timeout(SHUTDOWN_GRACE_PERIOD, async {
        for task in &mut runtime.listener_tasks {
            let _ = task.await;
        }
        let _ = (&mut runtime.watchdog).await;
    })
    .await;
    let join_result = if joined.is_err() {
        tracing::warn!(
            runtime_epoch = %runtime.epoch,
            "proxy runtime exceeded shutdown grace period; aborting remaining tasks"
        );
        for task in &runtime.listener_tasks {
            task.abort();
        }
        runtime.watchdog.abort();
        for task in runtime.listener_tasks {
            let _ = task.await;
        }
        let _ = runtime.watchdog.await;
        Err(ProxyError::new(
            ErrorCode::Internal,
            "proxy runtime exceeded shutdown grace period",
        ))
    } else {
        Ok(())
    };
    stopping_result.and(join_result)
}

pub(super) async fn notify_runtime_stopping(
    ports: &Arc<dyn PipelinePorts>,
    stopping_notified: &StoppingNotification,
    epoch: Uuid,
) -> Result<()> {
    if stopping_notified.is_complete() {
        return Ok(());
    }
    let _operation = stopping_notified.operation.lock().await;
    if stopping_notified.is_complete() {
        return Ok(());
    }
    let outcome = tokio::time::timeout(
        SHUTDOWN_GRACE_PERIOD,
        AssertUnwindSafe(ports.runtime_stopping(epoch)).catch_unwind(),
    )
    .await;
    let result = match outcome {
        Ok(Ok(())) => Ok(()),
        Ok(Err(payload)) => Err(ProxyError::new(
            ErrorCode::Internal,
            format!(
                "runtime_stopping callback panicked: {}",
                panic_message(payload.as_ref())
            ),
        )),
        Err(_) => Err(ProxyError::new(
            ErrorCode::Internal,
            "runtime_stopping callback exceeded shutdown grace period",
        )),
    };
    if result.is_ok() {
        stopping_notified.completed.store(true, Ordering::Release);
    }
    result
}

async fn notify_runtime_fault(
    ports: &Arc<dyn PipelinePorts>,
    epoch: Uuid,
    channel: ChannelId,
    error: &ProxyError,
) {
    let outcome = tokio::time::timeout(
        SHUTDOWN_GRACE_PERIOD,
        AssertUnwindSafe(ports.runtime_fault(epoch, channel, error)).catch_unwind(),
    )
    .await;
    match outcome {
        Ok(Ok(())) => {}
        Ok(Err(payload)) => tracing::error!(
            runtime_epoch = %epoch,
            panic = %panic_message(payload.as_ref()),
            "runtime_fault callback panicked"
        ),
        Err(_) => tracing::warn!(
            runtime_epoch = %epoch,
            "runtime_fault callback exceeded shutdown grace period"
        ),
    }
}

pub(super) fn operation_join_error(error: &tokio::task::JoinError) -> ProxyError {
    ProxyError::new(
        ErrorCode::Internal,
        format!("proxy lifecycle task failed: {error}"),
    )
}

pub(super) fn panic_message(payload: &(dyn std::any::Any + Send)) -> &str {
    payload
        .downcast_ref::<&'static str>()
        .copied()
        .or_else(|| payload.downcast_ref::<String>().map(String::as_str))
        .unwrap_or("non-string panic payload")
}

pub(super) fn snapshot(lifecycle: &Lifecycle) -> RuntimeSnapshot {
    RuntimeSnapshot {
        state: lifecycle.state,
        runtime_epoch: lifecycle.epoch,
        listeners: lifecycle.listeners.clone(),
        fault: lifecycle.fault.clone(),
    }
}
