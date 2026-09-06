use super::unified_working_exchange::http_rule;
use super::*;
use async_trait::async_trait;
use intercept_proxy_application::ListenerRuntimePort;
use intercept_proxy_domain::{
    FixedServerSettings, HttpHeader, HttpTopology, MatchContext, MessageStage, Revision,
    TerminalIdentity, UpstreamTlsSettings,
};
use intercept_proxy_runtime::{
    FaultAction, HandshakePolicy, HttpJointEvaluation, HttpRequestMetadata, ProxyError,
};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::{TcpListener, TcpStream},
};

const IO_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(2);

#[derive(Debug, Default)]
struct LiveHotPolicy {
    request_rule_ids: Mutex<Vec<Uuid>>,
}

impl HandshakePolicy for LiveHotPolicy {}

#[async_trait]
impl PipelinePorts for LiveHotPolicy {
    async fn apply_request_joint_policy(
        &self,
        connection: &ConnectionContext,
        request: &HttpRequestMetadata,
        message: &mut Message,
        mut evaluation: Box<dyn HttpJointEvaluation>,
    ) -> intercept_proxy_runtime::Result<Vec<FaultAction>> {
        let terminal = TerminalIdentity {
            source_ip: connection.peer_addr.ip().to_string(),
            certificate_sha256: String::new(),
        };
        let values = message
            .headers
            .iter()
            .map(|header| (header.name.clone(), header.value.clone()))
            .collect::<Vec<_>>();
        let headers = values
            .iter()
            .map(|(name, value)| HttpHeader::new(name, value))
            .collect::<Vec<_>>();
        let context = MatchContext {
            runtime_epoch: intercept_proxy_domain::RuntimeEpoch::from_uuid(
                connection.runtime_epoch,
            ),
            channel: intercept_proxy_domain::ChannelId::new(connection.channel.as_str()).unwrap(),
            stage: MessageStage::Request,
            terminal: &terminal,
            method: Some(&request.method),
            request_target: Some(&request.request_target),
            headers: &headers,
        };
        for rule_id in self.request_rule_ids.lock().iter().copied() {
            evaluation.gate(rule_id, &context, message, &mut |_, _| Ok(()))?;
        }
        evaluation.encode(message).await.map_err(|error| {
            let message = error.message.clone();
            ProxyError::from_exchange_error(
                intercept_proxy_runtime::ErrorCode::Internal,
                message,
                error,
            )
        })?;
        Ok(Vec::new())
    }
}

// Keep the complete multi-generation/direction scenario together so its state transitions are reviewable.
#[allow(clippy::too_many_lines)]
#[tokio::test]
async fn live_plain_keep_alive_connection_observes_document_rule_add_and_remove() {
    let origin = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let origin_address = origin.local_addr().unwrap();
    let origin_task = tokio::spawn(async move {
        let mut bodies = Vec::new();
        for index in 0..3 {
            let (mut stream, _) = tokio::time::timeout(IO_TIMEOUT, origin.accept())
                .await
                .expect("origin accept timed out")
                .unwrap();
            let body = read_body_timed(&mut stream).await;
            println!("origin exchange {index}: received={body:?}");
            bodies.push(body.clone());
            let close = if index == 2 {
                "Connection: close\r\n"
            } else {
                ""
            };
            stream
                .write_all(
                    format!(
                        "HTTP/1.1 200 OK\r\nContent-Length: {}\r\n{close}\r\n",
                        body.len()
                    )
                    .as_bytes(),
                )
                .await
                .unwrap();
            stream.write_all(&body).await.unwrap();
        }
        bodies
    });
    let reservation = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let bind_address = reservation.local_addr().unwrap();
    drop(reservation);
    let listener = ProxyListener {
        id: intercept_proxy_domain::ListenerId::new(),
        name: "plain hot replacement".into(),
        enabled: false,
        bind_address: bind_address.ip().to_string(),
        port: bind_address.port(),
        data_plane: ListenerDataPlane::Http(HttpListenerSettings {
            topology: HttpTopology::remote(Some(FixedServerSettings {
                upstream_url: format!("http://{origin_address}"),
                upstream_tls: UpstreamTlsSettings::default(),
            })),
            body_processing: HttpBodyProcessing::Plain,
            ..HttpListenerSettings::default()
        }),
        ..ProxyListener::default()
    };
    let initial = ProxyWorkspace {
        id: intercept_proxy_domain::WorkspaceId::new(),
        name: "plain hot replacement".into(),
        revision: Revision::INITIAL,
        listeners: vec![listener.clone()],
        rule_definitions: Vec::new(),
        rule_created_order_high_water: 0,
        certificate_references: Vec::new(),
        android_network_profiles: Vec::new(),
    };
    let store = Arc::new(SqliteStore::in_memory().unwrap());
    store
        .insert_workspace(&crate::adapters::WorkspaceRepositoryAdapter::record(&initial).unwrap())
        .unwrap();
    let workspaces = crate::adapters::WorkspaceRepositoryAdapter::new(Arc::clone(&store));
    let runtime = test_listener_runtime(store);
    let policy = Arc::new(LiveHotPolicy::default());
    runtime.set_pipeline_ports(policy.clone());
    runtime
        .start(initial.clone(), listener.clone())
        .await
        .unwrap();
    let mut client = TcpStream::connect(bind_address).await.unwrap();

    send_body(&mut client, b"plain-before", false).await;
    let first = read_body_timed(&mut client).await;
    println!(
        "client exchange 0: sent={:?} received={first:?}",
        b"plain-before"
    );
    assert_eq!(first, b"plain-before");
    let rule = http_rule(
        &listener,
        "live add",
        10,
        1,
        Condition::Document {
            path: JsonPointer::property("value"),
            predicate: DocumentPredicate::String(StringPredicate {
                operator: StringOperator::Equal,
                value: "old".into(),
            }),
        },
        UnifiedAction::Document(DocumentMutation::Set {
            path: JsonPointer::property("value").into(),
            value: DocumentValue::String("created".into()),
        }),
    );
    let mut added = initial.clone();
    added.rule_definitions = vec![rule];
    added.rule_created_order_high_water = 1;
    let added = runtime
        .replace_rule_definitions(&workspaces, added, listener.id)
        .await
        .unwrap();
    *policy.request_rule_ids.lock() = added
        .rule_definitions
        .iter()
        .map(|rule| rule.rule_id().as_uuid())
        .collect();
    send_body(&mut client, br#"{"value":"old"}"#, false).await;
    let second = read_body_timed(&mut client).await;
    println!(
        "client exchange 1: sent={:?} received={second:?}",
        br#"{"value":"old"}"#
    );
    assert_eq!(second, br#"{"value":"created"}"#);

    let mut removed = added.clone();
    removed.rule_definitions.clear();
    let _removed = runtime
        .replace_rule_definitions(&workspaces, removed, listener.id)
        .await
        .unwrap();
    policy.request_rule_ids.lock().clear();
    send_body(&mut client, b"plain-after", true).await;
    let third = read_body_timed(&mut client).await;
    println!(
        "client exchange 2: sent={:?} received={third:?}",
        b"plain-after"
    );
    assert_eq!(third, b"plain-after");
    assert_eq!(
        tokio::time::timeout(IO_TIMEOUT, origin_task)
            .await
            .expect("origin task join timed out")
            .unwrap(),
        vec![
            b"plain-before".to_vec(),
            br#"{"value":"created"}"#.to_vec(),
            b"plain-after".to_vec()
        ]
    );
    runtime.stop(listener.id).await.unwrap();
}

async fn send_body(stream: &mut TcpStream, body: &[u8], close: bool) {
    let close = if close { "Connection: close\r\n" } else { "" };
    stream
        .write_all(
            format!(
                "POST /hot HTTP/1.1\r\nHost: live.test\r\nContent-Length: {}\r\n{close}\r\n",
                body.len()
            )
            .as_bytes(),
        )
        .await
        .unwrap();
    stream.write_all(body).await.unwrap();
}

async fn read_body(stream: &mut TcpStream) -> Vec<u8> {
    let mut bytes = Vec::new();
    let mut buffer = [0_u8; 512];
    loop {
        let read = stream.read(&mut buffer).await.unwrap();
        assert!(read > 0, "HTTP message ended before its body");
        bytes.extend_from_slice(&buffer[..read]);
        let Some(end) = bytes.windows(4).position(|window| window == b"\r\n\r\n") else {
            continue;
        };
        let end = end + 4;
        let head = String::from_utf8_lossy(&bytes[..end]);
        let length: usize = head
            .lines()
            .find_map(|line| {
                line.to_ascii_lowercase()
                    .strip_prefix("content-length:")
                    .map(str::trim)
                    .map(str::to_owned)
            })
            .and_then(|value| value.parse().ok())
            .unwrap();
        if bytes.len() >= end + length {
            return bytes[end..end + length].to_vec();
        }
    }
}

async fn read_body_timed(stream: &mut TcpStream) -> Vec<u8> {
    tokio::time::timeout(IO_TIMEOUT, read_body(stream))
        .await
        .expect("HTTP body read timed out")
}
