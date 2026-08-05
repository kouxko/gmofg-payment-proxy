use super::{
    Arc, AssertUnwindSafe, BTreeMap, BoundChannel, CancelOnDrop, CancellationToken, ErrorCode,
    FutureExt, PendingCleanup, PreparedChannel, ProxyConfig, ProxyError, ProxyState, Result,
    Runtime, RuntimeSnapshot, StartedTasks, StoppingNotification, SupervisorCore, Uuid, mpsc,
    notify_runtime_stopping, panic_message, shutdown_runtime, snapshot, spawn_listener_task,
    spawn_watchdog,
};

impl SupervisorCore {
    pub(super) async fn run_start(self: Arc<Self>, config: ProxyConfig) -> Result<RuntimeSnapshot> {
        self.run_guarded(self.start_inner(config)).await
    }

    pub(super) async fn run_stop(self: Arc<Self>) -> Result<RuntimeSnapshot> {
        self.run_guarded(self.stop_inner()).await
    }

    pub(super) async fn run_restart(
        self: Arc<Self>,
        config: ProxyConfig,
    ) -> Result<RuntimeSnapshot> {
        self.run_guarded(async {
            self.stop_inner().await?;
            self.start_inner(config).await
        })
        .await
    }

    async fn run_guarded<F>(&self, operation: F) -> Result<RuntimeSnapshot>
    where
        F: std::future::Future<Output = Result<RuntimeSnapshot>>,
    {
        match AssertUnwindSafe(operation).catch_unwind().await {
            Ok(result) => result,
            Err(payload) => {
                let message = format!(
                    "proxy lifecycle operation panicked: {}",
                    panic_message(payload.as_ref())
                );
                let _ = self.cleanup_runtime().await;
                let mut lifecycle = self.lifecycle.write().await;
                lifecycle.state = ProxyState::Faulted;
                lifecycle.epoch = None;
                lifecycle.listeners.clear();
                lifecycle.fault = Some(message.clone());
                Err(ProxyError::new(ErrorCode::Internal, message))
            }
        }
    }

    async fn start_inner(&self, config: ProxyConfig) -> Result<RuntimeSnapshot> {
        // operation 锁覆盖整个生命周期事务，确保 start/stop/restart 不会交错操作同一批
        // socket 与任务。锁内的阶段顺序也是外部可观察状态机的唯一发布顺序。
        let _operation = self.operation.lock().await;
        config.validate()?;
        match self.lifecycle.read().await.state {
            ProxyState::Running => {
                return Err(ProxyError::new(
                    ErrorCode::ProxyAlreadyRunning,
                    "proxy is already running",
                ));
            }
            ProxyState::Starting
            | ProxyState::Stopping
            | ProxyState::Stopped
            | ProxyState::Faulted => {}
        }
        if let Err(error) = self.cleanup_runtime().await {
            self.mark_start_fault(&error).await;
            return Err(error);
        }
        {
            let mut lifecycle = self.lifecycle.write().await;
            lifecycle.state = ProxyState::Starting;
            lifecycle.epoch = None;
            lifecycle.listeners.clear();
            lifecycle.fault = None;
        }

        let prepared = match self.prepare_start(&config).await {
            Ok(prepared) => prepared,
            Err(error) => {
                self.mark_start_fault(&error).await;
                return Err(error);
            }
        };

        let epoch = Uuid::new_v4();
        let cancellation = CancellationToken::new();
        // 直到 Runtime 成功放入 self.runtime 前，guard 都拥有“异常路径必须取消”的责任；
        // 调用 future 被 abort 或发生 panic 时，也不会遗留孤儿 listener。
        let mut start_guard = CancelOnDrop::new(cancellation.clone());
        *self
            .active_cancellation
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner) = Some(cancellation.clone());
        let started = self.start_tasks(prepared, epoch, cancellation.clone());

        {
            let mut runtime = self.runtime.lock().await;
            *runtime = Some(Runtime {
                epoch,
                cancellation,
                listener_tasks: started.listener_tasks,
                watchdog: started.watchdog,
                ports: started.ports,
                stopping_notified: started.stopping_notified,
            });
        }
        start_guard.disarm();
        {
            let mut lifecycle = self.lifecycle.write().await;
            lifecycle.state = ProxyState::Running;
            lifecycle.epoch = Some(epoch);
            lifecycle.listeners = started.listener_addresses;
        }
        let _ = started.listeners_ready.send(true);
        Ok(self.snapshot_inner().await)
    }

    async fn mark_start_fault(&self, error: &ProxyError) {
        let mut lifecycle = self.lifecycle.write().await;
        lifecycle.state = ProxyState::Faulted;
        lifecycle.epoch = None;
        lifecycle.listeners.clear();
        lifecycle.fault = Some(error.to_string());
    }

    async fn prepare_start(&self, config: &ProxyConfig) -> Result<Vec<PreparedChannel>> {
        // Dropping this vector rolls back earlier binds if a later step fails.
        let bound = self.bind_enabled_channels(config).await?;
        self.prepare_channel_services(config, bound).await
    }

    async fn bind_enabled_channels(&self, config: &ProxyConfig) -> Result<Vec<BoundChannel>> {
        let mut bound = Vec::new();
        for channel in config.channels.iter().filter(|channel| channel.enabled) {
            let listener = self
                .binder
                .bind(channel.listen_addr)
                .await
                .map_err(|error| {
                    let code = if error.kind() == std::io::ErrorKind::AddrInUse {
                        ErrorCode::PortInUse
                    } else {
                        ErrorCode::Io
                    };
                    ProxyError::new(
                        code,
                        format!("failed to bind {}: {error}", channel.listen_addr),
                    )
                })?;
            let local_addr = listener
                .local_addr()
                .map_err(|error| ProxyError::io("read listener address", &error))?;
            bound.push(BoundChannel {
                channel: channel.channel.clone(),
                listener,
                local_addr,
            });
        }
        Ok(bound)
    }

    async fn prepare_channel_services(
        &self,
        config: &ProxyConfig,
        bound: Vec<BoundChannel>,
    ) -> Result<Vec<PreparedChannel>> {
        // Build every service before publishing the epoch so certificate or
        // upstream failures also release every bound socket.
        let mut services = self.service_factory.build(config).await?;
        bound
            .into_iter()
            .map(|bound| {
                let service = services.remove(&bound.channel).ok_or_else(|| {
                    ProxyError::new(
                        ErrorCode::ConfigInvalid,
                        format!("runtime factory omitted {} service", bound.channel),
                    )
                })?;
                Ok(PreparedChannel {
                    channel: bound.channel,
                    listener: bound.listener,
                    local_addr: bound.local_addr,
                    service,
                })
            })
            .collect()
    }

    fn start_tasks(
        &self,
        prepared: Vec<PreparedChannel>,
        epoch: Uuid,
        cancellation: CancellationToken,
    ) -> StartedTasks {
        let (fatal_tx, fatal_rx) = mpsc::channel(prepared.len());
        let (listeners_ready, listeners_ready_rx) = tokio::sync::watch::channel(false);
        let ports = Arc::clone(
            &prepared
                .first()
                .expect("validated enabled channel")
                .service
                .ports,
        );
        let stopping_notified = Arc::new(StoppingNotification::default());
        let mut listener_tasks = Vec::with_capacity(prepared.len());
        let mut listener_addresses = BTreeMap::new();
        for channel in prepared {
            listener_addresses.insert(channel.channel.clone(), channel.local_addr);
            listener_tasks.push(spawn_listener_task(
                channel,
                epoch,
                cancellation.child_token(),
                fatal_tx.clone(),
                listeners_ready_rx.clone(),
            ));
        }
        drop(fatal_tx);
        let watchdog = spawn_watchdog(
            Arc::clone(&self.lifecycle),
            epoch,
            cancellation,
            Arc::clone(&ports),
            Arc::clone(&stopping_notified),
            fatal_rx,
        );
        StartedTasks {
            listener_tasks,
            watchdog,
            listener_addresses,
            ports,
            stopping_notified,
            listeners_ready,
        }
    }

    async fn stop_inner(&self) -> Result<RuntimeSnapshot> {
        let _operation = self.operation.lock().await;
        match self.lifecycle.read().await.state {
            ProxyState::Stopped => {
                return Ok(self.snapshot_inner().await);
            }
            ProxyState::Starting
            | ProxyState::Stopping
            | ProxyState::Running
            | ProxyState::Faulted => {}
        }
        self.lifecycle.write().await.state = ProxyState::Stopping;
        if let Err(error) = self.cleanup_runtime().await {
            let mut lifecycle = self.lifecycle.write().await;
            lifecycle.state = ProxyState::Faulted;
            lifecycle.epoch = None;
            lifecycle.listeners.clear();
            lifecycle.fault = Some(error.to_string());
            return Err(error);
        }
        {
            let mut lifecycle = self.lifecycle.write().await;
            lifecycle.state = ProxyState::Stopped;
            lifecycle.epoch = None;
            lifecycle.listeners.clear();
            lifecycle.fault = None;
        }
        Ok(self.snapshot_inner().await)
    }

    async fn snapshot_inner(&self) -> RuntimeSnapshot {
        let lifecycle = self.lifecycle.read().await;
        snapshot(&lifecycle)
    }

    /// 收回当前 Runtime，并补交“即将停止”通知。
    ///
    /// `pending_cleanup` 表示上一次清理已经取得 Runtime 所有权、但停止通知尚未可靠送达。
    /// 本次调用必须先 `take` 走该所有权并重试；失败时放回，确保同一个 epoch 不会被两个
    /// 清理者并发处理。当前 Runtime 也先从共享槽位取出再 await shutdown，使后续生命周期
    /// 操作不可能再次取得同一批任务。只有通知未完成且 shutdown 失败时才登记新的 pending，
    /// 最后无条件取消兜底 token，保证异常路径不会遗留 listener 任务。
    async fn cleanup_runtime(&self) -> Result<()> {
        let pending = self.pending_cleanup.lock().await.take();
        if let Some(pending) = pending
            && let Err(error) =
                notify_runtime_stopping(&pending.ports, &pending.stopping_notified, pending.epoch)
                    .await
        {
            *self.pending_cleanup.lock().await = Some(pending);
            return Err(error);
        }

        let runtime = self.runtime.lock().await.take();
        let shutdown_result = if let Some(runtime) = runtime {
            let pending = PendingCleanup {
                epoch: runtime.epoch,
                ports: Arc::clone(&runtime.ports),
                stopping_notified: Arc::clone(&runtime.stopping_notified),
            };
            let result = shutdown_runtime(runtime).await;
            if result.is_err() && !pending.stopping_notified.is_complete() {
                *self.pending_cleanup.lock().await = Some(pending);
            }
            result
        } else {
            Ok(())
        };
        if let Some(cancellation) = self
            .active_cancellation
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .take()
        {
            cancellation.cancel();
        }
        shutdown_result
    }
}
