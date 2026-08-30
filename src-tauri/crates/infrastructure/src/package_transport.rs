//! Canonical package-initiated WebSocket JSON-RPC transport.

use std::{fmt, time::Duration};

use intercept_proxy_domain::{Document, DomainError};
use intercept_proxy_package_contract::{
    DecodeParams, DisplayParams, EncodeParams, FrameParams, FrameResult, JsonRpcVersion,
    PackageManifest, PackageRpcError, PackageRpcRequest,
};
use serde::de::DeserializeOwned;
use serde_json::Value;
use tokio::{
    io::{AsyncRead, AsyncWrite},
    sync::{mpsc, oneshot, watch},
};
use tokio_tungstenite::WebSocketStream;

mod driver;

/// Resource facts for one package WebSocket connection.
#[derive(Clone, Debug)]
pub struct PackageTransportConfig {
    registration_timeout: Duration,
    heartbeat_interval: Duration,
    heartbeat_timeout: Duration,
    max_logical_frame_bytes: usize,
    max_rpc_message_bytes: usize,
    max_registration_message_bytes: usize,
    max_display_message_bytes: usize,
}

impl PackageTransportConfig {
    /// Creates transport-only connection limits. Hook execution has no timeout or queue policy.
    pub fn new(
        registration_timeout: Duration,
        heartbeat_interval: Duration,
        heartbeat_timeout: Duration,
        max_logical_frame_bytes: usize,
        max_rpc_message_bytes: usize,
        max_registration_message_bytes: usize,
        max_display_message_bytes: usize,
    ) -> Self {
        assert!(!registration_timeout.is_zero());
        assert!(!heartbeat_interval.is_zero());
        assert!(heartbeat_timeout >= heartbeat_interval);
        assert!(max_logical_frame_bytes > 0);
        assert!(max_rpc_message_bytes > 0);
        assert!(max_registration_message_bytes > 0);
        assert!(max_display_message_bytes > 0);
        Self {
            registration_timeout,
            heartbeat_interval,
            heartbeat_timeout,
            max_logical_frame_bytes,
            max_rpc_message_bytes,
            max_registration_message_bytes,
            max_display_message_bytes,
        }
    }

    pub(crate) const fn websocket_message_bytes(&self) -> usize {
        let registration_or_rpc =
            if self.max_registration_message_bytes > self.max_rpc_message_bytes {
                self.max_registration_message_bytes
            } else {
                self.max_rpc_message_bytes
            };
        if registration_or_rpc > self.max_display_message_bytes {
            registration_or_rpc
        } else {
            self.max_display_message_bytes
        }
    }
}

/// Canonical package transport failure without Hook timeout, Busy, retry, or recovery semantics.
#[derive(Clone)]
pub enum PackageTransportError {
    /// Registration did not arrive before the connection deadline.
    RegistrationDeadline,
    /// WebSocket disconnected or its actor stopped.
    Disconnected,
    /// A wire message exceeded its transport byte limit.
    MessageTooLarge {
        /// Observed UTF-8 bytes.
        actual_bytes: usize,
        /// Allowed UTF-8 bytes.
        limit_bytes: usize,
    },
    /// Shared package contract validation failed.
    Package {
        /// Stable Domain error.
        error: DomainError,
    },
    /// Package returned a strict JSON-RPC error response.
    Remote {
        /// Request ID.
        request_id: String,
        /// Fixed method.
        method: &'static str,
        /// Strict error object with stable data code.
        error: PackageRpcError,
    },
    /// Response envelope, ID, or typed result was invalid.
    InvalidResponse,
    /// WebSocket transport failed.
    Transport(String),
}

impl fmt::Debug for PackageTransportError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::RegistrationDeadline => f.write_str("RegistrationDeadline"),
            Self::Disconnected => f.write_str("Disconnected"),
            Self::MessageTooLarge {
                actual_bytes,
                limit_bytes,
            } => f
                .debug_struct("MessageTooLarge")
                .field("actual_bytes", actual_bytes)
                .field("limit_bytes", limit_bytes)
                .finish(),
            Self::Package { error } => f
                .debug_struct("Package")
                .field("code", &error.code)
                .finish(),
            Self::Remote {
                request_id,
                method,
                error,
            } => f
                .debug_struct("Remote")
                .field("request_id", request_id)
                .field("method", method)
                .field("stable_code", &error.data().code())
                .finish(),
            Self::InvalidResponse => f.write_str("InvalidResponse"),
            Self::Transport(_) => f.write_str("Transport(<redacted>)"),
        }
    }
}

impl fmt::Display for PackageTransportError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{self:?}")
    }
}
impl std::error::Error for PackageTransportError {}

pub(super) struct Call {
    request: PackageRpcRequest,
    request_id: String,
    method: &'static str,
    response_limit: usize,
    response: oneshot::Sender<Result<Value, PackageTransportError>>,
}

/// Cloneable handle for the fixed API 1 package methods.
#[derive(Clone)]
pub struct PackageTransportClient {
    generation: u64,
    sequence: std::sync::Arc<std::sync::atomic::AtomicU64>,
    calls: mpsc::UnboundedSender<Call>,
    close: mpsc::Sender<()>,
    closed: watch::Receiver<Option<PackageTransportError>>,
    config: PackageTransportConfig,
}

impl fmt::Debug for PackageTransportClient {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("PackageTransportClient")
            .field("generation", &self.generation)
            .finish_non_exhaustive()
    }
}

impl PackageTransportClient {
    /// Returns the Socket logical frame byte ceiling inherited from the wire budget.
    #[must_use]
    pub const fn max_logical_frame_bytes(&self) -> usize {
        self.config.max_logical_frame_bytes
    }
    /// Starts one connection and waits for the package's id-less registration notification.
    pub async fn connect<S>(
        mut websocket: WebSocketStream<S>,
        generation: u64,
        config: PackageTransportConfig,
    ) -> Result<(PackageManifest, Self), PackageTransportError>
    where
        S: AsyncRead + AsyncWrite + Unpin + Send + 'static,
    {
        let (calls, call_rx) = mpsc::unbounded_channel();
        let (close, close_rx) = mpsc::channel(1);
        let (closed_tx, closed) = watch::channel(None);
        let manifest = driver::receive_registration(&mut websocket, &config).await?;
        tokio::spawn(driver::run_registered(
            websocket,
            config.clone(),
            call_rx,
            close_rx,
            closed_tx,
        ));
        Ok((
            manifest,
            Self {
                generation,
                sequence: std::sync::Arc::new(std::sync::atomic::AtomicU64::new(1)),
                calls,
                close,
                closed,
                config,
            },
        ))
    }

    fn next_id(&self) -> String {
        let sequence = self
            .sequence
            .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        format!("g{}-c{sequence}", self.generation)
    }

    async fn request<R>(
        &self,
        request: PackageRpcRequest,
        request_id: String,
        method: &'static str,
        display: bool,
    ) -> Result<R, PackageTransportError>
    where
        R: DeserializeOwned,
    {
        let (response, receiver) = oneshot::channel();
        self.calls
            .send(Call {
                request,
                request_id,
                method,
                response_limit: if display {
                    self.config.max_display_message_bytes
                } else {
                    self.config.max_rpc_message_bytes
                },
                response,
            })
            .map_err(|_| PackageTransportError::Disconnected)?;
        let value = receiver
            .await
            .map_err(|_| PackageTransportError::Disconnected)??;
        serde_json::from_value(value).map_err(|_| PackageTransportError::InvalidResponse)
    }

    /// Calls `hooks.upstream.frame` and validates consumed bytes against the sent buffer.
    pub async fn upstream_frame(
        &self,
        params: FrameParams,
    ) -> Result<FrameResult, PackageTransportError> {
        self.frame(params, true).await
    }
    /// Calls `hooks.downstream.frame` and validates consumed bytes against the sent buffer.
    pub async fn downstream_frame(
        &self,
        params: FrameParams,
    ) -> Result<FrameResult, PackageTransportError> {
        self.frame(params, false).await
    }
    async fn frame(
        &self,
        params: FrameParams,
        upstream: bool,
    ) -> Result<FrameResult, PackageTransportError> {
        let buffer_len = params.buffer.bytes().len();
        if buffer_len > self.config.max_logical_frame_bytes {
            return Err(PackageTransportError::MessageTooLarge {
                actual_bytes: buffer_len,
                limit_bytes: self.config.max_logical_frame_bytes,
            });
        }
        let id = self.next_id();
        let (request, method) = if upstream {
            (
                PackageRpcRequest::UpstreamFrame {
                    jsonrpc: JsonRpcVersion::V2,
                    id: id.clone(),
                    params,
                },
                "hooks.upstream.frame",
            )
        } else {
            (
                PackageRpcRequest::DownstreamFrame {
                    jsonrpc: JsonRpcVersion::V2,
                    id: id.clone(),
                    params,
                },
                "hooks.downstream.frame",
            )
        };
        let result: FrameResult = self.request(request, id, method, false).await?;
        result
            .validate_against_buffer_len(buffer_len)
            .map_err(|error| PackageTransportError::Package { error })?;
        Ok(result)
    }

    /// Calls `hooks.upstream.decode`.
    pub async fn upstream_decode(
        &self,
        params: DecodeParams,
    ) -> Result<Document, PackageTransportError> {
        let id = self.next_id();
        self.request(
            PackageRpcRequest::UpstreamDecode {
                jsonrpc: JsonRpcVersion::V2,
                id: id.clone(),
                params,
            },
            id,
            "hooks.upstream.decode",
            false,
        )
        .await
    }
    /// Calls `hooks.downstream.decode`.
    pub async fn downstream_decode(
        &self,
        params: DecodeParams,
    ) -> Result<Document, PackageTransportError> {
        let id = self.next_id();
        self.request(
            PackageRpcRequest::DownstreamDecode {
                jsonrpc: JsonRpcVersion::V2,
                id: id.clone(),
                params,
            },
            id,
            "hooks.downstream.decode",
            false,
        )
        .await
    }
    /// Calls `hooks.upstream.encode`.
    pub async fn upstream_encode(
        &self,
        params: EncodeParams,
    ) -> Result<String, PackageTransportError> {
        let id = self.next_id();
        self.request(
            PackageRpcRequest::UpstreamEncode {
                jsonrpc: JsonRpcVersion::V2,
                id: id.clone(),
                params,
            },
            id,
            "hooks.upstream.encode",
            false,
        )
        .await
    }
    /// Calls `hooks.downstream.encode`.
    pub async fn downstream_encode(
        &self,
        params: EncodeParams,
    ) -> Result<String, PackageTransportError> {
        let id = self.next_id();
        self.request(
            PackageRpcRequest::DownstreamEncode {
                jsonrpc: JsonRpcVersion::V2,
                id: id.clone(),
                params,
            },
            id,
            "hooks.downstream.encode",
            false,
        )
        .await
    }
    /// Calls `document.upstream.display`.
    pub async fn upstream_display(
        &self,
        params: DisplayParams,
    ) -> Result<String, PackageTransportError> {
        let id = self.next_id();
        self.request(
            PackageRpcRequest::UpstreamDisplay {
                jsonrpc: JsonRpcVersion::V2,
                id: id.clone(),
                params,
            },
            id,
            "document.upstream.display",
            true,
        )
        .await
    }
    /// Calls `document.downstream.display`.
    pub async fn downstream_display(
        &self,
        params: DisplayParams,
    ) -> Result<String, PackageTransportError> {
        let id = self.next_id();
        self.request(
            PackageRpcRequest::DownstreamDisplay {
                jsonrpc: JsonRpcVersion::V2,
                id: id.clone(),
                params,
            },
            id,
            "document.downstream.display",
            true,
        )
        .await
    }

    /// Requests transport shutdown. Pending calls fail and are never replayed.
    pub async fn disconnect(&self) {
        let _ = self.close.send(()).await;
    }
    /// Waits for the terminal transport fact.
    pub async fn wait_closed(&mut self) -> PackageTransportError {
        loop {
            if let Some(error) = self.closed.borrow().clone() {
                return error;
            }
            if self.closed.changed().await.is_err() {
                return PackageTransportError::Disconnected;
            }
        }
    }
}
