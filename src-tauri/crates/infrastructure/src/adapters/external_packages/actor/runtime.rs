//! 单 WebSocket 连接 Actor 的注册、相关性、心跳与关闭循环。

use std::{collections::HashMap, time::Duration};

use futures_util::{SinkExt, StreamExt};
use intercept_proxy_domain::ExternalPackageRegistration;
use serde_json::json;
use tokio::{
    io::{AsyncRead, AsyncWrite},
    sync::{mpsc, oneshot, watch},
    time::{Instant, MissedTickBehavior},
};
use tokio_tungstenite::{
    WebSocketStream,
    tungstenite::{Error as WebSocketError, Message, error::CapacityError},
};

use super::{
    config::ExternalPackageConnectionConfig,
    protocol::{ControlCommand, DataCommand, PendingCall},
    recent_ids::RecentRequestIds,
    registration::register,
    response::{ParsedResponse, parse_registration, parse_response},
};
use crate::adapters::external_packages::error::{
    ExternalPackageConnectionError, ExternalPackageFatalProtocolError,
};

#[allow(
    clippy::too_many_lines,
    reason = "the select loop keeps one owner for websocket, correlation, heartbeat, and close state"
)]
pub(super) async fn run_actor<S>(
    mut websocket: WebSocketStream<S>,
    generation: u64,
    config: ExternalPackageConnectionConfig,
    mut commands: mpsc::Receiver<DataCommand>,
    mut controls: mpsc::Receiver<ControlCommand>,
    registration: oneshot::Sender<
        Result<ExternalPackageRegistration, ExternalPackageConnectionError>,
    >,
    closed: watch::Sender<Option<ExternalPackageConnectionError>>,
) where
    S: AsyncRead + AsyncWrite + Unpin + Send + 'static,
{
    let register_id = format!("register-{generation}");
    let request =
        json!({"jsonrpc":"2.0","id":register_id,"method":"package.register","params":{"api":1}});
    let registration_result = register(&mut websocket, &register_id, &request, &config).await;
    let registration_value = match registration_result {
        Ok(value) => value,
        Err(error) => {
            let _ = registration.send(Err(error.clone()));
            let _ = closed.send(Some(error));
            let _ = tokio::time::timeout(config.write_timeout(), websocket.close(None)).await;
            return;
        }
    };
    let parsed_registration =
        parse_registration(registration_value).map_err(ExternalPackageConnectionError::Fatal);
    let parsed_registration = match parsed_registration {
        Ok(value) => value,
        Err(error) => {
            let _ = registration.send(Err(error.clone()));
            let _ = closed.send(Some(error));
            let _ = tokio::time::timeout(config.write_timeout(), websocket.close(None)).await;
            return;
        }
    };
    // 必须继续使用完成注册的同一个 WebSocket 状态机。重建实例会丢弃 tungstenite 已预读、
    // 但尚未产出的相邻帧（例如与注册响应同批到达的 Ping 或首个业务响应）。
    if registration.send(Ok(parsed_registration)).is_err() {
        let _ = tokio::time::timeout(config.write_timeout(), websocket.close(None)).await;
        return;
    }

    let mut pending = HashMap::<String, PendingCall>::new();
    let history_capacity = config.max_in_flight.saturating_mul(4).clamp(1_024, 16_384);
    let mut completed = RecentRequestIds::new(history_capacity);
    let mut cancelled = RecentRequestIds::new(history_capacity);
    let mut heartbeat = tokio::time::interval_at(
        Instant::now() + config.heartbeat_interval,
        config.heartbeat_interval,
    );
    heartbeat.set_missed_tick_behavior(MissedTickBehavior::Delay);
    let mut last_pong = Instant::now();
    let close_reason = loop {
        tokio::select! {
            biased;
            control = controls.recv() => match control {
                Some(ControlCommand::ProtocolFatal(kind)) => break ExternalPackageConnectionError::Fatal(kind),
                Some(ControlCommand::Close) | None => break ExternalPackageConnectionError::Disconnected,
            },
            command = commands.recv() => match command {
                Some(DataCommand::Call(call)) => {
                    let text = json!({"jsonrpc":"2.0","id":call.request_id,"method":call.method,"params":call.params}).to_string();
                    let request_limit = config
                        .max_rpc_message_bytes
                        .min(config.registration_websocket_message_bytes());
                    if text.len() > request_limit {
                        let _ = call.response.send(Err(ExternalPackageConnectionError::MessageTooLarge {
                            actual_bytes: text.len(), limit_bytes: request_limit,
                        }));
                        completed.insert(call.request_id);
                        continue;
                    }
                    pending.insert(call.request_id.clone(), PendingCall {
                        method: call.method,
                        response_limit: call.response_limit,
                        response: call.response,
                    });
                    let write_deadline = config
                        .write_timeout()
                        .saturating_add(Duration::from_millis(1));
                    if !matches!(
                        tokio::time::timeout(write_deadline, websocket.send(Message::Text(text.into()))).await,
                        Ok(Ok(()))
                    ) {
                        break ExternalPackageConnectionError::Disconnected;
                    }
                }
                Some(DataCommand::Cancel(request_id)) => {
                    if pending.remove(&request_id).is_some() {
                        cancelled.insert(request_id);
                    }
                }
                None => break ExternalPackageConnectionError::Disconnected,
            },
            message = websocket.next() => match message {
                Some(Ok(Message::Text(text))) => {
                    let response = match parse_response(&text) {
                        Ok(response) => response,
                        Err(kind) => break ExternalPackageConnectionError::Fatal(kind),
                    };
                    let request_id = match &response {
                        ParsedResponse::Result { request_id, .. } | ParsedResponse::Error { request_id, .. } => request_id.clone(),
                    };
                    if cancelled.remove(&request_id) { continue; }
                    if completed.contains(&request_id) {
                        break ExternalPackageConnectionError::Fatal(ExternalPackageFatalProtocolError::DuplicateResponse);
                    }
                    let Some(call) = pending.remove(&request_id) else {
                        break ExternalPackageConnectionError::Fatal(ExternalPackageFatalProtocolError::WrongResponseId);
                    };
                    if text.len() > call.response_limit {
                        let _ = call.response.send(Err(ExternalPackageConnectionError::MessageTooLarge {
                            actual_bytes: text.len(), limit_bytes: call.response_limit,
                        }));
                        completed.insert(request_id);
                        continue;
                    }
                    let result = match response {
                        ParsedResponse::Result { result, .. } => Ok(result),
                        ParsedResponse::Error { error, .. } => Err(ExternalPackageConnectionError::Remote {
                            request_id: request_id.clone(), method: call.method, error,
                        }),
                    };
                    let _ = call.response.send(result);
                    completed.insert(request_id);
                }
                Some(Ok(Message::Ping(_))) => {
                    // tungstenite 已按 RFC 自动排队 Pong；这里只负责及时 flush，避免重复发送控制帧。
                    if !matches!(
                        tokio::time::timeout(config.write_timeout(), websocket.flush()).await,
                        Ok(Ok(()))
                    ) {
                        break ExternalPackageConnectionError::Disconnected;
                    }
                }
                Some(Ok(Message::Pong(_))) => last_pong = Instant::now(),
                Some(Err(WebSocketError::Capacity(CapacityError::MessageTooLong { size, max_size }))) => {
                    break ExternalPackageConnectionError::MessageTooLarge {
                        actual_bytes: size,
                        limit_bytes: max_size,
                    };
                }
                Some(Ok(Message::Close(_)) | Err(_)) | None => break ExternalPackageConnectionError::Disconnected,
                Some(Ok(Message::Binary(_))) => break ExternalPackageConnectionError::Fatal(ExternalPackageFatalProtocolError::InvalidResponse),
                Some(Ok(Message::Frame(_))) => {}
            },
            _ = heartbeat.tick() => {
                if Instant::now().duration_since(last_pong) >= config.heartbeat_timeout {
                    break ExternalPackageConnectionError::Disconnected;
                }
                if !matches!(
                    tokio::time::timeout(
                        config.write_timeout(),
                        websocket.send(Message::Ping(Vec::new().into()))
                    ).await,
                    Ok(Ok(()))
                ) {
                    break ExternalPackageConnectionError::Disconnected;
                }
            }
        }
    };

    for (_, call) in pending {
        let _ = call.response.send(Err(close_reason.clone()));
    }
    // Close 也是写操作，对端停止读取时可能阻塞。发布终态前以本地期限约束它，保证
    // `disconnect`/`wait_closed` 最终收敛。
    let _ = tokio::time::timeout(config.write_timeout(), websocket.close(None)).await;
    let _ = closed.send(Some(close_reason));
}
