use std::sync::{Arc, Mutex};

use async_trait::async_trait;
use intercept_proxy_exchange::{
    Connection, Downstream, Error, Http, HttpContext, Reader, Upstream, WriteOutcome, Writer,
};
use tokio::sync::mpsc;

use super::{
    HttpExchangeInput, HttpExchangeState, ProxyError, ResponseDisposition, adapter_error,
    message_context, proxy_write_error, write_error,
};
use crate::fault::project_response_for_observation;

pub(in crate::http::exchange_runtime) struct BufferedApp {
    reader: BufferedAppReader,
    writer: BufferedAppWriter,
}

impl BufferedApp {
    pub(in crate::http::exchange_runtime) fn new(
        state: Arc<Mutex<HttpExchangeState>>,
        receiver: mpsc::Receiver<HttpExchangeInput>,
        endpoint: String,
    ) -> Self {
        Self {
            reader: BufferedAppReader {
                state: Arc::clone(&state),
                receiver,
                endpoint,
            },
            writer: BufferedAppWriter { state },
        }
    }
}

#[async_trait]
impl Connection<Http, Upstream, Downstream> for BufferedApp {
    fn reader(&mut self) -> &mut dyn Reader<Http, Upstream> {
        &mut self.reader
    }

    fn writer(&mut self) -> &mut dyn Writer<Http, Downstream> {
        &mut self.writer
    }

    async fn shutdown(&mut self) -> Result<(), Error> {
        Ok(())
    }
}

struct BufferedAppReader {
    state: Arc<Mutex<HttpExchangeState>>,
    receiver: mpsc::Receiver<HttpExchangeInput>,
    endpoint: String,
}

#[async_trait]
impl Reader<Http, Upstream> for BufferedAppReader {
    async fn read(&mut self) -> Result<Option<HttpContext>, Error> {
        let Some(input) = self.receiver.recv().await else {
            return Ok(None);
        };
        let command = match input {
            HttpExchangeInput::Request(command) => command,
            HttpExchangeInput::Fail(error) => return Err(proxy_write_error(error)),
        };
        if command.endpoint != self.endpoint {
            let error = endpoint_changed(&self.endpoint, &command.endpoint);
            let _ = command
                .completed
                .send(Err(super::super::exchange_error(error.clone())));
            return Err(error);
        }
        self.state
            .lock()
            .expect("HTTP Exchange state mutex poisoned")
            .begin(command)
            .map_err(proxy_write_error)?;
        let state = self
            .state
            .lock()
            .expect("HTTP Exchange state mutex poisoned");
        let transaction = state
            .current
            .as_ref()
            .ok_or_else(|| adapter_error("HTTP_TRANSACTION_MISSING", "HTTP transaction missing"))?;
        let request = transaction
            .request
            .as_ref()
            .ok_or_else(|| adapter_error("HTTP_REQUEST_MISSING", "HTTP request missing"))?;
        Ok(Some(message_context(request)))
    }
}

fn endpoint_changed(previous: &str, current: &str) -> Error {
    adapter_error(
        "HTTP_ENDPOINT_CHANGED",
        format!("HTTP connection Endpoint changed from {previous} to {current}"),
    )
}

pub(super) struct BufferedAppWriter {
    pub(super) state: Arc<Mutex<HttpExchangeState>>,
}

#[async_trait]
impl Writer<Http, Downstream> for BufferedAppWriter {
    async fn write(&mut self, _context: HttpContext) -> Result<WriteOutcome<HttpContext>, Error> {
        let (mut response, actions, mocked, close_requested, cancellation) = {
            let mut state = self
                .state
                .lock()
                .expect("HTTP Exchange state mutex poisoned");
            let transaction = state.current.as_mut().ok_or_else(|| {
                write_error("HTTP_TRANSACTION_MISSING", "HTTP transaction missing")
            })?;
            let response = transaction.response.take().ok_or_else(|| {
                write_error(
                    "HTTP_RESPONSE_MISSING",
                    "HTTP response missing before App write",
                )
            })?;
            let actions = transaction.response_actions.take().ok_or_else(|| {
                write_error(
                    "HTTP_RESPONSE_NOT_PREPARED",
                    "HTTP response actions missing before App write",
                )
            })?;
            (
                response,
                actions,
                transaction.mocked,
                transaction.close_requested,
                state.cancellation.clone(),
            )
        };
        if close_requested {
            response.remove_header("connection");
            response
                .headers
                .push(crate::message::RawHeader::new("Connection", "close"));
        }
        let written = project_response_for_observation(response.clone(), &actions)
            .map_err(proxy_write_error)?
            .map_or_else(
                || message_context(&response),
                |message| message_context(&message),
            );
        let disposition = if mocked {
            ResponseDisposition::Send {
                message: response,
                schedule: crate::traffic::TrafficSchedule::default(),
            }
        } else {
            crate::fault::apply_response_actions(response, &actions, &cancellation)
                .await
                .map_err(proxy_write_error)?
        };
        let write_completed = {
            let mut state = self
                .state
                .lock()
                .expect("HTTP Exchange state mutex poisoned");
            state
                .current
                .as_mut()
                .ok_or_else(|| write_error("HTTP_TRANSACTION_MISSING", "HTTP transaction missing"))?
                .disposition = Some(disposition);
            state.complete().ok_or_else(|| {
                write_error(
                    "HTTP_WRITE_COMPLETION_MISSING",
                    "HTTP response write completion channel missing",
                )
            })?
        };
        let completed = tokio::select! {
            () = cancellation.cancelled() => Err(ProxyError::new(
                crate::ErrorCode::ProxyStopped,
                "proxy stopped before downstream response write completed",
            )),
            completed = write_completed => completed.unwrap_or_else(|_| Err(ProxyError::new(
                crate::ErrorCode::Io,
                "downstream response write completion sender dropped",
            ))),
        };
        completed.map_err(proxy_write_error)?;
        Ok(WriteOutcome::Committed(written))
    }
}
