use std::{
    env,
    fmt::Write as _,
    fs,
    net::{IpAddr, Ipv4Addr, SocketAddr},
    path::PathBuf,
    sync::Arc,
    time::Duration,
};

use async_trait::async_trait;
use gmofg_proxy_infrastructure::{CertificateService, LeafCertificateRequest};
use gmofg_proxy_runtime::{
    Channel, ChannelConfig, ConnectionAdmission, ConnectionContext, DEFAULT_MAX_CONNECTIONS,
    FaultAction, HandshakePolicy, Message, MessageLimits, PipelinePorts, ProxyConfig, ProxyError,
    ProxySupervisor, SystemClock, TokioListenerBinder,
    tls::{ClientTlsAdapter, ServerTlsAdapter},
    transport::{ConnectionService, ForwardRequest, HyperUpstreamConnector, UpstreamConnector},
};
use p12_keystore::{KeyStore, KeyStoreEntry, Pkcs12ImportPolicy};
use ring::digest::{SHA256, digest};
use tokio::sync::mpsc;
use zeroize::Zeroizing;

const UPSTREAM_HOST: &str = "https.gmo-fg.net";
const UPSTREAM_PORT: u16 = 16_127;
const DEFAULT_LISTEN_IP: Ipv4Addr = Ipv4Addr::UNSPECIFIED;
const DEFAULT_PROXY_SAN_IP: Ipv4Addr = Ipv4Addr::new(10, 0, 34, 50);

#[derive(Debug)]
enum ProbeEvent {
    ResponseObserved {
        status_line: String,
        body_bytes: usize,
        body_sha256: String,
        successful: Option<bool>,
        error_code: Option<String>,
    },
    Completed,
    Failed {
        code: String,
        message: String,
    },
}

#[derive(Debug)]
struct TestClientIdentity {
    certificate_der: Vec<u8>,
    private_key_pkcs8_der: Zeroizing<Vec<u8>>,
    chain_der: Vec<Vec<u8>>,
    fingerprint_sha256: Vec<u8>,
}

#[derive(Debug, Clone)]
struct ProbePipelinePorts {
    events: mpsc::Sender<ProbeEvent>,
}

#[derive(Debug, Clone)]
struct ProbeUpstreamConnector {
    inner: HyperUpstreamConnector,
    events: mpsc::Sender<ProbeEvent>,
}

#[async_trait]
impl UpstreamConnector for ProbeUpstreamConnector {
    async fn send(
        &self,
        request: ForwardRequest,
        actions: &[FaultAction],
        cancellation: &tokio_util::sync::CancellationToken,
    ) -> gmofg_proxy_runtime::Result<Message> {
        let result = self.inner.send(request, actions, cancellation).await;
        if let Err(error) = &result {
            println!(
                "PROXY_UPSTREAM_FAILED code={} message={}",
                error.code, error.message
            );
            let _ = self
                .events
                .send(ProbeEvent::Failed {
                    code: error.code.to_owned(),
                    message: error.message.clone(),
                })
                .await;
        }
        result
    }
}

impl HandshakePolicy for ProbePipelinePorts {}

#[async_trait]
impl PipelinePorts for ProbePipelinePorts {
    async fn connection_opened(&self, context: &ConnectionContext) {
        let identity = context.tls_peer.as_ref();
        let fingerprint_suffix = identity.map_or_else(
            || "missing".to_owned(),
            |peer| {
                let segments = peer
                    .sha256_fingerprint
                    .rsplit(':')
                    .take(4)
                    .collect::<Vec<_>>();
                segments.into_iter().rev().collect::<Vec<_>>().join(":")
            },
        );
        println!(
            "PROXY_CLIENT_ACCEPTED peer={} cert_fingerprint_suffix={fingerprint_suffix}",
            context.peer_addr
        );
    }

    async fn request(
        &self,
        _context: &ConnectionContext,
        message: &mut Message,
    ) -> gmofg_proxy_runtime::Result<Vec<FaultAction>> {
        let parsed = message.parse_shift_jis_json()?;
        let transaction_type = parsed
            .get("TransactionType")
            .and_then(serde_json::Value::as_str)
            .unwrap_or("unknown");
        let request_id = parsed
            .get("RequestID")
            .and_then(serde_json::Value::as_str)
            .unwrap_or("unknown");
        println!(
            "PROXY_REQUEST_RECEIVED start_line={} body_bytes={} transaction_type={} request_id={}",
            message.start_line,
            message.body.len(),
            transaction_type,
            request_id
        );
        Ok(Vec::new())
    }

    async fn response(
        &self,
        _context: &ConnectionContext,
        message: &mut Message,
    ) -> gmofg_proxy_runtime::Result<Vec<FaultAction>> {
        let parsed = message.parse_shift_jis_json().ok();
        let error_code = parsed
            .as_ref()
            .and_then(|value| value.get("ErrorCode"))
            .and_then(serde_json::Value::as_str)
            .map(ToOwned::to_owned);
        let successful = error_code.as_ref().map(|value| value.trim().is_empty());
        let _ = self
            .events
            .send(ProbeEvent::ResponseObserved {
                status_line: message.start_line.clone(),
                body_bytes: message.body.len(),
                body_sha256: sha256_hex(&message.body),
                successful,
                error_code,
            })
            .await;
        Ok(Vec::new())
    }

    async fn connection_closed(
        &self,
        _context: &ConnectionContext,
        result: &gmofg_proxy_runtime::Result<()>,
    ) {
        match result {
            Ok(()) => {
                let _ = self.events.send(ProbeEvent::Completed).await;
            }
            Err(error) => {
                println!(
                    "PROXY_CLIENT_CONNECTION_CLOSED code={} message={}",
                    error.code, error.message
                );
                let _ = self
                    .events
                    .send(ProbeEvent::Failed {
                        code: error.code.to_owned(),
                        message: error.message.clone(),
                    })
                    .await;
            }
        }
    }
}

fn required_path(name: &str) -> Result<PathBuf, Box<dyn std::error::Error>> {
    let value = env::var_os(name).ok_or_else(|| format!("{name} is required"))?;
    Ok(PathBuf::from(value))
}

fn parse_test_client_pkcs12(
    bytes: &[u8],
    password: &str,
) -> Result<TestClientIdentity, Box<dyn std::error::Error>> {
    let store = KeyStore::from_pkcs12(bytes, password, Pkcs12ImportPolicy::Strict)?;
    let identities = store
        .entries()
        .filter_map(|(_, entry)| match entry {
            KeyStoreEntry::PrivateKeyChain(chain) => Some(chain),
            _ => None,
        })
        .collect::<Vec<_>>();
    if identities.len() != 1 {
        return Err(format!(
            "test PKCS12 must contain exactly one private-key identity, found {}",
            identities.len()
        )
        .into());
    }
    let identity = identities[0];
    let (certificate, chain) = identity
        .certs()
        .split_first()
        .ok_or("test PKCS12 identity has no certificate")?;
    Ok(TestClientIdentity {
        certificate_der: certificate.as_der().to_vec(),
        private_key_pkcs8_der: Zeroizing::new(identity.key().as_der().to_vec()),
        chain_der: chain
            .iter()
            .map(|certificate| certificate.as_der().to_vec())
            .collect(),
        fingerprint_sha256: digest(&SHA256, certificate.as_der()).as_ref().to_vec(),
    })
}

async fn select_upstream_address(
    tls: &ClientTlsAdapter,
) -> Result<SocketAddr, Box<dyn std::error::Error>> {
    let addresses = tokio::net::lookup_host((UPSTREAM_HOST, UPSTREAM_PORT))
        .await?
        .filter(SocketAddr::is_ipv4)
        .collect::<Vec<_>>();
    if addresses.is_empty() {
        return Err("upstream has no IPv4 address".into());
    }
    let mut failures = Vec::new();
    for address in addresses {
        let attempt = tokio::time::timeout(Duration::from_secs(15), async {
            let tcp = tokio::net::TcpStream::connect(address)
                .await
                .map_err(|error| error.to_string())?;
            tls.connect(UPSTREAM_HOST, Box::new(tcp))
                .await
                .map_err(|error| format!("{}: {}", error.code, error.message))
        })
        .await;
        match attempt {
            Ok(Ok(_)) => {
                println!("PROXY_UPSTREAM_TLS_READY address={address}");
                return Ok(address);
            }
            Ok(Err(error)) => failures.push(format!("{address}: {error}")),
            Err(_) => failures.push(format!("{address}: TLS preflight timed out")),
        }
    }
    Err(format!("upstream TLS preflight failed: {}", failures.join("; ")).into())
}

fn proxy_error(error: ProxyError) -> Box<dyn std::error::Error> {
    Box::new(error)
}

fn sha256_hex(bytes: &[u8]) -> String {
    digest(&SHA256, bytes)
        .as_ref()
        .iter()
        .fold(String::new(), |mut output, byte| {
            let _ = write!(output, "{byte:02x}");
            output
        })
}

async fn wait_for_forwarded_response(
    events: &mut mpsc::Receiver<ProbeEvent>,
) -> Result<(String, usize, String, Option<bool>, Option<String>), Box<dyn std::error::Error>> {
    let mut response = None;
    loop {
        let event = events.recv().await.ok_or("proxy event channel closed")?;
        match event {
            ProbeEvent::ResponseObserved {
                status_line,
                body_bytes,
                body_sha256,
                successful,
                error_code,
            } => {
                response = Some((status_line, body_bytes, body_sha256, successful, error_code));
            }
            ProbeEvent::Completed => {
                return response.ok_or_else(|| {
                    "client connection completed without an upstream response".into()
                });
            }
            ProbeEvent::Failed { code, message } => {
                return Err(format!("proxy connection failed: {code}: {message}").into());
            }
        }
    }
}

#[tokio::main]
#[allow(clippy::too_many_lines)]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let client_p12_path = required_path("GMOFG_CLIENT_P12")?;
    let upstream_ca_path = required_path("GMOFG_UPSTREAM_CA_DER")?;
    let proxy_ca_output = required_path("GMOFG_PROXY_CA_OUTPUT")?;
    let client_p12_password = env::var("GMOFG_CLIENT_P12_PASSWORD").unwrap_or_default();

    let certificate_service = CertificateService;
    // The real development identity uses a self-signed legacy CA without CA
    // extensions. This test-only loader preserves the actual device identity;
    // production certificate import remains strict.
    let client_identity =
        parse_test_client_pkcs12(&fs::read(&client_p12_path)?, &client_p12_password)?;
    let client_ca_der = client_identity
        .chain_der
        .last()
        .cloned()
        .ok_or("client PKCS12 does not contain a CA certificate")?;
    let client_fingerprint = client_identity.fingerprint_sha256.clone();

    let proxy_root =
        certificate_service.generate_root_ca("GMO-FG Real Device DLL Proxy Test Root")?;
    let proxy_leaf = certificate_service.generate_leaf(
        &proxy_root.certificate_der,
        &proxy_root.private_key_pkcs8_der,
        &LeafCertificateRequest {
            common_name: DEFAULT_PROXY_SAN_IP.to_string(),
            dns_names: Vec::new(),
            ip_addresses: vec![IpAddr::V4(DEFAULT_PROXY_SAN_IP)],
        },
    )?;
    if let Some(parent) = proxy_ca_output.parent() {
        fs::create_dir_all(parent)?;
    }
    fs::write(&proxy_ca_output, &proxy_root.certificate_der)?;

    let (events_tx, mut events_rx) = mpsc::channel(4);
    let ports = Arc::new(ProbePipelinePorts {
        events: events_tx.clone(),
    });
    let acceptor = ServerTlsAdapter::build(
        vec![
            proxy_leaf.certificate_der.clone(),
            proxy_root.certificate_der.clone(),
        ],
        proxy_leaf.private_key_pkcs8_der.to_vec(),
        client_ca_der,
        Some(client_fingerprint),
        ports.clone(),
    )
    .map_err(proxy_error)?;

    let mut upstream_client_chain = vec![client_identity.certificate_der.clone()];
    upstream_client_chain.extend(client_identity.chain_der.clone());
    let upstream_tls = ClientTlsAdapter::build(
        upstream_client_chain,
        client_identity.private_key_pkcs8_der.to_vec(),
        fs::read(upstream_ca_path)?,
    )
    .map_err(proxy_error)?;
    let upstream_address = select_upstream_address(&upstream_tls).await?;
    let limits = MessageLimits::default();
    let connector = HyperUpstreamConnector {
        address: upstream_address,
        host: UPSTREAM_HOST.to_owned(),
        host_header: format!("{UPSTREAM_HOST}:{UPSTREAM_PORT}"),
        rewrite_host: true,
        tls: Some(upstream_tls),
        connect_timeout: Duration::from_secs(30),
        write_timeout: Duration::from_secs(30),
        read_timeout: Duration::from_secs(90),
        limits,
    };
    let service = ConnectionService {
        acceptor: Arc::new(acceptor),
        upstream: Arc::new(ProbeUpstreamConnector {
            inner: connector,
            events: events_tx,
        }),
        ports,
        clock: Arc::new(SystemClock),
        admission: ConnectionAdmission::new(DEFAULT_MAX_CONNECTIONS).map_err(proxy_error)?,
        limits,
        read_timeout: Duration::from_secs(90),
    };
    let supervisor = ProxySupervisor::new(Arc::new(TokioListenerBinder), service);
    let snapshot = supervisor
        .start(ProxyConfig {
            channels: vec![ChannelConfig {
                channel: Channel::Dll,
                enabled: true,
                listen_addr: SocketAddr::new(IpAddr::V4(DEFAULT_LISTEN_IP), UPSTREAM_PORT),
                upstream_url: format!("https://{UPSTREAM_HOST}:{UPSTREAM_PORT}"),
            }],
            limits,
            max_connections: DEFAULT_MAX_CONNECTIONS,
            connect_timeout: Duration::from_secs(30),
            write_timeout: Duration::from_secs(30),
            read_timeout: Duration::from_secs(90),
            rewrite_host: true,
            leaf_sans: vec![DEFAULT_PROXY_SAN_IP.to_string()],
        })
        .await
        .map_err(proxy_error)?;
    println!(
        "PROXY_READY listen=0.0.0.0:{UPSTREAM_PORT} upstream=https://{UPSTREAM_HOST}:{UPSTREAM_PORT} ca_der={}",
        proxy_ca_output.display()
    );
    println!("PROXY_RUNTIME epoch={:?}", snapshot.runtime_epoch);

    let event = tokio::time::timeout(
        Duration::from_mins(3),
        wait_for_forwarded_response(&mut events_rx),
    )
    .await;
    let stop_result = supervisor.stop().await;
    if let Err(error) = stop_result {
        eprintln!(
            "PROXY_STOP_FAILED code={} message={}",
            error.code, error.message
        );
    }
    match event {
        Ok(Ok((status_line, body_bytes, body_sha256, successful, error_code))) => {
            println!(
                "PROXY_RESPONSE_FORWARDED status_line={status_line} body_bytes={body_bytes} body_sha256={body_sha256} successful={successful:?} error_code={error_code:?}"
            );
            Ok(())
        }
        Ok(Err(error)) => Err(error),
        Err(_) => Err("timed out waiting for a real-device DLL request".into()),
    }
}
