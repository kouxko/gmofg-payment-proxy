use super::production_shape::{Phase10ActorRules, actor_pipeline};
use super::*;
use crate::adapters::{InProcessWasmRuntime, ProtocolPackageRuntime};
use async_trait::async_trait;
use intercept_proxy_application::ListenerRuntimePort;
use intercept_proxy_domain::{
    FixedServerSettings, HttpTopology, ListenerId, Revision, UpstreamTlsSettings,
};
use intercept_proxy_package_runtime::{WasmPackageRuntime, read_package_component};
use std::{net::SocketAddr, path::PathBuf};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::{TcpListener, TcpStream},
};

const IO_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(5);
const REQUEST_TARGET: &str =
    "/gateway/products-pointPay/version=1.0/B01/1/0/MERCHANT%20%20TERMINAL%20%20TOKEN";
const EXPECTED_SIGN: &str = "F070F340438D40D0BC07295E241EE519";

#[derive(Debug)]
struct FixedCpsProvider(RuntimeExternalSocketPackageBinding);

#[async_trait]
impl ExternalSocketPackageProvider for FixedCpsProvider {
    async fn resolve(
        &self,
        package: &ProtocolPackageRef,
    ) -> intercept_proxy_application::AppResult<Option<RuntimeExternalSocketPackageBinding>> {
        Ok((self.0.registration().package().identity() == *package).then(|| self.0.clone()))
    }
}

#[derive(Debug)]
struct WireMessage {
    bytes: Vec<u8>,
    chunks: Vec<Vec<u8>>,
    head: String,
    body: Vec<u8>,
}

#[tokio::test]
#[allow(clippy::too_many_lines)]
async fn real_cps_wasm_resigns_modified_pay_response_on_app_wire() {
    let (before_body, after_body) = java_pay_fixture_pair();
    let package = load_cps_dist_package();
    let package_ref = package.manifest().package().identity().clone();
    let package_runtime: Arc<dyn ProtocolPackageRuntime> = Arc::new(InProcessWasmRuntime::new(
        WasmPackageRuntime::load(&package).await.unwrap(),
    ));
    let binding = RuntimeExternalSocketPackageBinding::new(
        package.manifest().clone(),
        Arc::clone(&package_runtime),
    );

    let origin = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let origin_address = origin.local_addr().unwrap();
    let path_id = Uuid::new_v4().to_string();
    let origin_path_id = path_id.clone();
    let origin_before_body = before_body.clone();
    let origin_task = tokio::spawn(async move {
        let (mut stream, peer) = tokio::time::timeout(IO_TIMEOUT, origin.accept())
            .await
            .expect("CPS origin accept timed out")
            .unwrap();
        let request = read_http_message(&mut stream).await;
        assert!(
            request
                .head
                .starts_with(&format!("POST {REQUEST_TARGET} HTTP/1.1\r\n"))
        );
        assert_header(&request.head, "x-path-id", &origin_path_id);
        assert_content_length(&request);
        assert_eq!(request.body, br#"{"request":"preserved"}"#);

        let response_head = format!(
            "HTTP/1.1 200 OK\r\nContent-Type: application/json; charset=utf-8\r\nX-Path-Id: {origin_path_id}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
            origin_before_body.len()
        );
        let mut response = response_head.into_bytes();
        response.extend_from_slice(origin_before_body.as_bytes());
        stream.write_all(&response).await.unwrap();
        (peer, request, response)
    });

    let reserved = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let proxy_address = reserved.local_addr().unwrap();
    drop(reserved);
    let listener = ProxyListener {
        id: ListenerId::new(),
        name: "CPS checksum wire".into(),
        enabled: false,
        bind_address: proxy_address.ip().to_string(),
        port: proxy_address.port(),
        data_plane: ListenerDataPlane::Http(HttpListenerSettings {
            topology: HttpTopology::remote(Some(FixedServerSettings {
                upstream_url: format!("http://{origin_address}"),
                upstream_tls: UpstreamTlsSettings::default(),
            })),
            body_processing: HttpBodyProcessing::Protocol {
                package: package_ref,
            },
            ..HttpListenerSettings::default()
        }),
        ..ProxyListener::default()
    };
    let amount_rule = amount_rule(&listener);
    let workspace = ProxyWorkspace {
        id: intercept_proxy_domain::WorkspaceId::new(),
        name: "CPS checksum wire".into(),
        revision: Revision::INITIAL,
        listeners: vec![listener.clone()],
        rule_definitions: vec![amount_rule],
        rule_created_order_high_water: 1,
        ..ProxyWorkspace::default()
    };
    workspace.validate().unwrap();

    let store = Arc::new(SqliteStore::in_memory().unwrap());
    store
        .insert_workspace(&crate::adapters::WorkspaceRepositoryAdapter::record(&workspace).unwrap())
        .unwrap();
    let runtime = test_listener_runtime(store);
    runtime.set_external_package_provider(Arc::new(FixedCpsProvider(binding)));
    let snapshot = HttpProtocolRuntimeSnapshot::prepare_async(&runtime, &workspace, &listener)
        .await
        .unwrap()
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
    runtime.set_pipeline_ports(Arc::new(actor_pipeline(&snapshot, Arc::clone(&rules))));
    runtime.start(workspace, listener.clone()).await.unwrap();

    let mut client = TcpStream::connect(proxy_address).await.unwrap();
    let client_local = client.local_addr().unwrap();
    let request_body = br#"{"request":"preserved"}"#;
    let client_head = format!(
        "POST {REQUEST_TARGET} HTTP/1.1\r\nHost: cps.test\r\nContent-Type: application/json; charset=utf-8\r\nX-Path-Id: {path_id}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
        request_body.len()
    );
    let mut client_input = client_head.into_bytes();
    client_input.extend_from_slice(request_body);
    client.write_all(&client_input).await.unwrap();
    let app_response = read_http_message(&mut client).await;
    runtime.stop(listener.id).await.unwrap();
    let listener_cleaned_up = runtime.statuses().await.unwrap().is_empty();
    assert!(listener_cleaned_up);

    let (origin_peer, origin_request, origin_response) =
        tokio::time::timeout(IO_TIMEOUT, origin_task)
            .await
            .expect("CPS origin task join timed out")
            .unwrap();
    assert!(app_response.head.starts_with("HTTP/1.1 200 OK\r\n"));
    assert_header(&app_response.head, "x-path-id", &path_id);
    assert_content_length(&app_response);

    let before: serde_json::Value = serde_json::from_str(&before_body).unwrap();
    let mut after: serde_json::Value = serde_json::from_str(&after_body).unwrap();
    let actual: serde_json::Value = serde_json::from_slice(&app_response.body).unwrap();
    assert_eq!(actual["data"]["result"]["amount"].as_f64(), Some(250.0));
    assert_eq!(actual["data"]["sign"], EXPECTED_SIGN);
    after["data"]["result"]["amount"] = actual["data"]["result"]["amount"].clone();
    after["data"]["sign"] = serde_json::json!(EXPECTED_SIGN);
    assert_json_semantically_eq(&actual, &after, "Java pay-after document");
    assert_only_amount_and_sign_changed(&before, &actual);
    assert_eq!(rules.commit_attempts.load(Ordering::SeqCst), 1);
    let committed = rules.snapshot.lock().clone();
    assert_eq!(committed.rules.len(), 1);
    assert_eq!(committed.rules[0].lifecycle().hit_count, 1);

    write_evidence(
        &path_id,
        client_local,
        proxy_address,
        origin_peer,
        origin_address,
        listener_cleaned_up,
        &before,
        &actual,
        committed.rules[0].lifecycle().hit_count,
        [&origin_request.chunks, &app_response.chunks],
        [
            ("client_input", client_input.as_slice()),
            ("origin_request", origin_request.bytes.as_slice()),
            ("origin_response", origin_response.as_slice()),
            ("app_response", app_response.bytes.as_slice()),
        ],
    );
}

fn amount_rule(listener: &ProxyListener) -> RuleDefinition {
    RuleDefinition::create(
        RuleDefinitionDraft {
            name: "CPS pay amount 100 to 250".into(),
            enabled: true,
            priority: 10,
            listener_id: listener.id,
            stage: RuleStage::ProxyToApp,
            content: RuleContent::Http(HttpRuleContent {
                description: "Java oracle pay-before to pay-after".into(),
                condition: Condition::Document {
                    path: JsonPointer::parse("/data/result/amount").unwrap(),
                    predicate: DocumentPredicate::Number(intercept_proxy_domain::NumberPredicate {
                        operator: intercept_proxy_domain::NumberOperator::Equal,
                        value: intercept_proxy_domain::DocumentNumber::new(100.0).unwrap(),
                    }),
                },
                action: UnifiedAction::Document(DocumentMutation::Set {
                    path: JsonPointer::parse("/data/result/amount").unwrap().into(),
                    value: DocumentValue::integer(250).unwrap(),
                }),
            }),
        },
        1,
    )
    .unwrap()
}

fn java_pay_fixture_pair() -> (String, String) {
    let fixture = repository_root()
        .join("examples/protocol-packages/gmofg_cps/tests/fixtures/java-signatures.json");
    let rows: serde_json::Value = serde_json::from_slice(&std::fs::read(fixture).unwrap()).unwrap();
    let body = |id: &str| {
        rows.as_array()
            .unwrap()
            .iter()
            .find(|row| row["id"] == id)
            .unwrap_or_else(|| panic!("missing Java CPS fixture {id}"))["body"]
            .as_str()
            .unwrap()
            .to_owned()
    };
    (body("pay-before"), body("pay-after"))
}

fn load_cps_dist_package() -> intercept_proxy_package_runtime::PackageComponent {
    let component = std::fs::read(
        repository_root().join("examples/protocol-packages/gmofg_cps/dist/gmofg-cps-1.0.0.wasm"),
    )
    .unwrap();
    read_package_component(&component).unwrap()
}

fn repository_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../..")
}

fn assert_only_amount_and_sign_changed(before: &serde_json::Value, actual: &serde_json::Value) {
    let mut expected = before.clone();
    expected["data"]["result"]["amount"] = actual["data"]["result"]["amount"].clone();
    expected["data"]["sign"] = serde_json::json!(EXPECTED_SIGN);
    assert_json_semantically_eq(actual, &expected, "amount and sign delta");
}

fn assert_json_semantically_eq(
    actual: &serde_json::Value,
    expected: &serde_json::Value,
    context: &str,
) {
    let mut actual = actual.clone();
    let mut expected = expected.clone();
    normalize_numbers(&mut actual);
    normalize_numbers(&mut expected);
    assert_eq!(actual, expected, "{context}");
}

fn normalize_numbers(value: &mut serde_json::Value) {
    match value {
        serde_json::Value::Number(number) => {
            *number = serde_json::Number::from_f64(number.as_f64().unwrap()).unwrap();
        }
        serde_json::Value::Array(values) => {
            values.iter_mut().for_each(normalize_numbers);
        }
        serde_json::Value::Object(values) => {
            values.values_mut().for_each(normalize_numbers);
        }
        serde_json::Value::Null | serde_json::Value::Bool(_) | serde_json::Value::String(_) => {}
    }
}

#[allow(clippy::too_many_arguments)]
fn write_evidence(
    path_id: &str,
    client_local: SocketAddr,
    proxy_address: SocketAddr,
    origin_peer: SocketAddr,
    origin_address: SocketAddr,
    listener_cleaned_up: bool,
    decoded_before: &serde_json::Value,
    final_document: &serde_json::Value,
    rule_hit_count: u64,
    received_chunks: [&Vec<Vec<u8>>; 2],
    messages: [(&str, &[u8]); 4],
) {
    let Ok(directory) = std::env::var("CPS_WIRE_EVIDENCE_DIR") else {
        return;
    };
    let directory = PathBuf::from(directory);
    std::fs::create_dir_all(&directory).unwrap();
    for (name, bytes) in messages {
        std::fs::write(directory.join(format!("{name}.bin")), bytes).unwrap();
        std::fs::write(directory.join(format!("{name}.http")), bytes).unwrap();
    }
    for (name, chunks) in ["origin_request", "app_response"]
        .into_iter()
        .zip(received_chunks)
    {
        std::fs::write(
            directory.join(format!("{name}.chunks.json")),
            serde_json::to_vec_pretty(chunks).unwrap(),
        )
        .unwrap();
    }
    std::fs::write(
        directory.join("decode.json"),
        serde_json::to_vec_pretty(&serde_json::json!({
            "operation": "pay",
            "request_target": REQUEST_TARGET,
            "parsed_origin_response_json": decoded_before,
            "observation": "JSON parsed from the exact origin fixture; actual Wasm decode is exercised by the matching Document rule and separately by Host integration",
            "final_document": final_document,
        }))
        .unwrap(),
    )
    .unwrap();
    std::fs::write(
        directory.join("rule.json"),
        serde_json::to_vec_pretty(&serde_json::json!({
            "stage": "proxy_to_app",
            "path": "/data/result/amount",
            "from": 100,
            "to": 250,
            "hit_count": rule_hit_count,
            "expected_sign": EXPECTED_SIGN,
        }))
        .unwrap(),
    )
    .unwrap();
    std::fs::write(
        directory.join("connection.json"),
        serde_json::to_vec_pretty(&serde_json::json!({
            "path_id": path_id,
            "client_to_proxy": { "client_local": client_local, "proxy_listener": proxy_address },
            "proxy_to_origin": { "proxy_egress_observed_by_origin": origin_peer, "origin_listener": origin_address },
            "listener_cleaned_up": listener_cleaned_up,
            "assertions": "PASS",
        }))
        .unwrap(),
    )
    .unwrap();
}

async fn read_http_message(stream: &mut TcpStream) -> WireMessage {
    tokio::time::timeout(IO_TIMEOUT, async {
        let mut bytes = Vec::new();
        let mut chunks = Vec::new();
        loop {
            let mut buffer = [0_u8; 1024];
            let read = stream.read(&mut buffer).await.unwrap();
            assert!(read > 0, "HTTP stream closed before the complete message");
            chunks.push(buffer[..read].to_vec());
            bytes.extend_from_slice(&buffer[..read]);
            let Some(head_end) = bytes
                .windows(4)
                .position(|part| part == b"\r\n\r\n")
                .map(|index| index + 4)
            else {
                continue;
            };
            let head = String::from_utf8(bytes[..head_end].to_vec()).unwrap();
            let length = header_value(&head, "content-length")
                .unwrap()
                .parse::<usize>()
                .unwrap();
            if bytes.len() < head_end + length {
                continue;
            }
            assert_eq!(bytes.len(), head_end + length, "expected one HTTP message");
            return WireMessage {
                body: bytes[head_end..].to_vec(),
                bytes,
                chunks,
                head,
            };
        }
    })
    .await
    .expect("CPS HTTP message read timed out")
}

fn header_value<'a>(head: &'a str, name: &str) -> Option<&'a str> {
    head.lines().skip(1).find_map(|line| {
        let (actual, value) = line.split_once(':')?;
        actual.eq_ignore_ascii_case(name).then(|| value.trim())
    })
}

fn assert_header(head: &str, name: &str, expected: &str) {
    assert_eq!(header_value(head, name), Some(expected));
}

fn assert_content_length(message: &WireMessage) {
    let declared = header_value(&message.head, "content-length")
        .unwrap()
        .parse::<usize>()
        .unwrap();
    assert_eq!(declared, message.body.len());
    assert_eq!(message.bytes.len(), message.head.len() + message.body.len());
}
