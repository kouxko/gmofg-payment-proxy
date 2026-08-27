//! 单连接 actor 的严格注册阶段。

use futures_util::{SinkExt, StreamExt};
use serde_json::Value;
use tokio::{
    io::{AsyncRead, AsyncWrite},
    time::{Instant, MissedTickBehavior, timeout_at},
};
use tokio_tungstenite::{WebSocketStream, tungstenite::Message};

use super::ExternalPackageConnectionConfig;
use crate::adapters::external_packages::{
    ExternalPackageConnectionError, ExternalPackageFatalProtocolError,
    actor::response::{ParsedResponse, parse_response},
};

pub(super) async fn register<S>(
    websocket: &mut WebSocketStream<S>,
    register_id: &str,
    request: &Value,
    config: &ExternalPackageConnectionConfig,
) -> Result<Value, ExternalPackageConnectionError>
where
    S: AsyncRead + AsyncWrite + Unpin,
{
    let deadline_at = Instant::now() + config.registration_timeout;
    match timeout_at(
        deadline_at,
        websocket.send(Message::Text(request.to_string().into())),
    )
    .await
    {
        Ok(result) => result.map_err(ExternalPackageConnectionError::from)?,
        Err(_) => return Err(registration_timeout(register_id)),
    }
    let deadline = tokio::time::sleep_until(deadline_at);
    tokio::pin!(deadline);
    let mut heartbeat = tokio::time::interval_at(
        Instant::now() + config.heartbeat_interval,
        config.heartbeat_interval,
    );
    heartbeat.set_missed_tick_behavior(MissedTickBehavior::Delay);
    let mut last_pong = Instant::now();
    loop {
        tokio::select! {
            biased;
            () = &mut deadline => return Err(registration_timeout(register_id)),
            message = websocket.next() => match message {
                Some(Ok(Message::Text(text))) => {
                    if text.len() > config.max_registration_message_bytes {
                        return Err(ExternalPackageConnectionError::MessageTooLarge {
                            actual_bytes: text.len(), limit_bytes: config.max_registration_message_bytes,
                        });
                    }
                    return match parse_response(&text) {
                        Ok(ParsedResponse::Result { request_id, result }) if request_id == register_id => Ok(result),
                        Ok(ParsedResponse::Error { request_id, error }) if request_id == register_id => Err(ExternalPackageConnectionError::Remote {
                            request_id, method: "package.register".into(), error,
                        }),
                        Ok(_) => Err(ExternalPackageConnectionError::Fatal(ExternalPackageFatalProtocolError::WrongResponseId)),
                        Err(kind) => Err(ExternalPackageConnectionError::Fatal(kind)),
                    };
                }
                Some(Ok(Message::Ping(_))) => match timeout_at(deadline_at, websocket.flush()).await {
                    Ok(result) => result.map_err(ExternalPackageConnectionError::from)?,
                    Err(_) => return Err(registration_timeout(register_id)),
                },
                Some(Ok(Message::Pong(_))) => last_pong = Instant::now(),
                Some(Ok(Message::Close(_))) | None => return Err(ExternalPackageConnectionError::Disconnected),
                Some(Err(error)) => return Err(ExternalPackageConnectionError::Transport(error.to_string())),
                Some(Ok(Message::Binary(_) | Message::Frame(_))) => return Err(ExternalPackageConnectionError::Fatal(ExternalPackageFatalProtocolError::RegistrationProtocolViolation)),
            },
            _ = heartbeat.tick() => {
                if Instant::now().duration_since(last_pong) >= config.heartbeat_timeout {
                    return Err(ExternalPackageConnectionError::Disconnected);
                }
                match timeout_at(
                    deadline_at,
                    websocket.send(Message::Ping(Vec::new().into())),
                )
                .await
                {
                    Ok(result) => result.map_err(ExternalPackageConnectionError::from)?,
                    Err(_) => return Err(registration_timeout(register_id)),
                }
            }
        }
    }
}

fn registration_timeout(register_id: &str) -> ExternalPackageConnectionError {
    ExternalPackageConnectionError::Timeout {
        request_id: register_id.to_owned(),
        method: "package.register".into(),
    }
}
