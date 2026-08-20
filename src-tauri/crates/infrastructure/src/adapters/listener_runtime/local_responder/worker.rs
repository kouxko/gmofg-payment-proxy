use chrono::Utc;
use intercept_proxy_application::SocketExchangeId;
use intercept_proxy_protocol_scripting::ProtocolRuntimeError;

use super::{
    BlockingCommandSlots, Bytes, DateTime, LocalCommand, LocalWorkerState, SocketProcessingFailure,
    acquire_for_reply, capture, frame_boundary, framing_failure, mpsc, preview, processing_failure,
    request_runtime_failure, response_runtime_failure,
};

pub(super) async fn run_local_worker(
    mut commands: mpsc::Receiver<LocalCommand>,
    mut state: LocalWorkerState,
    blocking_slots: BlockingCommandSlots,
) {
    while let Some(mut command) = commands.recv().await {
        if let LocalCommand::SetDiagnostics(diagnostics) = command {
            state.diagnostics = Some(diagnostics);
            continue;
        }
        let permits = match &mut command {
            LocalCommand::Inspect { reply, .. } => acquire_for_reply(&blocking_slots, reply).await,
            LocalCommand::Process { reply, .. } => acquire_for_reply(&blocking_slots, reply).await,
            // Display 已在线路提交之后。资源繁忙时丢弃展示并清理 pending response，不能
            // 留下等待 blocking permit 的孤儿任务，也不能阻塞下一个 request。
            LocalCommand::CommitDisplay { .. } | LocalCommand::FailOutput { .. } => {
                blocking_slots.try_acquire()
            }
            LocalCommand::SetDiagnostics(_) => unreachable!("handled before permit acquisition"),
        };
        let Some(permits) = permits else {
            handle_unavailable_display(&mut state, command);
            continue;
        };
        let result = tokio::task::spawn_blocking(move || {
            let _permits = permits;
            run_local_command(state, command)
        })
        .await;
        match result {
            Ok(next) => state = next,
            // panic 只关闭当前 exchange worker；等待的 oneshot 自动关闭并由 processor
            // 映射为稳定 ProcessorPanicked，panic payload 不进入诊断。
            Err(_) => return,
        }
    }
}

fn handle_unavailable_display(state: &mut LocalWorkerState, command: LocalCommand) {
    match command {
        LocalCommand::CommitDisplay {
            completed_at,
            ticket,
        } => {
            if let Some(pending) = state.pending_response.take() {
                capture::commit(
                    &mut state.coordinator,
                    pending,
                    capture::LocalCaptureCommit {
                        ticket,
                        capture: &state.capture,
                        connection: &state.connection,
                        completed_at,
                        package: state.package.clone(),
                        request_schema: state.request_schema.clone(),
                        response_schema: state.response_schema.clone(),
                        render_display: false,
                    },
                );
            }
        }
        LocalCommand::FailOutput {
            completed_at,
            ticket,
            failure_kind,
            written_bytes,
        } => {
            if let Some(pending) = state.pending_response.take() {
                capture::fail_output(
                    &mut state.coordinator,
                    pending,
                    capture::LocalCaptureFailure {
                        ticket,
                        capture: &state.capture,
                        connection: &state.connection,
                        completed_at,
                        package: state.package.clone(),
                        request_schema: state.request_schema.clone(),
                        response_schema: state.response_schema.clone(),
                        failure_kind,
                        written_bytes,
                        render_display: false,
                    },
                );
            }
        }
        _ => {}
    }
}

fn run_local_command(mut state: LocalWorkerState, command: LocalCommand) -> LocalWorkerState {
    match command {
        LocalCommand::SetDiagnostics(_) => unreachable!("handled by async worker"),
        LocalCommand::Inspect { buffered, reply } => {
            let result = state
                .inspector
                .inspect(&buffered)
                .map(frame_boundary)
                .map_err(|error| framing_failure(&error));
            let _ = reply.send(result);
        }
        LocalCommand::Process {
            origin,
            occurred_at,
            reply,
        } => {
            let result = process_exchange(&mut state, &origin, occurred_at);
            let _ = reply.send(result);
        }
        LocalCommand::CommitDisplay {
            completed_at,
            ticket,
        } => {
            if let Some(pending) = state.pending_response.take() {
                // response_committed 验证 handle 归属；任一错误或 panic 都只降级展示，不能
                // 反写已提交 response 或毒化下一次 request。
                capture::commit(
                    &mut state.coordinator,
                    pending,
                    capture::LocalCaptureCommit {
                        ticket,
                        capture: &state.capture,
                        connection: &state.connection,
                        completed_at,
                        package: state.package.clone(),
                        request_schema: state.request_schema.clone(),
                        response_schema: state.response_schema.clone(),
                        render_display: true,
                    },
                );
            }
        }
        LocalCommand::FailOutput {
            completed_at,
            ticket,
            failure_kind,
            written_bytes,
        } => {
            if let Some(pending) = state.pending_response.take() {
                capture::fail_output(
                    &mut state.coordinator,
                    pending,
                    capture::LocalCaptureFailure {
                        ticket,
                        capture: &state.capture,
                        connection: &state.connection,
                        completed_at,
                        package: state.package.clone(),
                        request_schema: state.request_schema.clone(),
                        response_schema: state.response_schema.clone(),
                        failure_kind,
                        written_bytes,
                        render_display: true,
                    },
                );
            }
        }
    }
    state
}

fn process_exchange(
    state: &mut LocalWorkerState,
    origin: &Bytes,
    occurred_at: DateTime<Utc>,
) -> Result<Bytes, SocketProcessingFailure> {
    ensure_no_pending_response(state)?;
    let exchange_id = SocketExchangeId::new();
    let package = state.package.clone();
    let cancellation = state.cancellation.clone();
    let mut matched_request_rule_ids = Vec::new();
    let request = state
        .coordinator
        .decode_request_with_document_transform(origin.to_vec(), |document| {
            state
                .request_rules
                .execute_with_cancellation(state.request_rules.bind_document(document), || {
                    cancellation.is_cancelled()
                })
                .map(|execution| {
                    let (document, matched_ids) = execution.into_parts();
                    matched_request_rule_ids = matched_ids;
                    document
                })
                .map_err(|_| {
                    if cancellation.is_cancelled() {
                        ProtocolRuntimeError::LocalResponseCancelled {
                            package: package.clone(),
                        }
                    } else {
                        ProtocolRuntimeError::DocumentTransformFailed {
                            package: package.clone(),
                        }
                    }
                })
        })
        .map_err(|error| request_runtime_failure(&error))?;
    preview::publish_request_parsed(state.diagnostics.as_ref(), exchange_id, &request);
    let mut matched_response_rule_ids = Vec::new();
    let mut response_document = None;
    let response = state.coordinator.build_response(&request, |document| {
        state
            .response_rules
            .execute_with_cancellation(state.response_rules.bind_document(document), || {
                cancellation.is_cancelled()
            })
            .map(|execution| {
                let (document, matched_ids) = execution.into_parts();
                matched_response_rule_ids = matched_ids;
                response_document = Some(
                    intercept_proxy_application::SocketCaptureDocument::from_document(&document),
                );
                document
            })
            .map_err(|_| {
                if cancellation.is_cancelled() {
                    ProtocolRuntimeError::LocalResponseCancelled {
                        package: package.clone(),
                    }
                } else {
                    ProtocolRuntimeError::DocumentTransformFailed {
                        package: package.clone(),
                    }
                }
            })
    });
    let response = match response {
        Ok(response) => response,
        Err(error) => {
            let failure = response_runtime_failure(&error);
            capture::fail_response_build(
                &mut state.coordinator,
                capture::FailedResponseBuild {
                    exchange_id,
                    request,
                    response_document,
                    matched_request_rule_ids,
                    matched_response_rule_ids,
                    occurred_at,
                },
                capture::LocalCaptureFailure {
                    ticket: state.capture.ticket(),
                    capture: &state.capture,
                    connection: &state.connection,
                    completed_at: Utc::now(),
                    package: state.package.clone(),
                    request_schema: state.request_schema.clone(),
                    response_schema: state.response_schema.clone(),
                    failure_kind: failure.kind,
                    written_bytes: 0,
                    render_display: true,
                },
            );
            return Err(failure);
        }
    };
    let written = Bytes::from_owner(response.written_owner());
    state.pending_response = Some(capture::PendingLocalCapture::new(
        response,
        exchange_id,
        request,
        matched_request_rule_ids,
        matched_response_rule_ids,
        occurred_at,
    ));
    Ok(written)
}

fn ensure_no_pending_response(state: &LocalWorkerState) -> Result<(), SocketProcessingFailure> {
    if state.pending_response.is_some() {
        return Err(processing_failure(
            "previous local response was not committed before next request",
        ));
    }
    Ok(())
}
