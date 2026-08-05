//! 可替换的 TCP、HTTP/1.1 与 pipeline 传输实现。
//!
//! Hyper 负责语义解析，旁路 I/O wrapper 保留线上原始 head 字节；每条连接由监听任务拥有，
//! 并受容量许可和取消令牌约束。读写、TLS、上游连接与故障注入各自有明确失败阶段，stop
//! 会取消连接树并等待子任务，不能让旧 epoch 的任务继续写入。

use std::fmt::{Debug, Formatter};
use std::future::{Future, poll_fn};
use std::io;
use std::net::SocketAddr;
use std::pin::Pin;
use std::sync::Arc;
use std::sync::Mutex as StdMutex;
use std::sync::atomic::{AtomicBool, Ordering};
use std::task::{Context, Poll};
use std::time::{Duration, SystemTime};

use async_trait::async_trait;
use bytes::Bytes;
use http::{HeaderMap, Method, Request, Response, StatusCode, Uri};
use http_body_util::BodyExt;
use hyper::body::{Body, Frame, Incoming, SizeHint};
use hyper::client::conn::http1 as client_http1;
use hyper::server::conn::http1 as server_http1;
use hyper::service::service_fn;
use hyper_util::rt::TokioIo;
use tokio::io::{AsyncRead, AsyncWrite, AsyncWriteExt, ReadBuf, ReadHalf, WriteHalf};
use tokio::net::{TcpListener, TcpStream};
use tokio::sync::{Notify, OwnedSemaphorePermit, Semaphore, TryAcquireError, mpsc};
use tokio::task::{JoinHandle, JoinSet};
use tokio_util::sync::CancellationToken;
use uuid::Uuid;

use crate::fault::{self, FaultAction, ResponseDisposition};
use crate::message::{Message, MessageLimits, RawHeader};
use crate::supervisor::ChannelId;
use crate::tls::ClientTlsAdapter;
use crate::traffic::{
    IntermittentProfile, JitterProfile, PacedBody, PacedBodyError, ThrottleProfile,
    TrafficDirection, TrafficSchedule,
};
use crate::{ErrorCode, ProxyError, Result};

mod contracts;
mod exchange;
mod helpers;
mod listener;
mod raw_http1;
mod raw_http1_response;
mod request_service;
mod schedule;
mod service;
#[path = "transport/io.rs"]
mod stream_io;
mod tracking;
mod upstream;
mod wire;

pub use contracts::{
    ConnectionContext, ForwardRequest, HandshakePolicy, NoopPipelinePorts, PipelinePorts,
    UpstreamConnector, UpstreamExchange, UpstreamSecurityEvidence, UpstreamTransportSecurity,
};
pub use listener::{
    AcceptedConnection, BoundListener, Clock, ConnectionAcceptor, ListenerBinder, SystemClock,
    TlsPeerIdentity, TokioListenerBinder,
};
pub(crate) use schedule::traffic_schedule;
pub use service::{ConnectionAdmission, ConnectionService};
pub use stream_io::{BoxIo, InformationalResponseSink, IoStream};
pub use upstream::HyperUpstreamConnector;

use exchange::{Http1ExchangeConfig, send_http1_request, send_scheduled_upstream_abort};
use helpers::{
    InjectedTimeoutStage, collect_limited, finish_downstream_write, informational_status,
    injected_timeout, message_wire_head, raw_head_capture_limit, response_from_disposition,
    timeout_stage, validate_headers, wait_for_injected_timeout,
};
use listener::TokioBoundListener;
use raw_http1::{RawHttp1HeadCapture, ReadRecordingIo, RequestHeadPreservingIo};
use raw_http1_response::ResponseHeadPreservingIo;
use service::RequestWireState;
use stream_io::SplitIo;
use tracking::{
    ConnectionTask, RequestWriteTracker, ResponseWriteTracker, TrackedIo, TrackedRequestBody,
};
use wire::{IntentionalWireFault, WireBody};

#[cfg(test)]
mod tests;
