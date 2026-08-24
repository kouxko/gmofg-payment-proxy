use super::exchange_runtime::{HttpExchangeConnection, HttpExchangeRequest, HttpExchangeRuntime};
use super::{
    Arc, BoxIo, Bytes, CancellationToken, CanonicalResponseHead, ConnectionContext,
    ConnectionService, ErrorCode, Incoming, InformationalResponseSink, IntentionalWireFault,
    Message, ProxyError, RawHttp1HeadCapture, ReadRecordingIo, Request, RequestWireState, Response,
    ResponseDisposition, ResponseHeadPreservingIo, ResponseWriteTracker, Result, SplitIo, StdMutex,
    TokioIo, WireBody, collect_limited, finish_downstream_write, io, raw_head_capture_limit,
    response_from_disposition, server_http1, service_fn, timeout_stage, validate_headers,
};

#[derive(Clone)]
struct RequestHandlerState {
    service: ConnectionService,
    cancellation: CancellationToken,
    raw_tail: Arc<StdMutex<Option<Bytes>>>,
    raw_request_head: Arc<StdMutex<RawHttp1HeadCapture>>,
    canonical_response_head: Arc<CanonicalResponseHead>,
    informational_response_sink: InformationalResponseSink,
    intentional_wire_fault: Arc<StdMutex<Option<IntentionalWireFault>>>,
    service_error: Arc<StdMutex<Option<ProxyError>>>,
    exchange: Arc<HttpExchangeConnection>,
    response_write: Arc<ResponseWriteTracker>,
}

impl RequestHandlerState {
    async fn handle(
        self,
        request: Request<Incoming>,
    ) -> std::result::Result<Response<WireBody>, io::Error> {
        let wire = RequestWireState {
            raw_request_head: &self.raw_request_head,
            canonical_response_head: &self.canonical_response_head,
            informational_response_sink: &self.informational_response_sink,
            raw_tail: &self.raw_tail,
            intentional_wire_fault: &self.intentional_wire_fault,
        };
        let mut result = self
            .service
            .handle_request(request, &self.cancellation, &wire, &self.exchange)
            .await;
        if let Ok(response) = &mut result {
            response.body_mut().track(self.response_write);
        }
        result.map_err(|error| {
            let wire_error = io::Error::other(error.to_string());
            *self
                .service_error
                .lock()
                .expect("handler error mutex poisoned") = Some(error);
            wire_error
        })
    }
}

impl ConnectionService {
    #[cfg(test)]
    pub(super) async fn run_connection_inner(
        &self,
        io: BoxIo,
        context: &ConnectionContext,
        cancellation: CancellationToken,
    ) -> Result<()> {
        let task_scope = crate::listener::ConnectionTaskScope::new();
        let result = self
            .run_connection_inner_in_scope(io, context, cancellation, &task_scope)
            .await;
        task_scope.close();
        task_scope.drain().await;
        let aggregate = task_scope.snapshot().aggregate;
        if result.is_ok() {
            if aggregate.panic_seen {
                return Err(ProxyError::new(
                    ErrorCode::Internal,
                    "HTTP connection Exchange task panicked",
                ));
            }
            if let Some((_, error)) = aggregate.lowest_error {
                return Err(ProxyError {
                    code: error.code,
                    message: error.message,
                });
            }
        }
        result
    }

    pub(super) async fn run_connection_inner_in_scope(
        &self,
        io: BoxIo,
        context: &ConnectionContext,
        cancellation: CancellationToken,
        task_scope: &crate::listener::ConnectionTaskScope,
    ) -> Result<()> {
        let context = context.clone();
        let raw_tail = Arc::new(StdMutex::new(None::<Bytes>));
        let raw_request_head = Arc::new(StdMutex::new(RawHttp1HeadCapture::default()));
        let canonical_response_head = Arc::new(CanonicalResponseHead::default());
        let (split_io, informational_response_sink) = split_http_io(io, self.write_timeout);
        let intentional_wire_fault = Arc::new(StdMutex::new(None::<IntentionalWireFault>));
        let handler_error = Arc::new(StdMutex::new(None::<ProxyError>));
        let response_write = Arc::new(ResponseWriteTracker::default());
        let exchange = self.start_exchange(
            context,
            cancellation.clone(),
            informational_response_sink.clone(),
            task_scope,
        )?;
        let handler_state = RequestHandlerState {
            service: self.clone(),
            cancellation: cancellation.clone(),
            raw_tail: Arc::clone(&raw_tail),
            raw_request_head: Arc::clone(&raw_request_head),
            canonical_response_head: Arc::clone(&canonical_response_head),
            informational_response_sink: informational_response_sink.clone(),
            intentional_wire_fault: Arc::clone(&intentional_wire_fault),
            service_error: Arc::clone(&handler_error),
            exchange: Arc::clone(&exchange),
            response_write: Arc::clone(&response_write),
        };
        let handler = service_fn(move |request| handler_state.clone().handle(request));
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
                .max_headers(self.limits.max_headers)
                .serve_connection(TokioIo::new(recording_io), handler)
                .without_shutdown(),
        );
        let result = loop {
            tokio::select! {
                () = cancellation.cancelled() => {
                    let error = ProxyError::new(
                        ErrorCode::ProxyStopped,
                        "proxy stopped while connection was active",
                    );
                    exchange.fail(&error).await;
                    return Err(error);
                }
                result = &mut connection => break result,
                () = response_write.wait_for_pending() => {
                    tokio::select! {
                        result = &mut connection => break result,
                        () = response_write.wait_for_clear() => {}
                        () = cancellation.cancelled() => {
                            let error = ProxyError::new(
                                ErrorCode::ProxyStopped,
                                "proxy stopped during downstream response write",
                            );
                            exchange.fail(&error).await;
                            return Err(error);
                        }
                        () = tokio::time::sleep(self.write_timeout) => {
                            let error = ProxyError::new(
                                ErrorCode::Io,
                                format!(
                                    "downstream response write timed out after {} ms",
                                    self.write_timeout.as_millis()
                                ),
                            );
                            exchange.fail(&error).await;
                            return Err(error);
                        }
                    }
                }
            }
        };
        let parts = match result {
            Ok(parts) => parts,
            Err(error) => {
                let error = connection_error(&error, &handler_error, &intentional_wire_fault);
                exchange.fail(&error).await;
                return Err(error);
            }
        };
        finish_http_connection(
            parts.io.into_inner().into_inner(),
            &raw_tail,
            self.write_timeout,
            &cancellation,
            &intentional_wire_fault,
            &exchange,
        )
        .await
    }

    fn start_exchange(
        &self,
        context: ConnectionContext,
        cancellation: CancellationToken,
        informational: InformationalResponseSink,
        task_scope: &crate::listener::ConnectionTaskScope,
    ) -> Result<Arc<HttpExchangeConnection>> {
        HttpExchangeRuntime {
            context,
            ports: Arc::clone(&self.ports),
            upstream: Arc::clone(&self.upstream),
            clock: Arc::clone(&self.clock),
            cancellation,
            informational: Some(informational),
            capabilities: Arc::clone(&self.capabilities),
            endpoint: self.endpoint.clone(),
        }
        .start(task_scope)
        .map(Arc::new)
    }

    async fn handle_request(
        &self,
        request: Request<Incoming>,
        cancellation: &CancellationToken,
        wire: &RequestWireState<'_>,
        exchange: &HttpExchangeConnection,
    ) -> Result<Response<WireBody>> {
        let (parts, body) = request.into_parts();
        if parts.method == http::Method::CONNECT || is_upgrade_request(&parts.headers) {
            return unsupported_exchange_response(wire, cancellation);
        }
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
            .take_required_head("downstream request")?;
        let message = Message::from_raw_http1_head(&raw_head, body)?;
        message.validate(self.limits)?;
        let output = exchange
            .exchange(
                self.endpoint.clone(),
                HttpExchangeRequest {
                    method: parts.method,
                    uri: parts.uri,
                    message,
                },
            )
            .await?;
        for head in output.informational_heads {
            wire.informational_response_sink
                .publish(head, cancellation)
                .await?;
        }
        response_from_disposition(
            output.disposition,
            wire.canonical_response_head,
            wire.raw_tail,
            wire.intentional_wire_fault,
            cancellation,
        )
    }
}

fn split_http_io(
    io: BoxIo,
    write_timeout: std::time::Duration,
) -> (BoxIo, InformationalResponseSink) {
    let (reader, writer) = tokio::io::split(io);
    let shared_writer = Arc::new(StdMutex::new(writer));
    let split_io: BoxIo = Box::new(SplitIo {
        reader,
        writer: Arc::clone(&shared_writer),
    });
    let informational = InformationalResponseSink::new(shared_writer, write_timeout);
    (split_io, informational)
}

async fn finish_http_connection(
    mut io: BoxIo,
    raw_tail: &StdMutex<Option<Bytes>>,
    write_timeout: std::time::Duration,
    cancellation: &CancellationToken,
    intentional_wire_fault: &StdMutex<Option<IntentionalWireFault>>,
    exchange: &HttpExchangeConnection,
) -> Result<()> {
    let tail = raw_tail.lock().expect("raw tail mutex poisoned").take();
    match finish_downstream_write(
        &mut io,
        tail,
        write_timeout,
        cancellation,
        intentional_wire_fault,
    )
    .await
    {
        Ok(()) => {
            exchange.shutdown();
            Ok(())
        }
        Err(error) => {
            exchange.fail(&error).await;
            Err(error)
        }
    }
}

fn connection_error(
    hyper_error: &hyper::Error,
    handler_error: &StdMutex<Option<ProxyError>>,
    intentional_wire_fault: &StdMutex<Option<IntentionalWireFault>>,
) -> ProxyError {
    let original = handler_error
        .lock()
        .expect("handler error mutex poisoned")
        .take();
    if let Some(original) = original {
        return original;
    }
    let fault = *intentional_wire_fault
        .lock()
        .expect("intentional wire fault mutex poisoned");
    if let Some(fault) = fault {
        return fault.error();
    }
    ProxyError::new(
        ErrorCode::Io,
        format!("HTTP/1.1 connection failed: {hyper_error}"),
    )
}

fn unsupported_exchange_response(
    wire: &RequestWireState<'_>,
    cancellation: &CancellationToken,
) -> Result<Response<WireBody>> {
    const MESSAGE: &str = "HTTP CONNECT and Upgrade are not supported by the Exchange runtime";
    let mut headers = http::HeaderMap::new();
    headers.insert(
        http::header::CONTENT_TYPE,
        http::HeaderValue::from_static("text/plain; charset=utf-8"),
    );
    response_from_disposition(
        ResponseDisposition::Send {
            message: Message::response(
                http::StatusCode::NOT_IMPLEMENTED,
                &headers,
                Bytes::from_static(MESSAGE.as_bytes()),
            ),
            schedule: crate::traffic::TrafficSchedule::default(),
        },
        wire.canonical_response_head,
        wire.raw_tail,
        wire.intentional_wire_fault,
        cancellation,
    )
}

fn is_upgrade_request(headers: &http::HeaderMap) -> bool {
    headers.contains_key(http::header::UPGRADE)
        && headers
            .get_all(http::header::CONNECTION)
            .iter()
            .filter_map(|value| value.to_str().ok())
            .flat_map(|value| value.split(','))
            .any(|token| token.trim().eq_ignore_ascii_case("upgrade"))
}
