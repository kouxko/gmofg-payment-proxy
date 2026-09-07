use super::production_shape::{Phase10ActorRules, actor_pipeline};
use super::*;
use crate::adapters::{InProcessWasmRuntime, ProtocolPackageRuntime};
use async_trait::async_trait;
use intercept_proxy_application::ListenerRuntimePort;
use intercept_proxy_domain::{
    FixedServerSettings, HttpTopology, ListenerId, Revision, UpstreamTlsSettings,
};
use intercept_proxy_package_runtime::{
    WasmPackageRuntime, embed_package_manifest, read_package_component,
};
use std::{path::PathBuf, process::Command};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::{TcpListener, TcpStream},
};

const IO_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(5);

#[derive(Debug)]
struct FixedWasmProvider(RuntimeExternalSocketPackageBinding);

#[async_trait]
impl ExternalSocketPackageProvider for FixedWasmProvider {
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
    head: String,
    body: Vec<u8>,
}

#[tokio::test]
#[allow(clippy::too_many_lines)]
async fn real_wasm_finalize_reaches_fixed_server_and_app_wire() {
    let package = build_finalize_package();
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
    let origin_task = tokio::spawn(async move {
        let (mut stream, peer) = tokio::time::timeout(IO_TIMEOUT, origin.accept())
            .await
            .expect("origin accept timed out")
            .unwrap();
        let request = read_http_message(&mut stream).await;
        println!(
            "origin_observed_head={}origin_observed_body={:?}",
            request.head, request.body
        );
        assert!(
            request
                .head
                .starts_with("POST /upstream?value=upstream HTTP/1.1\r\n")
        );
        assert_header(&request.head, "x-value", "upstream");
        assert_header(&request.head, "x-path-id", &origin_path_id);
        assert_content_length(&request);
        assert_eq!(request.body, br#""upstream""#);

        let response = format!(
            "HTTP/1.1 200 OK\r\nContent-Type: application/json; charset=utf-8\r\nX-Value: before\r\nX-Path-Id: {origin_path_id}\r\nContent-Length: 8\r\nConnection: close\r\n\r\n\"before\""
        );
        stream.write_all(response.as_bytes()).await.unwrap();
        (peer, request, response.into_bytes())
    });

    let reserved = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let proxy_address = reserved.local_addr().unwrap();
    drop(reserved);
    let listener = ProxyListener {
        id: ListenerId::new(),
        name: "real wasm finalize wire".into(),
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
    let workspace = ProxyWorkspace {
        id: intercept_proxy_domain::WorkspaceId::new(),
        name: "real wasm finalize wire".into(),
        revision: Revision::INITIAL,
        listeners: vec![listener.clone()],
        ..ProxyWorkspace::default()
    };
    workspace.validate().unwrap();
    let store = Arc::new(SqliteStore::in_memory().unwrap());
    store
        .insert_workspace(&crate::adapters::WorkspaceRepositoryAdapter::record(&workspace).unwrap())
        .unwrap();
    let runtime = test_listener_runtime(store);
    runtime.set_external_package_provider(Arc::new(FixedWasmProvider(binding)));
    let snapshot = HttpProtocolRuntimeSnapshot::prepare_async(&runtime, &workspace, &listener)
        .await
        .unwrap()
        .unwrap();
    let rules = Arc::new(Phase10ActorRules {
        snapshot: Mutex::new(RuleRuntimeSnapshot::with_collection_identity_and_order(
            Some(workspace.id.as_uuid()),
            workspace.revision.get(),
            Vec::new(),
            Vec::new(),
        )),
        commit_attempts: AtomicUsize::new(0),
    });
    runtime.set_pipeline_ports(Arc::new(actor_pipeline(&snapshot, rules)));
    runtime.start(workspace, listener.clone()).await.unwrap();

    let mut client = TcpStream::connect(proxy_address).await.unwrap();
    let client_local = client.local_addr().unwrap();
    let client_input = format!(
        "POST /before?value=before HTTP/1.1\r\nHost: wire.test\r\nContent-Type: application/json; charset=utf-8\r\nX-Value: before\r\nX-Path-Id: {path_id}\r\nContent-Length: 8\r\nConnection: close\r\n\r\n\"before\""
    )
    .into_bytes();
    client.write_all(&client_input).await.unwrap();
    let app_response = read_http_message(&mut client).await;
    runtime.stop(listener.id).await.unwrap();
    let listener_cleaned_up = runtime.statuses().await.unwrap().is_empty();
    assert!(listener_cleaned_up);

    let (origin_peer, origin_request, origin_response) =
        tokio::time::timeout(IO_TIMEOUT, origin_task)
            .await
            .expect("origin task join timed out")
            .unwrap();
    assert!(app_response.head.starts_with("HTTP/1.1 202 Accepted\r\n"));
    assert_header(&app_response.head, "x-value", "downstream");
    assert_header(&app_response.head, "x-path-id", &path_id);
    assert_content_length(&app_response);
    assert_eq!(app_response.body, br#""downstream""#);

    write_evidence(
        &path_id,
        client_local,
        proxy_address,
        origin_peer,
        origin_address,
        listener_cleaned_up,
        [
            ("client_input", client_input.as_slice()),
            ("origin_request", origin_request.bytes.as_slice()),
            ("origin_response", origin_response.as_slice()),
            ("app_response", app_response.bytes.as_slice()),
        ],
    );

    println!("path_id={path_id}");
    println!(
        "connection client_local={client_local} proxy={proxy_address} origin_peer={origin_peer} origin={origin_address}"
    );
    println!("client_input={}", String::from_utf8_lossy(&client_input));
    println!(
        "origin_request_head={}origin_request_body={:?}",
        origin_request.head, origin_request.body
    );
    println!(
        "origin_response={}",
        String::from_utf8_lossy(&origin_response)
    );
    println!(
        "app_final_head={}app_final_body={:?}",
        app_response.head, app_response.body
    );
    println!("cleanup=PASS listener_stopped={listener_cleaned_up}");
    println!("assertions=PASS path_query header body status content_length exact_wire_causality");
}

fn write_evidence(
    path_id: &str,
    client_local: SocketAddr,
    proxy_address: SocketAddr,
    origin_peer: SocketAddr,
    origin_address: SocketAddr,
    listener_cleaned_up: bool,
    messages: [(&str, &[u8]); 4],
) {
    let Ok(directory) = std::env::var("HTTP_FINALIZE_WIRE_EVIDENCE_DIR") else {
        return;
    };
    let directory = PathBuf::from(directory);
    std::fs::create_dir_all(&directory).unwrap();
    for (name, bytes) in messages {
        std::fs::write(directory.join(format!("{name}.bin")), bytes).unwrap();
        std::fs::write(directory.join(format!("{name}.http")), bytes).unwrap();
    }
    let connection = serde_json::json!({
        "path_id": path_id,
        "client_to_proxy": { "client_local": client_local, "proxy_listener": proxy_address },
        "proxy_to_origin": { "proxy_egress_observed_by_origin": origin_peer, "origin_listener": origin_address },
        "listener_cleaned_up": listener_cleaned_up,
        "assertions": "PASS"
    });
    std::fs::write(
        directory.join("connection.json"),
        serde_json::to_vec_pretty(&connection).unwrap(),
    )
    .unwrap();
}

fn build_finalize_package() -> intercept_proxy_package_runtime::PackageComponent {
    let manifest_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let fixture = manifest_dir.join("../package-runtime/tests/fixtures/http-finalize");
    let target_dir = fixture.join("target/upstream-downstream");
    let output = Command::new(env!("CARGO"))
        .args([
            "build",
            "--locked",
            "--manifest-path",
            fixture.join("Cargo.toml").to_str().unwrap(),
            "--target",
            "wasm32-wasip2",
            "--target-dir",
            target_dir.to_str().unwrap(),
            "--no-default-features",
            "--features",
            "upstream,downstream",
        ])
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let component = std::fs::read(
        target_dir.join("wasm32-wasip2/debug/intercept_proxy_http_finalize_component.wasm"),
    )
    .unwrap();
    let manifest = std::fs::read(fixture.join("manifest.json")).unwrap();
    read_package_component(&embed_package_manifest(&component, &manifest).unwrap()).unwrap()
}

async fn read_http_message(stream: &mut TcpStream) -> WireMessage {
    tokio::time::timeout(IO_TIMEOUT, async {
        let mut bytes = Vec::new();
        loop {
            let mut buffer = [0_u8; 1024];
            let read = stream.read(&mut buffer).await.unwrap();
            assert!(
                read > 0,
                "HTTP stream closed before the complete message: {:?}",
                String::from_utf8_lossy(&bytes)
            );
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
            assert_eq!(
                bytes.len(),
                head_end + length,
                "expected exactly one HTTP message"
            );
            return WireMessage {
                body: bytes[head_end..].to_vec(),
                bytes,
                head,
            };
        }
    })
    .await
    .expect("HTTP message read timed out")
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
