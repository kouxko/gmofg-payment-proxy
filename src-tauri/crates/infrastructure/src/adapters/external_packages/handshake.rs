use tokio::io::{AsyncRead, AsyncWrite};
use tokio_tungstenite::{
    WebSocketStream, accept_hdr_async_with_config,
    tungstenite::{
        Error,
        handshake::server::{ErrorResponse, Request, Response},
        http::StatusCode,
        protocol::WebSocketConfig,
    },
};

/// 接受且仅接受请求目标为 `/packages` 的 WebSocket 握手。
///
/// 查询串也会被拒绝，以保证服务只有一个无歧义入口。WebSocket RFC 握手、帧解析和控制帧均委托给
/// `tokio-tungstenite`，本适配器不自行实现协议细节。
#[allow(
    clippy::result_large_err,
    reason = "tungstenite handshake callbacks require their concrete HTTP error response type"
)]
pub async fn accept_packages_websocket<S>(
    stream: S,
    max_message_bytes: usize,
) -> Result<WebSocketStream<S>, Error>
where
    S: AsyncRead + AsyncWrite + Unpin,
{
    assert!(
        max_message_bytes > 0,
        "WebSocket message limit must be positive"
    );
    let websocket_config = WebSocketConfig::default()
        .max_message_size(Some(max_message_bytes))
        .max_frame_size(Some(max_message_bytes));
    accept_hdr_async_with_config(
        stream,
        |request: &Request, response: Response| {
            if request.uri().path() == "/packages" && request.uri().query().is_none() {
                Ok(response)
            } else {
                let mut rejected =
                    ErrorResponse::new(Some("WebSocket 路径必须是 /packages".into()));
                *rejected.status_mut() = StatusCode::NOT_FOUND;
                Err(rejected)
            }
        },
        Some(websocket_config),
    )
    .await
}
