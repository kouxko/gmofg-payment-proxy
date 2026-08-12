use super::{
    Arc, BoxIo, Bytes, CancellationToken, ConnectionContext, ConnectionService, ErrorCode,
    FaultAction, ForwardRequest, Incoming, InformationalResponseSink, IntentionalWireFault,
    Message, ProxyError, RawHttp1HeadCapture, ReadRecordingIo, Request, RequestWireState, Response,
    ResponseDisposition, ResponseHeadPreservingIo, ResponseWriteTracker, Result, SplitIo, StdMutex,
    TokioIo, TrafficSchedule, WireBody, collect_limited, fault, finish_downstream_write, io,
    raw_head_capture_limit, response_from_disposition, server_http1, service_fn, timeout_stage,
    validate_headers,
};

impl ConnectionService {
    pub(super) async fn run_connection_inner(
        &self,
        io: BoxIo,
        context: &ConnectionContext,
        cancellation: CancellationToken,
    ) -> Result<()> {
        let service = self.clone();
        let context = context.clone();
        let request_cancel = cancellation.clone();
        let raw_tail = Arc::new(StdMutex::new(None::<Bytes>));
        let handler_tail = Arc::clone(&raw_tail);
        let raw_request_head = Arc::new(StdMutex::new(RawHttp1HeadCapture::default()));
        let handler_request_head = Arc::clone(&raw_request_head);
        let canonical_response_head = Arc::new(StdMutex::new(None::<Bytes>));
        let handler_response_head = Arc::clone(&canonical_response_head);
        let (reader, writer) = tokio::io::split(io);
        let shared_writer = Arc::new(StdMutex::new(writer));
        let split_io: BoxIo = Box::new(SplitIo {
            reader,
            writer: Arc::clone(&shared_writer),
        });
        let informational_response_sink =
            InformationalResponseSink::new(shared_writer, self.write_timeout);
        let handler_informational_sink = informational_response_sink.clone();
        let intentional_wire_fault = Arc::new(StdMutex::new(None::<IntentionalWireFault>));
        let handler_wire_fault = Arc::clone(&intentional_wire_fault);
        let handler_error = Arc::new(StdMutex::new(None::<ProxyError>));
        let service_error = Arc::clone(&handler_error);
        let response_write = Arc::new(ResponseWriteTracker::default());
        let handler_response_write = Arc::clone(&response_write);
        let handler = service_fn(move |request| {
            let service = service.clone();
            let context = context.clone();
            let cancellation = request_cancel.clone();
            let raw_tail = Arc::clone(&handler_tail);
            let raw_request_head = Arc::clone(&handler_request_head);
            let canonical_response_head = Arc::clone(&handler_response_head);
            let informational_response_sink = handler_informational_sink.clone();
            let intentional_wire_fault = Arc::clone(&handler_wire_fault);
            let service_error = Arc::clone(&service_error);
            let response_write = Arc::clone(&handler_response_write);
            async move {
                let wire = RequestWireState {
                    raw_request_head: &raw_request_head,
                    canonical_response_head: &canonical_response_head,
                    informational_response_sink: &informational_response_sink,
                    raw_tail: &raw_tail,
                    intentional_wire_fault: &intentional_wire_fault,
                };
                let result = service
                    .handle_request(request, &context, &cancellation, &wire)
                    .await;
                if result.is_ok() {
                    response_write.mark_response_ready();
                }
                result.map_err(|error| {
                    let wire_error = io::Error::other(error.to_string());
                    *service_error.lock().expect("handler error mutex poisoned") = Some(error);
                    wire_error
                })
            }
        });
        let response_preserving_io: BoxIo = Box::new(ResponseHeadPreservingIo::new(
            split_io,
            canonical_response_head,
        ));
        let recording_io = ReadRecordingIo::new(
            response_preserving_io,
            raw_request_head,
            raw_head_capture_limit(self.limits),
        );
        let mut connection = Box::pin(
            server_http1::Builder::new()
                .keep_alive(false)
                .max_headers(self.limits.max_headers)
                .serve_connection(TokioIo::new(recording_io), handler)
                .without_shutdown(),
        );
        let initial = tokio::select! {
            () = cancellation.cancelled() => {
                return Err(ProxyError::new(
                    ErrorCode::ProxyStopped,
                    "proxy stopped while connection was active",
                ));
            }
            result = &mut connection => Some(result),
            () = response_write.wait_until_ready() => None,
        };
        let result = match initial {
            Some(result) => result,
            None => {
                timeout_stage(
                    self.write_timeout,
                    &cancellation,
                    &mut connection,
                    ErrorCode::Io,
                )
                .await?
            }
        };
        let parts = match result {
            Ok(parts) => parts,
            Err(error) => {
                let original = handler_error
                    .lock()
                    .expect("handler error mutex poisoned")
                    .take();
                if let Some(original) = original {
                    return Err(original);
                }
                if let Some(fault) = *intentional_wire_fault
                    .lock()
                    .expect("intentional wire fault mutex poisoned")
                {
                    return Err(fault.error());
                }
                return Err(ProxyError::new(
                    ErrorCode::Io,
                    format!("HTTP/1.1 connection failed: {error}"),
                ));
            }
        };
        let mut io = parts.io.into_inner().into_inner();
        let tail = raw_tail.lock().expect("raw tail mutex poisoned").take();
        finish_downstream_write(
            &mut io,
            tail,
            self.write_timeout,
            &cancellation,
            &intentional_wire_fault,
        )
        .await
    }

    async fn handle_request(
        &self,
        request: Request<Incoming>,
        context: &ConnectionContext,
        cancellation: &CancellationToken,
        wire: &RequestWireState<'_>,
    ) -> Result<Response<WireBody>> {
        let (parts, body) = request.into_parts();
        validate_headers(&parts.headers, self.limits)?;
        let body = timeout_stage(
            self.read_timeout,
            cancellation,
            collect_limited(body, self.limits.max_body_bytes),
            ErrorCode::Io,
        )
        .await??;
        let raw_head = wire
            .raw_request_head
            .lock()
            .expect("raw HTTP request head capture mutex poisoned")
            .required_head("downstream request")?;
        let mut message = Message::from_raw_http1_head(&raw_head, body)?;
        message.validate(self.limits)?;
        let request_actions = self.ports.request(context, &mut message).await?;

        for action in &request_actions {
            match action {
                FaultAction::Delay(duration) => {
                    tokio::select! {
                        () = cancellation.cancelled() => return Err(ProxyError::new(
                            ErrorCode::ProxyStopped,
                            "proxy stopped during request delay",
                        )),
                        () = self.clock.sleep(*duration) => {}
                    }
                }
                FaultAction::DisconnectBeforeUpstream => {
                    return Err(ProxyError::new(
                        ErrorCode::ClientDisconnected,
                        "request intentionally disconnected before upstream",
                    ));
                }
                FaultAction::MockResponse {
                    status,
                    headers,
                    body,
                } => {
                    let message = fault::mock_response(*status, headers, body.clone());
                    return response_from_disposition(
                        ResponseDisposition::Send {
                            message,
                            schedule: TrafficSchedule::default(),
                        },
                        wire.canonical_response_head,
                        wire.raw_tail,
                        wire.intentional_wire_fault,
                        cancellation,
                    );
                }
                FaultAction::RejectTls => {
                    return Err(ProxyError::new(
                        ErrorCode::TlsHandshakeFailed,
                        "TLS intentionally rejected",
                    ));
                }
                _ => {}
            }
        }

        let forward = ForwardRequest {
            method: parts.method,
            uri: parts.uri,
            message,
        };
        let upstream_exchange = self
            .upstream
            .send(
                context,
                self.ports.as_ref(),
                forward,
                &request_actions,
                Some(wire.informational_response_sink),
                cancellation,
            )
            .await?;
        if request_actions.iter().any(|action| {
            matches!(
                action,
                FaultAction::DropResponse {
                    read_upstream: true
                }
            )
        }) {
            return Err(ProxyError::new(
                ErrorCode::ClientDisconnected,
                "upstream response intentionally dropped after complete read",
            ));
        }
        for head in upstream_exchange.informational_heads {
            wire.informational_response_sink
                .publish(head, cancellation)
                .await?;
        }
        let mut upstream_response = upstream_exchange.final_response;
        let response_actions = self.ports.response(context, &mut upstream_response).await?;
        let disposition =
            fault::apply_response_actions(upstream_response, &response_actions, cancellation)
                .await?;
        response_from_disposition(
            disposition,
            wire.canonical_response_head,
            wire.raw_tail,
            wire.intentional_wire_fault,
            cancellation,
        )
    }
}
