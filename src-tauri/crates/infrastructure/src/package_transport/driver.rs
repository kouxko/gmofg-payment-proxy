use std::collections::HashMap;

use futures_util::{SinkExt, StreamExt};
use intercept_proxy_domain::{DomainError, ErrorCode};
use intercept_proxy_package_contract::{
    PackageManifest, PackageRegisterNotification, PackageRpcFailure, PackageRpcSuccess,
};
use serde_json::Value;
use tokio::{
    io::{AsyncRead, AsyncWrite},
    sync::{mpsc, oneshot, watch},
    time::{Instant, MissedTickBehavior, timeout_at},
};
use tokio_tungstenite::{WebSocketStream, tungstenite::Message};

use super::{Call, PackageTransportConfig, PackageTransportError};

pub(super) async fn receive_registration<S>(
    websocket: &mut WebSocketStream<S>,
    config: &PackageTransportConfig,
) -> Result<PackageManifest, PackageTransportError>
where
    S: AsyncRead + AsyncWrite + Unpin,
{
    let deadline_at = Instant::now() + config.registration_timeout;
    let mut heartbeat = tokio::time::interval_at(
        Instant::now() + config.heartbeat_interval,
        config.heartbeat_interval,
    );
    heartbeat.set_missed_tick_behavior(MissedTickBehavior::Delay);
    let mut last_pong = Instant::now();
    loop {
        tokio::select! {
            message = websocket.next() => match message {
                Some(Ok(Message::Text(text))) => {
                    if text.len() > config.max_registration_message_bytes { return Err(PackageTransportError::MessageTooLarge { actual_bytes: text.len(), limit_bytes: config.max_registration_message_bytes }); }
                    let notification = serde_json::from_str::<PackageRegisterNotification>(&text).map_err(|error| PackageTransportError::Package { error: DomainError::new(ErrorCode::ProtocolPackageInvalid, "package.register notification is invalid").with_field_error("package.register", error.to_string()) })?;
                    return Ok(notification.params().clone());
                }
                Some(Ok(Message::Ping(_))) => websocket.flush().await.map_err(|error| PackageTransportError::Transport(error.to_string()))?,
                Some(Ok(Message::Pong(_))) => last_pong = Instant::now(),
                Some(Ok(_)) => return Err(PackageTransportError::Package { error: DomainError::new(ErrorCode::ProtocolPackageInvalid, "package.register must be one text notification") }),
                Some(Err(error)) => return Err(PackageTransportError::Transport(error.to_string())),
                None => return Err(PackageTransportError::Disconnected),
            },
            () = tokio::time::sleep_until(deadline_at) => return Err(PackageTransportError::RegistrationDeadline),
            _ = heartbeat.tick() => {
                if Instant::now().duration_since(last_pong) >= config.heartbeat_timeout { return Err(PackageTransportError::Disconnected); }
                timeout_at(deadline_at, websocket.send(Message::Ping(Vec::new().into()))).await.map_err(|_| PackageTransportError::RegistrationDeadline)?.map_err(|error| PackageTransportError::Transport(error.to_string()))?;
            }
        }
    }
}

#[derive(serde::Deserialize)]
#[serde(untagged)]
enum Response {
    Success(PackageRpcSuccess<Value>),
    Failure(PackageRpcFailure),
}

#[allow(clippy::too_many_lines)]
pub(super) async fn run_registered<S>(
    websocket: WebSocketStream<S>,
    config: PackageTransportConfig,
    mut calls: mpsc::UnboundedReceiver<Call>,
    mut close: mpsc::Receiver<()>,
    closed: watch::Sender<Option<PackageTransportError>>,
) where
    S: AsyncRead + AsyncWrite + Unpin + Send + 'static,
{
    let (mut sink, mut stream) = websocket.split();
    let (outgoing, mut outgoing_rx) = mpsc::unbounded_channel::<Message>();
    let mut writer = tokio::spawn(async move {
        while let Some(message) = outgoing_rx.recv().await {
            sink.send(message)
                .await
                .map_err(|error| error.to_string())?;
        }
        Ok::<(), String>(())
    });
    let mut pending = HashMap::<
        String,
        (
            &'static str,
            usize,
            oneshot::Sender<Result<Value, PackageTransportError>>,
        ),
    >::new();
    let mut heartbeat = tokio::time::interval_at(
        Instant::now() + config.heartbeat_interval,
        config.heartbeat_interval,
    );
    heartbeat.set_missed_tick_behavior(MissedTickBehavior::Delay);
    let mut last_pong = Instant::now();
    let reason = loop {
        tokio::select! {
            _ = close.recv() => break PackageTransportError::Disconnected,
            call = calls.recv() => match call {
                Some(call) => {
                    let Ok(text) = serde_json::to_string(&call.request) else { let _ = call.response.send(Err(PackageTransportError::InvalidResponse)); continue; };
                    if text.len() > config.max_rpc_message_bytes { let _ = call.response.send(Err(PackageTransportError::MessageTooLarge { actual_bytes: text.len(), limit_bytes: config.max_rpc_message_bytes })); continue; }
                    pending.insert(call.request_id.clone(), (call.method, call.response_limit, call.response));
                    if outgoing.send(Message::Text(text.into())).is_err() { break PackageTransportError::Disconnected; }
                }
                None => break PackageTransportError::Disconnected,
            },
            message = stream.next() => match message {
                Some(Ok(Message::Text(text))) => {
                    let Ok(response) = serde_json::from_str::<Response>(&text) else { break PackageTransportError::InvalidResponse };
                    let (id, reply) = match response {
                        Response::Success(success) => { let _ = success.jsonrpc; (success.id, Ok(success.result)) }
                        Response::Failure(failure) => { let _ = failure.jsonrpc; let id = failure.id; (id.clone(), Err(failure.error)) }
                    };
                    let Some((method, limit, sender)) = pending.remove(&id) else { break PackageTransportError::InvalidResponse; };
                    if text.len() > limit { let _ = sender.send(Err(PackageTransportError::MessageTooLarge { actual_bytes: text.len(), limit_bytes: limit })); continue; }
                    let reply = reply.map_err(|error| PackageTransportError::Remote { request_id: id.clone(), method, error });
                    let _ = sender.send(reply);
                }
                Some(Ok(Message::Ping(payload))) => { if outgoing.send(Message::Pong(payload)).is_err() { break PackageTransportError::Disconnected; } },
                Some(Ok(Message::Pong(_))) => last_pong = Instant::now(),
                Some(Ok(Message::Close(_))) | None => break PackageTransportError::Disconnected,
                Some(Ok(_)) => break PackageTransportError::InvalidResponse,
                Some(Err(error)) => break PackageTransportError::Transport(error.to_string()),
            },
            _ = heartbeat.tick() => {
                if Instant::now().duration_since(last_pong) >= config.heartbeat_timeout { break PackageTransportError::Disconnected; }
                if outgoing.send(Message::Ping(Vec::new().into())).is_err() { break PackageTransportError::Disconnected; }
            }
            result = &mut writer => break match result {
                Ok(Ok(())) => PackageTransportError::Disconnected,
                Ok(Err(error)) => PackageTransportError::Transport(error),
                Err(error) => PackageTransportError::Transport(error.to_string()),
            },
        }
    };
    for (_, (_, _, sender)) in pending {
        let _ = sender.send(Err(reason.clone()));
    }
    drop(outgoing);
    writer.abort();
    let _ = closed.send(Some(reason));
}
