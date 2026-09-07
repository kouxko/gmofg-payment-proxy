use super::production_shape::{Phase10ActorRules, actor_pipeline};
use super::*;
use crate::adapters::listener_runtime::ListenerRuntimeAdapter;
use intercept_proxy_application::ListenerRuntimePort;
use intercept_proxy_domain::{HttpAction, HttpTopology, ListenerId, MatchField, MatchOperator};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::{TcpListener, TcpStream},
};

const IO_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(5);

struct LocalFixture {
    runtime: ListenerRuntimeAdapter,
    listener_id: ListenerId,
    address: SocketAddr,
    unused_upstream: TcpListener,
    rules: Arc<Phase10ActorRules>,
}

impl LocalFixture {
    async fn start(with_rules: bool) -> Self {
        let reserved = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = reserved.local_addr().unwrap();
        drop(reserved);
        let listener = ProxyListener {
            id: ListenerId::new(),
            name: "local response contract".into(),
            enabled: false,
            bind_address: address.ip().to_string(),
            port: address.port(),
            data_plane: ListenerDataPlane::Http(HttpListenerSettings {
                topology: HttpTopology::LocalServer,
                body_processing: HttpBodyProcessing::Plain,
                ..HttpListenerSettings::default()
            }),
            ..ProxyListener::default()
        };
        let definitions = if with_rules {
            response_rules(&listener)
        } else {
            Vec::new()
        };
        let workspace = workspace_with_http_rules(&listener, definitions);
        workspace.validate().unwrap();
        let store = Arc::new(SqliteStore::in_memory().unwrap());
        store
            .insert_workspace(
                &crate::adapters::WorkspaceRepositoryAdapter::record(&workspace).unwrap(),
            )
            .unwrap();
        let rules = Arc::new(Phase10ActorRules {
            snapshot: Mutex::new(RuleRuntimeSnapshot::with_collection_identity_and_order(
                Some(workspace.id.as_uuid()),
                workspace.revision.get(),
                workspace.rule_definitions.clone(),
                workspace.http_runtime_rule_execution_order(),
            )),
            commit_attempts: AtomicUsize::new(0),
        });
        let snapshot = prepared_plain_snapshot(&workspace, &listener).await;
        let pipeline = Arc::new(actor_pipeline(&snapshot, Arc::clone(&rules)));
        let runtime = test_listener_runtime(store);
        runtime.set_pipeline_ports(pipeline);
        runtime.start(workspace, listener.clone()).await.unwrap();
        Self {
            runtime,
            listener_id: listener.id,
            address,
            rules,
            unused_upstream: TcpListener::bind("127.0.0.1:0").await.unwrap(),
        }
    }

    async fn finish(self) {
        assert!(
            tokio::time::timeout(
                std::time::Duration::from_millis(50),
                self.unused_upstream.accept()
            )
            .await
            .is_err(),
            "Local mode must not connect to request Host"
        );
        self.runtime.stop(self.listener_id).await.unwrap();
    }

    async fn exchange(&self, stream: &mut TcpStream, path: &str, body: &[u8]) -> Message {
        tokio::time::timeout(IO_TIMEOUT, async {
            let mut request = format!(
                "POST {path} HTTP/1.1\r\nHost: {}\r\nContent-Type: application/octet-stream\r\nContent-Encoding: identity\r\nAuthorization: test-only\r\nCookie: test-only\r\nX-Request: do-not-echo\r\nContent-Length: {}\r\n\r\n",
                self.unused_upstream.local_addr().unwrap(), body.len()
            ).into_bytes();
            request.extend_from_slice(body);
            println!("Local client sent={request:?}");
            stream.write_all(&request).await.unwrap();
            let mut bytes = Vec::new();
            loop {
                let mut buffer = [0_u8; 512];
                let read = stream.read(&mut buffer).await.unwrap();
                assert!(read > 0, "Local response ended before complete HTTP body");
                bytes.extend_from_slice(&buffer[..read]);
                let Some(end) = bytes.windows(4).position(|chunk| chunk == b"\r\n\r\n").map(|index| index + 4) else { continue; };
                let mut message = Message::from_raw_http1_head(&bytes[..end], Bytes::new()).unwrap();
                let length = message.declared_content_length().expect("Local reply is fixed-length");
                if bytes.len() < end + length { continue; }
                assert_eq!(bytes.len(), end + length, "one response per request");
                message.body = Bytes::copy_from_slice(&bytes[end..]);
                println!("Local client received={bytes:?}");
                return message;
            }
        }).await.expect("Local HTTP exchange timed out")
    }
}

fn response_rules(listener: &ProxyListener) -> Vec<RuleDefinition> {
    [
        (
            RuleStage::ProxyToUpstream,
            HttpAction::ReplaceBodyText("upstream-effective".into()),
        ),
        (
            RuleStage::ProxyToApp,
            HttpAction::ReplaceBodyText("downstream-effective".into()),
        ),
        (
            RuleStage::ProxyToApp,
            HttpAction::SetHeader {
                name: "x-local-rule".into(),
                value: "applied".into(),
            },
        ),
        (
            RuleStage::ProxyToApp,
            HttpAction::CustomHttpStatus { status: 207 },
        ),
    ]
    .into_iter()
    .enumerate()
    .map(|(index, (stage, action))| {
        let condition = if stage == RuleStage::ProxyToUpstream {
            Condition::Http {
                field: MatchField::Method,
                operator: MatchOperator::Equals("POST".into()),
            }
        } else {
            Condition::Http {
                    field: MatchField::RequestTarget,
                operator: MatchOperator::Equals("/modified".into()),
            }
        };
        RuleDefinition::create(
            RuleDefinitionDraft {
                name: format!("local rule {index}"),
                enabled: true,
                priority: i32::try_from(index).unwrap(),
                listener_id: listener.id,
                stage,
                content: RuleContent::Http(HttpRuleContent {
                    description: String::new(),
                    condition,
                    action: UnifiedAction::Http(action),
                }),
            },
            u64::try_from(index + 1).unwrap(),
        )
        .unwrap()
    })
    .collect()
}

fn assert_content_headers(response: &Message) {
    for forbidden in [
        b"host".as_slice(),
        b"authorization",
        b"cookie",
        b"x-request",
        b"transfer-encoding",
    ] {
        assert!(
            !response
                .headers
                .iter()
                .any(|header| header.name.eq_ignore_ascii_case(forbidden))
        );
    }
    for name in [b"content-type".as_slice(), b"content-encoding"] {
        assert!(
            response
                .headers
                .iter()
                .any(|header| header.name.eq_ignore_ascii_case(name))
        );
    }
    assert_eq!(
        response.declared_content_length(),
        Some(response.body.len())
    );
}

#[tokio::test]
async fn local_http_default_response_preserves_empty_and_binary_bodies_over_keep_alive() {
    let fixture = LocalFixture::start(false).await;
    let mut client = TcpStream::connect(fixture.address).await.unwrap();
    for body in [b"".as_slice(), b"\x00\xff\x81"] {
        let response = fixture.exchange(&mut client, "/default", body).await;
        assert_eq!(response.http_status(), Some(200));
        assert_eq!(response.body.as_ref(), body);
        assert_content_headers(&response);
    }
    drop(client);
    fixture.finish().await;
}

#[tokio::test]
async fn local_http_default_response_allows_real_upstream_and_downstream_rules() {
    let fixture = LocalFixture::start(true).await;
    let mut client = TcpStream::connect(fixture.address).await.unwrap();
    let echoed = fixture
        .exchange(&mut client, "/echo", b"client-original")
        .await;
    assert_eq!(echoed.http_status(), Some(200));
    assert_eq!(echoed.body.as_ref(), b"upstream-effective");
    assert_content_headers(&echoed);
    let changed = fixture
        .exchange(&mut client, "/modified", b"client-original")
        .await;
    assert_eq!(changed.http_status(), Some(207));
    assert_eq!(changed.body.as_ref(), b"downstream-effective");
    assert!(changed.headers.iter().any(
        |header| header.name.eq_ignore_ascii_case(b"x-local-rule") && header.value == "applied"
    ));
    assert_content_headers(&changed);
    let counts = fixture
        .rules
        .snapshot
        .lock()
        .rules
        .iter()
        .map(|rule| rule.lifecycle().hit_count)
        .collect::<Vec<_>>();
    println!("Local real rule hit counts={counts:?}");
    assert_eq!(counts, vec![2, 1, 1, 1]);
    drop(client);
    fixture.finish().await;
}
