//! Single-file WebAssembly Component package validation.

use std::borrow::Cow;

use futures_util::{SinkExt, StreamExt};
use intercept_proxy_domain::{Document, DomainError, ErrorCode, ProtocolDirection};
use intercept_proxy_package_contract::{FrameResult, PackageKind, PackageManifest};
use wasm_encoder::{ComponentSection, CustomSection};
use wasmparser::{Encoding, Parser, Payload, Validator, WasmFeatures};
use wasmtime::component::{Component, Linker, Resource, ResourceTable};
use wasmtime::{Config, Engine, Store};
use wasmtime_wasi::{FsPerms, WasiCtx, WasiCtxBuilder, WasiCtxView, WasiView};
use wasmtime_wasi_http::{WasiHttpCtx, WasiHttpCtxView, WasiHttpView};

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

mod http_bindings {
    wasmtime::component::bindgen!({
        path: "wit",
        world: "http-package",
        imports: { default: async },
        exports: { default: async },
        require_store_data_send: true,
        with: {
            "intercept-proxy:protocol-package/websocket.connection": crate::wasm_component::HostWebSocket,
        },
    });
}

mod socket_bindings {
    wasmtime::component::bindgen!({
        path: "wit",
        world: "socket-package",
        imports: { default: async },
        exports: { default: async },
        require_store_data_send: true,
        with: {
            "intercept-proxy:protocol-package/websocket.connection": crate::wasm_component::HostWebSocket,
        },
    });
}

/// Top-level custom section containing the strict package Manifest JSON.
pub const PACKAGE_MANIFEST_SECTION: &str = "intercept-proxy:manifest";

/// Validated single-file WebAssembly Component package.
#[derive(Clone, Debug)]
pub struct PackageComponent {
    manifest: PackageManifest,
    bytes: Vec<u8>,
}

struct HostState {
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
            async fn open(
                &mut self,
                url: String,
            ) -> Result<Resource<HostWebSocket>, String> {
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
                        .ok_or_else(|| "WebSocket connection ended without a close frame".to_owned())?
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

enum PackageInstance {
    Http {
        store: Store<HostState>,
        bindings: http_bindings::HttpPackage,
    },
    Socket {
        store: Store<HostState>,
        bindings: socket_bindings::SocketPackage,
    },
}

/// One validated package Component instantiated inside the current process.
pub struct WasmPackageRuntime {
    instance: PackageInstance,
}

impl std::fmt::Debug for WasmPackageRuntime {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let kind = match &self.instance {
            PackageInstance::Http { .. } => PackageKind::Http,
            PackageInstance::Socket { .. } => PackageKind::Socket,
        };
        formatter
            .debug_struct("WasmPackageRuntime")
            .field("kind", &kind)
            .finish_non_exhaustive()
    }
}

impl PackageComponent {
    /// Returns the strict embedded package Manifest.
    #[must_use]
    pub const fn manifest(&self) -> &PackageManifest {
        &self.manifest
    }

    /// Returns the exact imported Component bytes.
    #[must_use]
    pub fn bytes(&self) -> &[u8] {
        &self.bytes
    }
}

fn invalid_component(message: impl Into<String>) -> DomainError {
    DomainError::new(ErrorCode::ProtocolPackageInvalid, message)
}

fn engine() -> Result<Engine, DomainError> {
    let mut config = Config::new();
    config.wasm_component_model(true);
    Engine::new(&config).map_err(|error| invalid_component(error.to_string()))
}

fn host_state() -> Result<HostState, DomainError> {
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

impl WasmPackageRuntime {
    /// Compiles and instantiates the manifest-selected WIT world.
    pub async fn load(package: &PackageComponent) -> Result<Self, DomainError> {
        let engine = engine()?;
        let component = Component::new(&engine, package.bytes())
            .map_err(|error| invalid_component(error.to_string()))?;
        let mut linker = Linker::<HostState>::new(&engine);
        wasmtime_wasi::p2::add_to_linker_async(&mut linker)
            .map_err(|error| invalid_component(error.to_string()))?;
        wasmtime_wasi_http::p2::add_only_http_to_linker_async(&mut linker)
            .map_err(|error| invalid_component(error.to_string()))?;
        match package.manifest().kind() {
            PackageKind::Http => {
                http_bindings::HttpPackage::add_to_linker::<
                    HostState,
                    wasmtime::component::HasSelf<HostState>,
                >(&mut linker, |state| state)
                .map_err(|error| invalid_component(error.to_string()))?;
            }
            PackageKind::Socket => {
                socket_bindings::SocketPackage::add_to_linker::<
                    HostState,
                    wasmtime::component::HasSelf<HostState>,
                >(&mut linker, |state| state)
                .map_err(|error| invalid_component(error.to_string()))?;
            }
        }
        let mut store = Store::new(&engine, host_state()?);
        let instance = match package.manifest().kind() {
            PackageKind::Http => PackageInstance::Http {
                bindings: http_bindings::HttpPackage::instantiate_async(
                    &mut store, &component, &linker,
                )
                .await
                .map_err(|error| invalid_component(error.to_string()))?,
                store,
            },
            PackageKind::Socket => PackageInstance::Socket {
                bindings: socket_bindings::SocketPackage::instantiate_async(
                    &mut store, &component, &linker,
                )
                .await
                .map_err(|error| invalid_component(error.to_string()))?,
                store,
            },
        };
        Ok(Self { instance })
    }

    /// Calls the Socket frame export for one direction.
    pub async fn frame(
        &mut self,
        direction: ProtocolDirection,
        buffer: &[u8],
    ) -> Result<FrameResult, DomainError> {
        let PackageInstance::Socket { store, bindings } = &mut self.instance else {
            return Err(invalid_component(
                "HTTP protocol packages do not export Socket frame hooks",
            ));
        };
        let result = match direction {
            ProtocolDirection::Upstream => bindings.call_upstream_frame(store, buffer).await,
            ProtocolDirection::Downstream => bindings.call_downstream_frame(store, buffer).await,
        }
        .map_err(|error| runtime_trap(&error))?
        .map_err(|error| runtime_guest_error(&error.code, &error.message))?;
        let result = match result {
            socket_bindings::intercept_proxy::protocol_package::types::FrameResult::NeedMore(
                required_bytes,
            ) => FrameResult::NeedMore {
                required_bytes: required_bytes
                    .map(usize::try_from)
                    .transpose()
                    .map_err(|_| invalid_component("required frame bytes exceed host usize"))?,
            },
            socket_bindings::intercept_proxy::protocol_package::types::FrameResult::Complete(
                consumed_bytes,
            ) => FrameResult::complete(
                usize::try_from(consumed_bytes)
                    .map_err(|_| invalid_component("consumed frame bytes exceed host usize"))?,
            )?,
            socket_bindings::intercept_proxy::protocol_package::types::FrameResult::Reject(
                reason,
            ) => FrameResult::Reject { reason },
        };
        result.validate_against_buffer_len(buffer.len())?;
        Ok(result)
    }

    /// Calls an HTTP decode export and parses its Document JSON.
    pub async fn decode_http(
        &mut self,
        direction: ProtocolDirection,
        input: &str,
    ) -> Result<Document, DomainError> {
        let PackageInstance::Http { store, bindings } = &mut self.instance else {
            return Err(invalid_component(
                "Socket protocol packages do not export HTTP decode hooks",
            ));
        };
        let document_json = match direction {
            ProtocolDirection::Upstream => bindings.call_upstream_decode(store, input).await,
            ProtocolDirection::Downstream => bindings.call_downstream_decode(store, input).await,
        }
        .map_err(|error| runtime_trap(&error))?
        .map_err(|error| runtime_guest_error(&error.code, &error.message))?;
        parse_document_json(&document_json)
    }

    /// Calls a Socket decode export with raw frame bytes and parses its Document JSON.
    pub async fn decode_socket(
        &mut self,
        direction: ProtocolDirection,
        input: &[u8],
    ) -> Result<Document, DomainError> {
        let PackageInstance::Socket { store, bindings } = &mut self.instance else {
            return Err(invalid_component(
                "HTTP protocol packages do not export Socket decode hooks",
            ));
        };
        let document_json = match direction {
            ProtocolDirection::Upstream => bindings.call_upstream_decode(store, input).await,
            ProtocolDirection::Downstream => bindings.call_downstream_decode(store, input).await,
        }
        .map_err(|error| runtime_trap(&error))?
        .map_err(|error| runtime_guest_error(&error.code, &error.message))?;
        parse_document_json(&document_json)
    }

    /// Calls an HTTP encode export.
    pub async fn encode_http(
        &mut self,
        direction: ProtocolDirection,
        original_input: &str,
        document: &Document,
    ) -> Result<String, DomainError> {
        let document_json = document_json(document)?;
        let PackageInstance::Http { store, bindings } = &mut self.instance else {
            return Err(invalid_component(
                "Socket protocol packages do not export HTTP encode hooks",
            ));
        };
        match direction {
            ProtocolDirection::Upstream => {
                bindings
                    .call_upstream_encode(store, original_input, &document_json)
                    .await
            }
            ProtocolDirection::Downstream => {
                bindings
                    .call_downstream_encode(store, original_input, &document_json)
                    .await
            }
        }
        .map_err(|error| runtime_trap(&error))?
        .map_err(|error| runtime_guest_error(&error.code, &error.message))
    }

    /// Calls a Socket encode export and returns raw frame bytes.
    pub async fn encode_socket(
        &mut self,
        direction: ProtocolDirection,
        original_input: &[u8],
        document: &Document,
    ) -> Result<Vec<u8>, DomainError> {
        let document_json = document_json(document)?;
        let PackageInstance::Socket { store, bindings } = &mut self.instance else {
            return Err(invalid_component(
                "HTTP protocol packages do not export Socket encode hooks",
            ));
        };
        match direction {
            ProtocolDirection::Upstream => {
                bindings
                    .call_upstream_encode(store, original_input, &document_json)
                    .await
            }
            ProtocolDirection::Downstream => {
                bindings
                    .call_downstream_encode(store, original_input, &document_json)
                    .await
            }
        }
        .map_err(|error| runtime_trap(&error))?
        .map_err(|error| runtime_guest_error(&error.code, &error.message))
    }

    /// Calls the manifest-selected display export.
    pub async fn display(
        &mut self,
        direction: ProtocolDirection,
        document: &Document,
    ) -> Result<String, DomainError> {
        let document_json = document_json(document)?;
        match &mut self.instance {
            PackageInstance::Http { store, bindings } => match direction {
                ProtocolDirection::Upstream => {
                    bindings.call_upstream_display(store, &document_json).await
                }
                ProtocolDirection::Downstream => {
                    bindings
                        .call_downstream_display(store, &document_json)
                        .await
                }
            }
            .map_err(|error| runtime_trap(&error))?
            .map_err(|error| runtime_guest_error(&error.code, &error.message)),
            PackageInstance::Socket { store, bindings } => match direction {
                ProtocolDirection::Upstream => {
                    bindings.call_upstream_display(store, &document_json).await
                }
                ProtocolDirection::Downstream => {
                    bindings
                        .call_downstream_display(store, &document_json)
                        .await
                }
            }
            .map_err(|error| runtime_trap(&error))?
            .map_err(|error| runtime_guest_error(&error.code, &error.message)),
        }
    }
}

fn parse_document_json(document_json: &str) -> Result<Document, DomainError> {
    serde_json::from_str(document_json).map_err(|error| {
        DomainError::new(
            ErrorCode::BodyDecodeFailed,
            format!("Wasm package returned invalid Document JSON: {error}"),
        )
    })
}

fn document_json(document: &Document) -> Result<String, DomainError> {
    serde_json::to_string(document).map_err(|error| {
        DomainError::new(
            ErrorCode::BodyEncodeFailed,
            format!("cannot encode Document JSON for Wasm package: {error}"),
        )
    })
}

fn runtime_trap(error: &wasmtime::Error) -> DomainError {
    DomainError::new(
        ErrorCode::InternalError,
        format!("Wasm package trapped: {error}"),
    )
}

fn runtime_guest_error(code: &str, message: &str) -> DomainError {
    let code = ErrorCode::ALL
        .iter()
        .copied()
        .find(|known| known.as_str() == code)
        .unwrap_or(ErrorCode::InternalError);
    DomainError::new(code, message)
}

/// Validates a single Component binary and reads its unique embedded Manifest.
pub fn read_package_component(bytes: &[u8]) -> Result<PackageComponent, DomainError> {
    let mut encoding = None;
    let mut manifest_bytes = None;
    let mut depth = 0_usize;
    let mut saw_root_version = false;
    for payload in Parser::new(0).parse_all(bytes) {
        match payload.map_err(|error| invalid_component(error.to_string()))? {
            Payload::Version {
                encoding: value, ..
            } if !saw_root_version => {
                saw_root_version = true;
                encoding = Some(value);
            }
            Payload::Version { .. } => depth = depth.saturating_add(1),
            Payload::End(_) if depth > 0 => depth -= 1,
            Payload::CustomSection(section)
                if depth == 0 && section.name() == PACKAGE_MANIFEST_SECTION =>
            {
                if manifest_bytes.replace(section.data()).is_some() {
                    return Err(invalid_component(
                        "WebAssembly Component contains duplicate package Manifest sections",
                    ));
                }
            }
            Payload::CustomSection(section) if section.name() == PACKAGE_MANIFEST_SECTION => {
                return Err(invalid_component(
                    "package Manifest section must be top-level in the WebAssembly Component",
                ));
            }
            _ => {}
        }
    }
    if encoding != Some(Encoding::Component) {
        return Err(invalid_component(
            "protocol package must be a WebAssembly Component",
        ));
    }
    Validator::new_with_features(WasmFeatures::default() | WasmFeatures::COMPONENT_MODEL)
        .validate_all(bytes)
        .map_err(|error| invalid_component(error.to_string()))?;
    let manifest = serde_json::from_slice::<PackageManifest>(manifest_bytes.ok_or_else(|| {
        invalid_component("WebAssembly Component is missing its package Manifest section")
    })?)
    .map_err(|error| invalid_component(format!("package Manifest is invalid: {error}")))?;
    Ok(PackageComponent {
        manifest,
        bytes: bytes.to_vec(),
    })
}

/// Adds the strict package Manifest as a top-level Component custom section.
///
/// Language toolchains produce the executable Component first. This packaging step owns the
/// top-level metadata boundary; guest core modules must not be searched for a nested substitute.
pub fn embed_package_manifest(
    component_bytes: &[u8],
    manifest_bytes: &[u8],
) -> Result<Vec<u8>, DomainError> {
    serde_json::from_slice::<PackageManifest>(manifest_bytes)
        .map_err(|error| invalid_component(format!("package Manifest is invalid: {error}")))?;
    let mut output = component_bytes.to_vec();
    CustomSection {
        name: Cow::Borrowed(PACKAGE_MANIFEST_SECTION),
        data: Cow::Borrowed(manifest_bytes),
    }
    .append_to_component(&mut output);
    read_package_component(&output)?;
    Ok(output)
}
