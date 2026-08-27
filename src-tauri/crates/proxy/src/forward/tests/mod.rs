//! 正向代理测试共享夹具。
//!
//! 各职责文件通过 `include!` 进入同一个私有测试模块，因此仍可直接访问生产模块的私有边界，
//! 同时不会为了测试拆分而扩大任何运行时 API 的可见性。

use super::*;
use bytes::Bytes;
use http::HeaderMap;
use http::header::{CONNECTION, UPGRADE};
use http_body_util::BodyExt;
use std::sync::Mutex as StdMutex;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpListener;
use tokio::sync::oneshot;

use super::pipeline::response_from_pipeline_disposition;
use crate::fault::{FaultAction, ResponseDisposition};
use crate::message::Message;
use crate::message::RawHeader;
use crate::traffic::TrafficSchedule;
use crate::transport::HandshakePolicy;

fn loopback_config() -> ForwardProxyConfig {
    ForwardProxyConfig {
        bind_addr: "127.0.0.1:8080".parse().unwrap(),
        authentication: ForwardAuthenticationMode::None,
        connect_timeout: Duration::from_secs(1),
        read_timeout: Duration::from_secs(1),
        write_timeout: Duration::from_secs(1),
    }
}

#[derive(Debug, Default)]
struct CapturingPipelinePorts {
    requests: StdMutex<Vec<Message>>,
    responses: StdMutex<Vec<Message>>,
    mutate: bool,
    request_actions: Vec<FaultAction>,
}

impl HandshakePolicy for CapturingPipelinePorts {}

#[async_trait::async_trait]
impl PipelinePorts for CapturingPipelinePorts {
    async fn apply_request_policy(
        &self,
        _context: &ConnectionContext,
        message: &mut Message,
    ) -> Result<Vec<FaultAction>> {
        self.requests.lock().unwrap().push(message.clone());
        if self.mutate {
            message.headers.push(RawHeader::new("X-Rule", "applied"));
            message.replace_body(Bytes::from_static(b"rule-request"));
        }
        Ok(self.request_actions.clone())
    }

    async fn apply_response_policy(
        &self,
        _context: &ConnectionContext,
        message: &mut Message,
    ) -> Result<Vec<FaultAction>> {
        self.responses.lock().unwrap().push(message.clone());
        if self.mutate {
            message.replace_body(Bytes::from_static(b"rule-response"));
            return Ok(vec![FaultAction::CustomStatus(StatusCode::CREATED)]);
        }
        Ok(Vec::new())
    }
}

fn plain_capabilities(listener_id: &str) -> Arc<dyn crate::http::HttpProtocolCapabilityFactory> {
    Arc::new(crate::http::PlainHttpCapabilityFactory::new(
        "forward-test-workspace",
        listener_id,
    ))
}

#[derive(Debug)]
struct CountingHttpCapabilities {
    inner: crate::http::PlainHttpCapabilityFactory,
    upstream: std::sync::atomic::AtomicUsize,
    downstream: std::sync::atomic::AtomicUsize,
}

impl CountingHttpCapabilities {
    fn new(listener_id: &str) -> Self {
        Self {
            inner: crate::http::PlainHttpCapabilityFactory::new(
                "forward-test-workspace",
                listener_id,
            ),
            upstream: std::sync::atomic::AtomicUsize::new(0),
            downstream: std::sync::atomic::AtomicUsize::new(0),
        }
    }
}

impl crate::http::HttpProtocolCapabilityFactory for CountingHttpCapabilities {
    fn observation_metadata(&self) -> crate::http::HttpObservationMetadata {
        self.inner.observation_metadata()
    }

    fn create_upstream(
        &self,
        connection: crate::http::HttpConnectionIdentity,
    ) -> std::result::Result<
        crate::http::HttpDirectionCapabilities<intercept_proxy_exchange::Upstream>,
        intercept_proxy_exchange::Error,
    > {
        self.upstream
            .fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        self.inner.create_upstream(connection)
    }

    fn create_downstream(
        &self,
        connection: crate::http::HttpConnectionIdentity,
    ) -> std::result::Result<
        crate::http::HttpDirectionCapabilities<intercept_proxy_exchange::Downstream>,
        intercept_proxy_exchange::Error,
    > {
        self.downstream
            .fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        self.inner.create_downstream(connection)
    }
}

#[derive(Debug)]
struct PanickingHttpCapabilities;

impl crate::http::HttpProtocolCapabilityFactory for PanickingHttpCapabilities {
    fn observation_metadata(&self) -> crate::http::HttpObservationMetadata {
        crate::http::HttpObservationMetadata {
            workspace_id: "forward-test-workspace".into(),
            listener_id: "panic-capability".into(),
        }
    }

    fn create_upstream(
        &self,
        _connection: crate::http::HttpConnectionIdentity,
    ) -> std::result::Result<
        crate::http::HttpDirectionCapabilities<intercept_proxy_exchange::Upstream>,
        intercept_proxy_exchange::Error,
    > {
        panic!("test capability factory panic")
    }

    fn create_downstream(
        &self,
        _connection: crate::http::HttpConnectionIdentity,
    ) -> std::result::Result<
        crate::http::HttpDirectionCapabilities<intercept_proxy_exchange::Downstream>,
        intercept_proxy_exchange::Error,
    > {
        unreachable!("upstream capability construction fails first")
    }
}

async fn read_raw_http_request_body(stream: &mut TcpStream) -> Bytes {
    let mut request = Vec::new();
    let header_end = loop {
        if let Some(index) = request.windows(4).position(|part| part == b"\r\n\r\n") {
            break index + 4;
        }
        let mut buffer = [0_u8; 512];
        let read = stream.read(&mut buffer).await.unwrap();
        assert_ne!(read, 0);
        request.extend_from_slice(&buffer[..read]);
    };
    let headers = std::str::from_utf8(&request[..header_end]).unwrap();
    let length = headers
        .lines()
        .find_map(|line| {
            line.to_ascii_lowercase()
                .strip_prefix("content-length:")
                .map(str::trim)
                .map(str::parse::<usize>)
        })
        .transpose()
        .unwrap()
        .unwrap_or(0);
    while request.len() - header_end < length {
        let mut buffer = [0_u8; 512];
        let read = stream.read(&mut buffer).await.unwrap();
        assert_ne!(read, 0);
        request.extend_from_slice(&buffer[..read]);
    }
    Bytes::copy_from_slice(&request[header_end..header_end + length])
}

// 基础协议转换与管线语义。
include!("request_semantics.rs");
// 监听安全与 MITM 缓存配置。
include!("configuration.rs");
// 普通 HTTP 转发及 DropResponse 边界。
include!("plain_http.rs");
include!("connection_exchange.rs");
include!("capability_sequence.rs");
// CONNECT/Upgrade 在当前 Exchange 模型中严格拒绝。
include!("websocket.rs");
// Listener 取消和空闲连接回收。
include!("listener_lifecycle.rs");
