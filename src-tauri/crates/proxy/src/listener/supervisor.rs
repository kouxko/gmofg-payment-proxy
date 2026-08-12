use std::{fmt::Debug, net::SocketAddr, sync::Arc, time::Duration};

use futures_util::FutureExt;
use tokio::sync::Semaphore;
use tokio::task::JoinSet;
use tokio_util::sync::CancellationToken;

use crate::{
    ErrorCode, ProxyError, Result,
    supervisor::ChannelId,
    transport::{BoundListener, Clock, ConnectionContext, ListenerBinder},
};

use super::{
    AdmissionDecision, ConnectionHandler, ConnectionLifecycleObserver, ConnectionTaskScope,
    ListenerAdmission, ListenerCapacity, ListenerRejection, ListenerRunContext,
    PrimaryConnectionOutcome, TerminalConnectionOutcome, handler::SharedConnectionObserver,
    outcome::synthesize_terminal,
};

#[derive(Debug, Clone)]
pub(crate) struct ListenerConfig {
    pub(crate) bind_addr: SocketAddr,
    pub(crate) runtime_epoch: uuid::Uuid,
    pub(crate) listener_id: ChannelId,
    pub(crate) allowed_client_cidrs: Vec<String>,
    pub(crate) capacity: ListenerCapacity,
    pub(crate) shutdown_grace: Duration,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum ListenerRunOutcome {
    Stopped {
        local_addr: SocketAddr,
    },
    Faulted {
        local_addr: SocketAddr,
        code: &'static str,
    },
}

impl ListenerRunOutcome {
    pub(crate) fn into_result(self, fault_message: &'static str) -> Result<()> {
        match self {
            Self::Stopped { .. } => Ok(()),
            Self::Faulted { code, .. } => Err(ProxyError {
                code,
                message: fault_message.to_owned(),
            }),
        }
    }
}

#[derive(Debug, Default)]
pub(crate) struct NoopConnectionLifecycleObserver;

impl ConnectionLifecycleObserver for NoopConnectionLifecycleObserver {}

#[derive(Debug)]
pub(crate) struct ListenerSupervisor<H: ConnectionHandler + ?Sized> {
    config: ListenerConfig,
    binder: Arc<dyn ListenerBinder>,
    clock: Arc<dyn Clock>,
    handler: Arc<H>,
    observer: SharedConnectionObserver,
}

impl<H: ConnectionHandler + ?Sized + 'static> ListenerSupervisor<H> {
    pub(crate) fn new(
        config: ListenerConfig,
        binder: Arc<dyn ListenerBinder>,
        clock: Arc<dyn Clock>,
        handler: Arc<H>,
        observer: SharedConnectionObserver,
    ) -> Result<Self> {
        if config.shutdown_grace.is_zero() {
            return Err(ProxyError::new(
                ErrorCode::ConfigInvalid,
                "listener shutdown grace must be greater than zero",
            ));
        }
        ListenerAdmission::new(config.allowed_client_cidrs.clone(), config.capacity.clone())?;
        Ok(Self {
            config,
            binder,
            clock,
            handler,
            observer,
        })
    }

    pub(crate) async fn bind_and_run(
        &self,
        cancellation: CancellationToken,
    ) -> Result<ListenerRunOutcome> {
        let listener = self
            .binder
            .bind(self.config.bind_addr)
            .await
            .map_err(|error| bind_error(self.config.bind_addr, &error))?;
        self.run_bound(listener, cancellation).await
    }

    pub(crate) async fn run_bound(
        &self,
        listener: Arc<dyn BoundListener>,
        cancellation: CancellationToken,
    ) -> Result<ListenerRunOutcome> {
        let local_addr = listener
            .local_addr()
            .map_err(|error| ProxyError::io("read listener address", &error))?;
        let admission = ListenerAdmission::new(
            self.config.allowed_client_cidrs.clone(),
            self.config.capacity.clone(),
        )?;
        let run_context = ListenerRunContext::new(
            self.config.runtime_epoch,
            self.config.listener_id.clone(),
            Arc::clone(&self.clock),
        );
        let mut connections = JoinSet::new();
        let rejection_projections = Arc::new(Semaphore::new(16));
        let mut listener_fault = None;

        loop {
            tokio::select! {
                biased;
                () = cancellation.cancelled() => break,
                completed = connections.join_next(), if !connections.is_empty() => {
                    if let Some(code) = handle_completed(completed) {
                        listener_fault = Some(code);
                        cancellation.cancel();
                        break;
                    }
                }
                accepted = listener.accept() => {
                    let (io, peer_addr) = match accepted {
                        Ok(accepted) => accepted,
                        Err(error) => {
                            listener_fault = Some(ErrorCode::Io.as_str());
                            tracing::warn!(%error, "listener accept failed");
                            cancellation.cancel();
                            break;
                        }
                    };
                    let context = run_context.connection(peer_addr);
                    let permit = match admission.admit(peer_addr.ip()) {
                        AdmissionDecision::Admitted(permit) => permit,
                        AdmissionDecision::NetworkDenied => {
                            self.observer.connection_rejected(
                                peer_addr,
                                ListenerRejection::NetworkDenied,
                            );
                            let Ok(projection_permit) = Arc::clone(&rejection_projections)
                                .try_acquire_owned()
                            else {
                                drop(io);
                                continue;
                            };
                            let handler = Arc::clone(&self.handler);
                            let rejection_cancel = cancellation.child_token();
                            connections.spawn(async move {
                                let _permit = projection_permit;
                                handler
                                    .reject(
                                        io,
                                        context,
                                        ListenerRejection::NetworkDenied,
                                        rejection_cancel,
                                    )
                                    .await;
                                TerminalConnectionOutcome::Success
                            });
                            continue;
                        }
                        AdmissionDecision::CapacityExhausted => {
                            self.observer
                                .connection_rejected(
                                    peer_addr,
                                    ListenerRejection::CapacityExhausted,
                                );
                            drop(io);
                            continue;
                        }
                    };
                    self.observer.connection_admitted(&context);
                    let handler = Arc::clone(&self.handler);
                    let observer = Arc::clone(&self.observer);
                    let child_cancel = cancellation.child_token();
                    let shutdown_grace = self.config.shutdown_grace;
                    connections.spawn(async move {
                        let _permit = permit;
                        run_connection(
                            handler,
                            observer,
                            io,
                            context,
                            child_cancel,
                            shutdown_grace,
                        )
                        .await
                    });
                }
            }
        }

        drop(listener);
        cancellation.cancel();
        if let Some(code) = drain_connections(&mut connections).await {
            listener_fault = Some(code);
        }
        Ok(match listener_fault {
            Some(code) => ListenerRunOutcome::Faulted { local_addr, code },
            None => ListenerRunOutcome::Stopped { local_addr },
        })
    }
}

async fn run_connection<H: ConnectionHandler + ?Sized + 'static>(
    handler: Arc<H>,
    observer: SharedConnectionObserver,
    io: crate::transport::BoxIo,
    context: ConnectionContext,
    cancellation: CancellationToken,
    shutdown_grace: Duration,
) -> TerminalConnectionOutcome {
    let scope = ConnectionTaskScope::new();
    let handler_future = std::panic::AssertUnwindSafe(handler.handle(
        io,
        context.clone(),
        scope.clone(),
        cancellation.clone(),
    ))
    .catch_unwind();
    tokio::pin!(handler_future);
    let child_fatal = scope.fatal_notified();
    tokio::pin!(child_fatal);
    let mut handler_grace_exceeded = false;
    let mut shutdown_deadline = None;
    let (primary, cancellation_observed) = tokio::select! {
        biased;
        () = cancellation.cancelled() => {
            let deadline = tokio::time::Instant::now() + shutdown_grace;
            shutdown_deadline = Some(deadline);
            if let Ok(result) = tokio::time::timeout_at(deadline, &mut handler_future).await {
                (primary_outcome(result, &cancellation), true)
            } else {
                handler_grace_exceeded = true;
                (PrimaryConnectionOutcome::Cancelled, true)
            }
        },
        result = &mut handler_future => (
            primary_outcome(result, &cancellation),
            cancellation.is_cancelled(),
        ),
        () = &mut child_fatal => {
            cancellation.cancel();
            (PrimaryConnectionOutcome::Cancelled, false)
        },
    };
    scope.close();
    let cancelled_while_draining = if cancellation.is_cancelled() {
        shutdown_deadline.get_or_insert_with(|| tokio::time::Instant::now() + shutdown_grace);
        true
    } else {
        tokio::select! {
            biased;
            () = cancellation.cancelled() => {
                shutdown_deadline = Some(tokio::time::Instant::now() + shutdown_grace);
                true
            },
            () = scope.drain() => false,
        }
    };
    let child_abort = if cancelled_while_draining && scope.snapshot().live_count > 0 {
        let deadline =
            shutdown_deadline.unwrap_or_else(|| tokio::time::Instant::now() + shutdown_grace);
        let timed_out = tokio::time::timeout_at(deadline, scope.drain())
            .await
            .is_err();
        if timed_out {
            scope.abort_live();
            scope.drain().await;
        }
        timed_out
    } else {
        false
    };
    let outcome = synthesize_terminal(
        primary,
        scope.snapshot().aggregate,
        handler_grace_exceeded || child_abort,
        cancellation_observed || cancelled_while_draining,
    );
    observer.connection_terminal(&context, &outcome);
    outcome
}

fn primary_outcome(
    result: std::result::Result<PrimaryConnectionOutcome, Box<dyn std::any::Any + Send>>,
    cancellation: &CancellationToken,
) -> PrimaryConnectionOutcome {
    result.unwrap_or_else(|_| {
        cancellation.cancel();
        PrimaryConnectionOutcome::Panicked
    })
}

fn handle_completed(
    completed: Option<std::result::Result<TerminalConnectionOutcome, tokio::task::JoinError>>,
) -> Option<&'static str> {
    match completed {
        Some(Ok(outcome)) if outcome.is_listener_fault() => Some(match outcome {
            TerminalConnectionOutcome::ChildTaskPanicked => super::CONNECTION_CHILD_TASK_PANICKED,
            TerminalConnectionOutcome::ShutdownGraceExceeded => {
                super::LISTENER_SHUTDOWN_GRACE_EXCEEDED
            }
            _ => unreachable!("listener fault outcome was checked"),
        }),
        Some(Err(error)) if !error.is_cancelled() => Some(ErrorCode::Internal.as_str()),
        _ => None,
    }
}

async fn drain_connections(
    connections: &mut JoinSet<TerminalConnectionOutcome>,
) -> Option<&'static str> {
    let mut fault = None;
    while let Some(completed) = connections.join_next().await {
        fault = fault.or_else(|| handle_completed(Some(completed)));
    }
    fault
}

fn bind_error(address: SocketAddr, error: &std::io::Error) -> ProxyError {
    let code = if error.kind() == std::io::ErrorKind::AddrInUse {
        ErrorCode::PortInUse
    } else {
        ErrorCode::Io
    };
    ProxyError::new(code, format!("cannot bind listener {address}: {error}"))
}

#[cfg(test)]
mod tests;
