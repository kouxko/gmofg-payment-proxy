use std::{
    net::SocketAddr,
    sync::{
        Arc,
        atomic::{AtomicBool, AtomicUsize, Ordering},
    },
};

use futures_util::{SinkExt, StreamExt};
use intercept_proxy_package_contract::{
    CanonicalBase64, DecodeParams, EncodeParams, FrameParams, FrameResult, PackageManifest,
    PackageRegisterNotification,
};
use parking_lot::Mutex;
use serde_json::{Value, json};
use tokio::{
    sync::{mpsc, oneshot},
    task::JoinHandle,
    time::timeout,
};
use tokio_tungstenite::{connect_async, tungstenite::Message};

use super::TEST_TIMEOUT;

pub(in super::super) struct TestExternalPeer {
    registrations: Arc<AtomicUsize>,
    encode_methods: Arc<Mutex<Vec<String>>>,
    invalid_boundary_once: Arc<AtomicBool>,
    fail_encode_once: Arc<AtomicBool>,
    need_more: tokio::sync::Mutex<mpsc::UnboundedReceiver<()>>,
    close: Option<oneshot::Sender<()>>,
    task: JoinHandle<()>,
}

impl TestExternalPeer {
    pub(super) fn spawn(address: SocketAddr, registration: PackageManifest) -> Self {
        let registrations = Arc::new(AtomicUsize::new(0));
        let encode_methods = Arc::new(Mutex::new(Vec::new()));
        let invalid_boundary_once = Arc::new(AtomicBool::new(false));
        let fail_encode_once = Arc::new(AtomicBool::new(false));
        let (need_more_tx, need_more_rx) = mpsc::unbounded_channel();
        let (close_tx, mut close_rx) = oneshot::channel();
        let task_registrations = Arc::clone(&registrations);
        let task_encode_methods = Arc::clone(&encode_methods);
        let task_invalid_boundary_once = Arc::clone(&invalid_boundary_once);
        let task_fail_encode_once = Arc::clone(&fail_encode_once);
        let task = tokio::spawn(async move {
            let (mut socket, _) = timeout(
                TEST_TIMEOUT,
                connect_async(format!("ws://{address}/packages")),
            )
            .await
            .expect("external peer connection deadline")
            .expect("external peer WebSocket connection");
            socket
                .send(Message::Text(
                    serde_json::to_string(&PackageRegisterNotification::new(registration))
                        .unwrap()
                        .into(),
                ))
                .await
                .unwrap();
            task_registrations.fetch_add(1, Ordering::AcqRel);
            loop {
                tokio::select! {
                    _ = &mut close_rx => {
                        socket.close(None).await.unwrap();
                        break;
                    }
                    incoming = socket.next() => {
                        let Some(incoming) = incoming else { break };
                        match incoming.unwrap() {
                            Message::Text(text) => respond(
                                &mut socket,
                                &task_invalid_boundary_once,
                                &task_fail_encode_once,
                                &task_encode_methods,
                                &need_more_tx,
                                &text,
                            ).await,
                            Message::Ping(payload) => socket.send(Message::Pong(payload)).await.unwrap(),
                            Message::Close(_) => break,
                            Message::Pong(_) => {}
                            other => panic!("unexpected WebSocket message: {other:?}"),
                        }
                    }
                }
            }
        });
        Self {
            registrations,
            encode_methods,
            invalid_boundary_once,
            fail_encode_once,
            need_more: tokio::sync::Mutex::new(need_more_rx),
            close: Some(close_tx),
            task,
        }
    }

    pub(in super::super) fn registration_count(&self) -> usize {
        self.registrations.load(Ordering::Acquire)
    }

    pub(in super::super) fn encode_methods(&self) -> Vec<String> {
        self.encode_methods.lock().clone()
    }

    pub(in super::super) fn return_oversized_frame_boundary_once(&self) {
        self.invalid_boundary_once.store(true, Ordering::Release);
    }

    pub(in super::super) fn fail_encode_once(&self) {
        self.fail_encode_once.store(true, Ordering::Release);
    }

    pub(in super::super) async fn wait_for_need_more(&self) {
        timeout(TEST_TIMEOUT, self.need_more.lock().await.recv())
            .await
            .expect("frame NeedMore observation deadline")
            .expect("frame NeedMore observation");
    }

    pub(super) async fn close(mut self) {
        if let Some(close) = self.close.take() {
            let _ = close.send(());
        }
        timeout(TEST_TIMEOUT, self.task)
            .await
            .expect("external peer close deadline")
            .expect("external peer task");
    }
}

async fn respond<S>(
    socket: &mut tokio_tungstenite::WebSocketStream<S>,
    invalid_boundary_once: &AtomicBool,
    fail_encode_once: &AtomicBool,
    encode_methods: &Mutex<Vec<String>>,
    need_more: &mpsc::UnboundedSender<()>,
    text: &str,
) where
    S: tokio::io::AsyncRead + tokio::io::AsyncWrite + Unpin,
{
    let request: Value = serde_json::from_str(text).unwrap();
    let method = request["method"].as_str().unwrap();
    let is_encode = matches!(method, "hooks.upstream.encode" | "hooks.downstream.encode");
    if is_encode {
        encode_methods.lock().push(method.to_owned());
    }
    if is_encode && fail_encode_once.swap(false, Ordering::AcqRel) {
        socket
            .send(Message::Text(
                json!({
                    "jsonrpc": "2.0",
                    "id": request["id"],
                    "error": {
                        "code": -32011,
                        "message": "phase11 integration encode rejected",
                        "data": {"code": "BODY_ENCODE_FAILED"}
                    }
                })
                .to_string()
                .into(),
            ))
            .await
            .unwrap();
        return;
    }
    let result = match method {
        "hooks.upstream.frame" | "hooks.downstream.frame" => {
            let frame: FrameParams = serde_json::from_value(request["params"].clone()).unwrap();
            let bytes = frame.buffer.bytes();
            let boundary = if invalid_boundary_once.swap(false, Ordering::AcqRel) {
                FrameResult::complete(bytes.len() + 1).unwrap()
            } else if bytes
                .first()
                .is_some_and(|length| bytes.len() >= usize::from(*length))
            {
                FrameResult::complete(usize::from(bytes[0])).unwrap()
            } else {
                let _ = need_more.send(());
                FrameResult::NeedMore {
                    required_bytes: bytes.first().copied().map(usize::from),
                }
            };
            serde_json::to_value(boundary).unwrap()
        }
        "hooks.upstream.decode" | "hooks.downstream.decode" => {
            let decoded: DecodeParams = serde_json::from_value(request["params"].clone()).unwrap();
            let buffer: CanonicalBase64 = decoded.input.try_into().unwrap();
            json!({"payload": buffer.bytes()})
        }
        "hooks.upstream.encode" | "hooks.downstream.encode" => {
            let encoded: EncodeParams = serde_json::from_value(request["params"].clone()).unwrap();
            let document = serde_json::to_value(encoded.document).unwrap();
            let bytes = document["payload"]
                .as_array()
                .and_then(|values| {
                    values
                        .iter()
                        .map(|value| {
                            let value = value.as_f64()?;
                            #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
                            (value.fract() == 0.0 && (0.0..=255.0).contains(&value))
                                .then_some(value as u8)
                        })
                        .collect::<Option<Vec<_>>>()
                })
                .unwrap_or_else(|| vec![3, b'O', b'K']);
            json!(CanonicalBase64::from_bytes(&bytes).as_str())
        }
        "document.upstream.display" | "document.downstream.display" => {
            json!("<p>external e2e</p>")
        }
        other => panic!("unexpected external method: {other}"),
    };
    socket
        .send(Message::Text(
            json!({"jsonrpc": "2.0", "id": request["id"], "result": result})
                .to_string()
                .into(),
        ))
        .await
        .unwrap();
}
