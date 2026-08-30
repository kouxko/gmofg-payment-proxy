//! Generic local package Sidecar executable.

use std::{env, fs::File, path::PathBuf, process::ExitCode};

use futures_util::{SinkExt, StreamExt};
use intercept_proxy_domain::ErrorCode;
use intercept_proxy_package_contract::{
    JsonRpcVersion, PackageRegisterNotification, PackageRpcError, PackageRpcFailure,
    PackageRpcRequest, PackageRpcSuccess,
};
use intercept_proxy_package_runtime::{
    LocalSidecarRuntime, PackageArchiveResourceLimits, read_package_zip,
};
use serde::Serialize;
use tokio_tungstenite::{connect_async, tungstenite::Message};

#[derive(Clone, Copy)]
struct ArchiveLimits;

impl PackageArchiveResourceLimits for ArchiveLimits {
    fn max_archive_bytes(&self) -> u64 {
        8 * 1024 * 1024
    }
    fn max_entries(&self) -> usize {
        64
    }
    fn max_file_bytes(&self) -> u64 {
        1024 * 1024
    }
    fn max_total_bytes(&self) -> u64 {
        4 * 1024 * 1024
    }
    fn max_compression_ratio(&self) -> u64 {
        100
    }
    fn max_path_depth(&self) -> usize {
        8
    }
}

struct LaunchArguments {
    archive: PathBuf,
    packages_url: String,
}

fn arguments() -> Result<LaunchArguments, String> {
    let mut values = env::args_os().skip(1);
    let mut archive = None;
    let mut packages_url = None;
    while let Some(flag) = values.next() {
        match flag.to_str() {
            Some("--archive") if archive.is_none() => {
                archive = Some(PathBuf::from(
                    values.next().ok_or("--archive requires a path")?,
                ));
            }
            Some("--packages-url") if packages_url.is_none() => {
                packages_url = Some(
                    values
                        .next()
                        .ok_or("--packages-url requires a URL")?
                        .into_string()
                        .map_err(|_| "--packages-url must be UTF-8")?,
                );
            }
            _ => return Err("expected exactly --archive <path> --packages-url <url>".to_owned()),
        }
    }
    Ok(LaunchArguments {
        archive: archive.ok_or("missing --archive")?,
        packages_url: packages_url.ok_or("missing --packages-url")?,
    })
}

fn success<R: Serialize>(id: String, result: R) -> Result<String, String> {
    serde_json::to_string(&PackageRpcSuccess {
        jsonrpc: JsonRpcVersion::V2,
        id,
        result,
    })
    .map_err(|error| error.to_string())
}

fn rpc_result<R: Serialize>(
    id: &str,
    result: Result<R, intercept_proxy_package_runtime::LocalSidecarError>,
) -> Result<String, String> {
    result
        .map_err(|error| error.to_string())
        .and_then(|value| success(id.to_owned(), value))
}

fn failure(id: String, error: impl std::fmt::Display) -> String {
    serde_json::to_string(&PackageRpcFailure {
        jsonrpc: JsonRpcVersion::V2,
        id,
        error: PackageRpcError::new(
            -32_000,
            error.to_string(),
            ErrorCode::ProtocolPackageInvalid,
        ),
    })
    .expect("fixed package RPC failure is serializable")
}

fn execute(runtime: &mut LocalSidecarRuntime, request: PackageRpcRequest) -> String {
    let (id, result) = match request {
        PackageRpcRequest::UpstreamFrame { id, params, .. } => {
            let result = rpc_result(&id, runtime.upstream_frame(params));
            (id, result)
        }
        PackageRpcRequest::DownstreamFrame { id, params, .. } => {
            let result = rpc_result(&id, runtime.downstream_frame(params));
            (id, result)
        }
        PackageRpcRequest::UpstreamDecode { id, params, .. } => {
            let result = rpc_result(&id, runtime.upstream_decode(params));
            (id, result)
        }
        PackageRpcRequest::DownstreamDecode { id, params, .. } => {
            let result = rpc_result(&id, runtime.downstream_decode(params));
            (id, result)
        }
        PackageRpcRequest::UpstreamEncode { id, params, .. } => {
            let result = rpc_result(&id, runtime.upstream_encode(params));
            (id, result)
        }
        PackageRpcRequest::DownstreamEncode { id, params, .. } => {
            let result = rpc_result(&id, runtime.downstream_encode(params));
            (id, result)
        }
        PackageRpcRequest::UpstreamDisplay { id, params, .. } => {
            let result = rpc_result(&id, runtime.upstream_display(params));
            (id, result)
        }
        PackageRpcRequest::DownstreamDisplay { id, params, .. } => {
            let result = rpc_result(&id, runtime.downstream_display(params));
            (id, result)
        }
    };
    result.unwrap_or_else(|error| failure(id, error))
}

async fn run() -> Result<(), String> {
    let arguments = arguments()?;
    let archive = read_package_zip(
        File::open(&arguments.archive).map_err(|error| error.to_string())?,
        &ArchiveLimits,
    )
    .map_err(|error| error.to_string())?;
    let registration = PackageRegisterNotification::new(archive.manifest().clone());
    let mut runtime = LocalSidecarRuntime::load(&archive).map_err(|error| error.to_string())?;
    let (mut websocket, _) = connect_async(&arguments.packages_url)
        .await
        .map_err(|error| error.to_string())?;
    websocket
        .send(Message::Text(
            serde_json::to_string(&registration)
                .map_err(|error| error.to_string())?
                .into(),
        ))
        .await
        .map_err(|error| error.to_string())?;
    while let Some(message) = websocket.next().await {
        match message.map_err(|error| error.to_string())? {
            Message::Text(text) => {
                let request = serde_json::from_str::<PackageRpcRequest>(&text)
                    .map_err(|error| error.to_string())?;
                websocket
                    .send(Message::Text(execute(&mut runtime, request).into()))
                    .await
                    .map_err(|error| error.to_string())?;
            }
            Message::Ping(bytes) => websocket
                .send(Message::Pong(bytes))
                .await
                .map_err(|error| error.to_string())?,
            Message::Close(_) => break,
            Message::Pong(_) => {}
            Message::Binary(_) | Message::Frame(_) => {
                return Err("unsupported WebSocket message".to_owned());
            }
        }
    }
    Ok(())
}

#[tokio::main]
async fn main() -> ExitCode {
    intercept_proxy_package_runtime::sidecar_executable_marker();
    match run().await {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("{error}");
            ExitCode::FAILURE
        }
    }
}
