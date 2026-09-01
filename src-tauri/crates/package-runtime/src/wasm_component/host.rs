use futures_util::{SinkExt, StreamExt};
use intercept_proxy_domain::DomainError;
use wasmtime::component::{Resource, ResourceTable};
use wasmtime_wasi::{FsPerms, WasiCtx, WasiCtxBuilder, WasiCtxView, WasiView};
use wasmtime_wasi_http::{WasiHttpCtx, WasiHttpCtxView, WasiHttpView};

use super::{http_bindings, invalid_component, socket_bindings};

type HostWebSocketStream =
    tokio_tungstenite::WebSocketStream<tokio_tungstenite::MaybeTlsStream<tokio::net::TcpStream>>;

/// Internal concrete resource backing the versioned Host WebSocket WIT.
#[doc(hidden)]
pub struct HostWebSocket {
    stream: HostWebSocketStream,
}

impl std::fmt::Debug for HostWebSocket {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("HostWebSocket(..)")
    }
}

pub(super) struct HostState {
    table: ResourceTable,
    wasi: WasiCtx,
    http: WasiHttpCtx,
}

impl WasiView for HostState {
    fn ctx(&mut self) -> WasiCtxView<'_> {
        WasiCtxView {
            ctx: &mut self.wasi,
            table: &mut self.table,
        }
    }
}

impl WasiHttpView for HostState {
    fn http(&mut self) -> WasiHttpCtxView<'_> {
        WasiHttpCtxView {
            ctx: &mut self.http,
            table: &mut self.table,
            hooks: Default::default(),
        }
    }
}

macro_rules! impl_websocket_host {
    ($bindings:ident) => {
        impl $bindings::intercept_proxy::protocol_package::websocket::HostConnection
            for HostState
        {
            async fn open(&mut self, url: String) -> Result<Resource<HostWebSocket>, String> {
                let (stream, _) = tokio_tungstenite::connect_async(&url)
                    .await
                    .map_err(|error| error.to_string())?;
                self.table
                    .push(HostWebSocket { stream })
                    .map_err(|error| error.to_string())
            }

            async fn send_text(
                &mut self,
                connection: Resource<HostWebSocket>,
                value: String,
            ) -> Result<(), String> {
                self.table
                    .get_mut(&connection)
                    .map_err(|error| error.to_string())?
                    .stream
                    .send(tokio_tungstenite::tungstenite::Message::Text(value.into()))
                    .await
                    .map_err(|error| error.to_string())
            }

            async fn send_binary(
                &mut self,
                connection: Resource<HostWebSocket>,
                value: Vec<u8>,
            ) -> Result<(), String> {
                self.table
                    .get_mut(&connection)
                    .map_err(|error| error.to_string())?
                    .stream
                    .send(tokio_tungstenite::tungstenite::Message::Binary(value.into()))
                    .await
                    .map_err(|error| error.to_string())
            }

            async fn receive(
                &mut self,
                connection: Resource<HostWebSocket>,
            ) -> Result<
                $bindings::intercept_proxy::protocol_package::websocket::Message,
                String,
            > {
                use $bindings::intercept_proxy::protocol_package::websocket::Message as GuestMessage;
                loop {
                    let message = self
                        .table
                        .get_mut(&connection)
                        .map_err(|error| error.to_string())?
                        .stream
                        .next()
                        .await
                        .ok_or_else(|| {
                            "WebSocket connection ended without a close frame".to_owned()
                        })?
                        .map_err(|error| error.to_string())?;
                    match message {
                        tokio_tungstenite::tungstenite::Message::Text(value) => {
                            return Ok(GuestMessage::Text(value.to_string()));
                        }
                        tokio_tungstenite::tungstenite::Message::Binary(value) => {
                            return Ok(GuestMessage::Binary(value.to_vec()));
                        }
                        tokio_tungstenite::tungstenite::Message::Close(frame) => {
                            return Ok(GuestMessage::Closed(
                                frame.map(|frame| frame.reason.to_string()),
                            ));
                        }
                        tokio_tungstenite::tungstenite::Message::Ping(_)
                        | tokio_tungstenite::tungstenite::Message::Pong(_)
                        | tokio_tungstenite::tungstenite::Message::Frame(_) => {}
                    }
                }
            }

            async fn close(
                &mut self,
                connection: Resource<HostWebSocket>,
            ) -> Result<(), String> {
                self.table
                    .get_mut(&connection)
                    .map_err(|error| error.to_string())?
                    .stream
                    .close(None)
                    .await
                    .map_err(|error| error.to_string())
            }

            async fn drop(
                &mut self,
                connection: Resource<HostWebSocket>,
            ) -> wasmtime::Result<()> {
                self.table.delete(connection)?;
                Ok(())
            }
        }

        impl $bindings::intercept_proxy::protocol_package::websocket::Host for HostState {}
    };
}

impl_websocket_host!(http_bindings);
impl_websocket_host!(socket_bindings);
impl http_bindings::intercept_proxy::protocol_package::types::Host for HostState {}
impl socket_bindings::intercept_proxy::protocol_package::types::Host for HostState {}

pub(super) fn host_state() -> Result<HostState, DomainError> {
    let mut wasi = WasiCtxBuilder::new();
    wasi.inherit_stdio()
        .inherit_env()
        .inherit_network()
        .allow_ip_name_lookup(true)
        .allow_tcp(true)
        .allow_udp(true);
    #[cfg(unix)]
    wasi.preopened_dir("/", "/", FsPerms::ReadWrite)
        .map_err(|error| invalid_component(format!("cannot expose host root to WASI: {error}")))?;
    #[cfg(windows)]
    for drive in b'A'..=b'Z' {
        let root = format!("{}:\\", char::from(drive));
        if std::path::Path::new(&root).is_dir() {
            let guest = format!("/host/{}", char::from(drive).to_ascii_lowercase());
            wasi.preopened_dir(&root, guest, FsPerms::ReadWrite)
                .map_err(|error| {
                    invalid_component(format!("cannot expose host drive {root} to WASI: {error}"))
                })?;
        }
    }
    Ok(HostState {
        table: ResourceTable::new(),
        wasi: wasi.build(),
        http: WasiHttpCtx::new(),
    })
}
