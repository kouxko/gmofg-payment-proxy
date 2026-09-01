//! In-process Wasmtime implementation of the transport-neutral package runtime.

use async_trait::async_trait;
use intercept_proxy_domain::{Document, ProtocolDirection};
use intercept_proxy_package_contract::FrameResult;
use intercept_proxy_package_runtime::WasmPackageRuntime;
use parking_lot::Mutex;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use tokio_util::sync::CancellationToken;

use super::{PackageTransportError, ProtocolPackageRuntime};

/// Serializes every hook call through the `Store` owned by one exact package version.
pub(crate) struct InProcessWasmRuntime {
    runtime: Mutex<Option<Arc<RuntimeGeneration>>>,
    active: AtomicBool,
}

struct RuntimeGeneration {
    runtime: tokio::sync::Mutex<WasmPackageRuntime>,
    cancelled: CancellationToken,
}

impl RuntimeGeneration {
    fn new(runtime: WasmPackageRuntime) -> Self {
        Self {
            runtime: tokio::sync::Mutex::new(runtime),
            cancelled: CancellationToken::new(),
        }
    }
}

impl InProcessWasmRuntime {
    pub(crate) fn new(runtime: WasmPackageRuntime) -> Self {
        Self {
            runtime: Mutex::new(Some(Arc::new(RuntimeGeneration::new(runtime)))),
            active: AtomicBool::new(true),
        }
    }

    pub(crate) fn replace(&self, runtime: WasmPackageRuntime) {
        if let Some(previous) = self
            .runtime
            .lock()
            .replace(Arc::new(RuntimeGeneration::new(runtime)))
        {
            previous.cancelled.cancel();
        }
        self.active.store(true, Ordering::Release);
    }

    pub(crate) fn deactivate(&self) -> bool {
        let previous = self.runtime.lock().take();
        let removed = previous.is_some();
        if let Some(previous) = previous {
            previous.cancelled.cancel();
        }
        self.active.store(false, Ordering::Release);
        removed
    }

    pub(crate) fn is_active(&self) -> bool {
        self.active.load(Ordering::Acquire)
    }
}

impl std::fmt::Debug for InProcessWasmRuntime {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("InProcessWasmRuntime")
            .finish_non_exhaustive()
    }
}

fn package_error(error: intercept_proxy_domain::DomainError) -> PackageTransportError {
    PackageTransportError::Package { error }
}

fn runtime_offline() -> PackageTransportError {
    PackageTransportError::Package {
        error: intercept_proxy_domain::DomainError::new(
            intercept_proxy_domain::ErrorCode::InternalError,
            "local WebAssembly protocol package runtime is offline",
        ),
    }
}

#[async_trait]
impl ProtocolPackageRuntime for InProcessWasmRuntime {
    async fn frame(
        &self,
        direction: ProtocolDirection,
        buffer: Vec<u8>,
    ) -> Result<FrameResult, PackageTransportError> {
        let runtime = self.runtime.lock().clone().ok_or_else(runtime_offline)?;
        tokio::select! {
            biased;
            () = runtime.cancelled.cancelled() => Err(runtime_offline()),
            result = async {
                runtime.runtime.lock().await.frame(direction, &buffer).await
            } => result.map_err(package_error),
        }
    }

    async fn decode_http(
        &self,
        direction: ProtocolDirection,
        input: String,
    ) -> Result<Document, PackageTransportError> {
        let runtime = self.runtime.lock().clone().ok_or_else(runtime_offline)?;
        tokio::select! {
            biased;
            () = runtime.cancelled.cancelled() => Err(runtime_offline()),
            result = async {
                runtime.runtime.lock().await.decode_http(direction, &input).await
            } => result.map_err(package_error),
        }
    }

    async fn encode_http(
        &self,
        direction: ProtocolDirection,
        original_input: String,
        document: Document,
    ) -> Result<String, PackageTransportError> {
        let runtime = self.runtime.lock().clone().ok_or_else(runtime_offline)?;
        tokio::select! {
            biased;
            () = runtime.cancelled.cancelled() => Err(runtime_offline()),
            result = async {
                runtime.runtime.lock().await.encode_http(direction, &original_input, &document).await
            } => result.map_err(package_error),
        }
    }

    async fn decode_socket(
        &self,
        direction: ProtocolDirection,
        input: Vec<u8>,
    ) -> Result<Document, PackageTransportError> {
        let runtime = self.runtime.lock().clone().ok_or_else(runtime_offline)?;
        tokio::select! {
            biased;
            () = runtime.cancelled.cancelled() => Err(runtime_offline()),
            result = async {
                runtime.runtime.lock().await.decode_socket(direction, &input).await
            } => result.map_err(package_error),
        }
    }

    async fn encode_socket(
        &self,
        direction: ProtocolDirection,
        original_input: Vec<u8>,
        document: Document,
    ) -> Result<Vec<u8>, PackageTransportError> {
        let runtime = self.runtime.lock().clone().ok_or_else(runtime_offline)?;
        tokio::select! {
            biased;
            () = runtime.cancelled.cancelled() => Err(runtime_offline()),
            result = async {
                runtime.runtime.lock().await.encode_socket(direction, &original_input, &document).await
            } => result.map_err(package_error),
        }
    }

    async fn display(
        &self,
        direction: ProtocolDirection,
        document: Document,
    ) -> Result<String, PackageTransportError> {
        let runtime = self.runtime.lock().clone().ok_or_else(runtime_offline)?;
        tokio::select! {
            biased;
            () = runtime.cancelled.cancelled() => Err(runtime_offline()),
            result = async {
                runtime.runtime.lock().await.display(direction, &document).await
            } => result.map_err(package_error),
        }
    }
}

#[cfg(test)]
mod tests {
    use std::{path::PathBuf, process::Command, sync::Arc, time::Duration};

    use futures_util::StreamExt;
    use intercept_proxy_package_runtime::{embed_package_manifest, read_package_component};

    use super::*;

    fn blocking_websocket_component() -> Vec<u8> {
        let repository = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../..");
        let fixture = repository.join("src-tauri/crates/package-runtime/tests/fixtures/http-echo");
        let output = Command::new(std::env::var_os("CARGO").unwrap_or_else(|| "cargo".into()))
            .args([
                "build",
                "--locked",
                "--manifest-path",
                fixture.join("Cargo.toml").to_str().unwrap(),
                "--target",
                "wasm32-wasip2",
            ])
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        let component = std::fs::read(
            fixture.join("target/wasm32-wasip2/debug/intercept_proxy_http_echo_component.wasm"),
        )
        .unwrap();
        let manifest = std::fs::read(fixture.join("manifest.json")).unwrap();
        embed_package_manifest(&component, &manifest).unwrap()
    }

    #[tokio::test]
    async fn deactivate_does_not_wait_for_a_guest_blocked_in_websocket_receive() {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let (blocked, entered) = tokio::sync::oneshot::channel();
        let peer = tokio::spawn(async move {
            let (stream, _) = listener.accept().await.unwrap();
            let mut websocket = tokio_tungstenite::accept_async(stream).await.unwrap();
            websocket.next().await.unwrap().unwrap();
            websocket.next().await.unwrap().unwrap();
            let _ = blocked.send(());
            std::future::pending::<()>().await;
        });

        let component = read_package_component(&blocking_websocket_component()).unwrap();
        let runtime = Arc::new(InProcessWasmRuntime::new(
            WasmPackageRuntime::load(&component).await.unwrap(),
        ));
        let call_runtime = Arc::clone(&runtime);
        let call = tokio::spawn(async move {
            call_runtime
                .decode_http(
                    ProtocolDirection::Upstream,
                    format!("websocket-roundtrip:ws://{address}"),
                )
                .await
        });
        tokio::time::timeout(Duration::from_secs(5), entered)
            .await
            .expect("guest entered WebSocket receive")
            .unwrap();

        assert!(
            tokio::time::timeout(Duration::from_millis(100), async { runtime.deactivate() })
                .await
                .expect("deactivate must not wait for the guest")
        );
        assert!(!runtime.is_active());
        assert!(
            runtime
                .decode_http(ProtocolDirection::Upstream, "{}".into())
                .await
                .is_err()
        );
        assert!(
            tokio::time::timeout(Duration::from_millis(100), call)
                .await
                .expect("blocked guest call must be cancelled")
                .unwrap()
                .is_err()
        );
        peer.abort();
    }
}
