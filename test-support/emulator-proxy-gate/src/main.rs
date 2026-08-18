//! Android emulator-only end-to-end proxy rule matrix.
//!
//! The executable deliberately lives under `test-support`. Its D48 payload comes from a local
//! simulated upstream and must never be reported as real GMO-FG or A920MAX acceptance evidence.

use std::{env, error::Error, fs, path::PathBuf, sync::Arc, time::Duration};

use encoding_rs::SHIFT_JIS;
use intercept_proxy_application::{
    AppResult, BodyCodecKind, CaptureQuery, CaptureSort, MessageStage, PageRequest, RuleAction,
    RuleCondition, RuleDraft, RuleDropResponseMode, RuleMatchField, RuleMatchOperator,
    RuleTerminalAction, SessionDetailViewModel, SessionQuery, SessionSort, SortDirection,
};
use intercept_proxy_domain::{FixedServerSettings, UpstreamTlsSettings};
use intercept_proxy_host::{ApplicationHostBuilder, HostPlatformServices};
use intercept_proxy_infrastructure::{
    InfrastructureError, NativeFileDialog, SecretProtector, adapters::FileSelection,
};
use intercept_proxy_product_api::InterceptProxyProfile;
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::TcpListener,
};

const DLL_DEVICE_PORT: u16 = 6555;
const TRANSACTION_DEVICE_PORT: u16 = 6556;
const DEFAULT_HOST_LISTENER_PORT: u16 = 16_555;
const EXPECTED_SESSION_COUNT: usize = 16;
const EXPECTED_DLL_UPSTREAM_REQUEST_COUNT: usize = 11;
const EXPECTED_TRANSACTION_UPSTREAM_REQUEST_COUNT: usize = 3;
const BASELINE_TEXT: &str = "{\"result\":\"D48\",\"message\":\"端末情報更新が必要です\"}";
const RESPONSE_RULE_TEXT: &str = "{\"result\":\"R48\",\"message\":\"代理修改\"}";

#[derive(Debug)]
struct NoFileDialog;

impl NativeFileDialog for NoFileDialog {
    fn choose_open_file(&self, _purpose: &str) -> AppResult<Option<PathBuf>> {
        Ok(None)
    }

    fn choose_save_file(
        &self,
        _purpose: &str,
        _suggested_file_name: &str,
    ) -> AppResult<Option<FileSelection>> {
        Ok(None)
    }
}

#[derive(Debug)]
struct TestOnlyProtector;

impl SecretProtector for TestOnlyProtector {
    fn protect(&self, plaintext: &[u8]) -> Result<Vec<u8>, InfrastructureError> {
        Ok(plaintext.iter().map(|byte| byte ^ 0xa5).collect())
    }

    fn unprotect(&self, ciphertext: &[u8]) -> Result<Vec<u8>, InfrastructureError> {
        Ok(ciphertext.iter().map(|byte| byte ^ 0xa5).collect())
    }
}

#[derive(Debug)]
struct RecordedRequest {
    path: String,
    head: String,
    body: Vec<u8>,
}

fn required_path(name: &str) -> Result<PathBuf, Box<dyn Error + Send + Sync>> {
    env::var_os(name)
        .map(PathBuf::from)
        .ok_or_else(|| format!("{name} is required").into())
}

fn session_query() -> SessionQuery {
    SessionQuery {
        keyword: None,
        terminal_ip: None,
        channel: None,
        result: None,
        rule_id: None,
        started_from: None,
        started_to: None,
        sort: SessionSort::StartedAt,
        direction: SortDirection::Asc,
    }
}

fn capture_query() -> CaptureQuery {
    CaptureQuery {
        keyword: None,
        terminal_ip: None,
        channel: None,
        stage: None,
        result: None,
        rule_id: None,
        after_event_id: None,
        sort: CaptureSort::OccurredAt,
        direction: SortDirection::Asc,
        page: PageRequest {
            page: 1,
            page_size: 100,
        },
    }
}

fn encode_shift_jis(text: &str) -> Result<Vec<u8>, Box<dyn Error + Send + Sync>> {
    let (bytes, _, had_errors) = SHIFT_JIS.encode(text);
    if had_errors {
        return Err(
            format!("fixture text is not losslessly representable in Shift-JIS: {text}").into(),
        );
    }
    Ok(bytes.into_owned())
}

fn find_header_end(bytes: &[u8]) -> Option<usize> {
    bytes.windows(4).position(|window| window == b"\r\n\r\n")
}

async fn read_request(
    socket: &mut tokio::net::TcpStream,
) -> Result<RecordedRequest, Box<dyn Error + Send + Sync>> {
    let mut request = Vec::new();
    let mut chunk = [0_u8; 4096];
    let header_end = loop {
        let count = socket.read(&mut chunk).await?;
        if count == 0 {
            return Err("upstream request closed before complete headers".into());
        }
        request.extend_from_slice(&chunk[..count]);
        if let Some(header_end) = find_header_end(&request) {
            break header_end;
        }
    };
    let head = String::from_utf8_lossy(&request[..header_end]).into_owned();
    let content_length = head
        .lines()
        .find_map(|line| {
            let (name, value) = line.split_once(':')?;
            name.eq_ignore_ascii_case("content-length")
                .then(|| value.trim().parse::<usize>().ok())
                .flatten()
        })
        .unwrap_or(0);
    let body_start = header_end + 4;
    while request.len() < body_start + content_length {
        let count = socket.read(&mut chunk).await?;
        if count == 0 {
            return Err("upstream request closed before complete body".into());
        }
        request.extend_from_slice(&chunk[..count]);
    }
    let path = head
        .lines()
        .next()
        .and_then(|line| line.split_whitespace().nth(1))
        .ok_or("upstream request line has no target")?
        .to_owned();
    Ok(RecordedRequest {
        path,
        head,
        body: request[body_start..body_start + content_length].to_vec(),
    })
}

async fn serve_fixtures(
    listener: TcpListener,
    baseline_body: Vec<u8>,
    expected_request_count: usize,
    channel: &'static str,
) -> Result<Vec<RecordedRequest>, Box<dyn Error + Send + Sync>> {
    let mut requests = Vec::new();
    while requests.len() < expected_request_count {
        let (mut socket, _) = listener.accept().await?;
        let request = read_request(&mut socket).await?;
        let body = match request.path.as_str() {
            "/matrix/response-modify" => encode_shift_jis(
                "{\"result\":\"UPSTREAM\",\"scenario\":\"response-modify\",\"message\":\"original\"}",
            )?,
            "/matrix/nth-hit" => encode_shift_jis(
                "{\"result\":\"D48\",\"scenario\":\"nth-hit\",\"message\":\"counter\"}",
            )?,
            "/matrix/truncate" => encode_shift_jis(
                "{\"result\":\"TRUNCATE\",\"scenario\":\"truncate\",\"message\":\"long response body\"}",
            )?,
            "/matrix/drop" => encode_shift_jis(
                "{\"result\":\"DROP\",\"scenario\":\"drop\",\"message\":\"discard me\"}",
            )?,
            _ => baseline_body.clone(),
        };
        let response_head = format!(
            "HTTP/1.1 200 OK\r\nContent-Type: application/json; charset=Shift_JIS\r\nX-Upstream-Fixture: simulated-d48\r\nX-Upstream-Channel: {channel}\r\nX-Header-Order: first\r\nX-Header-Order: second\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
            body.len(),
        );
        socket.write_all(response_head.as_bytes()).await?;
        socket.write_all(&body).await?;
        socket.shutdown().await?;
        requests.push(request);
    }
    Ok(requests)
}

fn path_rule(
    mut draft: RuleDraft,
    name: &str,
    path: &str,
    stage: MessageStage,
    actions: Vec<RuleAction>,
) -> RuleDraft {
    draft.name = format!("TEST ONLY - {name}");
    draft.description = "Android emulator proxy gate scenario".into();
    draft.enabled = true;
    draft.priority = 100;
    draft.stage = Some(stage);
    draft.conditions = vec![RuleCondition::Field {
        field: RuleMatchField::PathOrRequestType,
        operator: RuleMatchOperator::Contains { value: path.into() },
    }];
    draft.actions = actions;
    draft.one_shot = false;
    draft
}

/// Response rules cannot match the original request path because their message target is the HTTP
/// response status token. Use the response JSON itself as the supported, observable discriminator.
fn response_json_rule(
    mut draft: RuleDraft,
    name: &str,
    scenario: &str,
    actions: Vec<RuleAction>,
) -> RuleDraft {
    draft.name = format!("TEST ONLY - {name}");
    draft.description = "Android emulator proxy gate response scenario".into();
    draft.enabled = true;
    draft.priority = 100;
    draft.stage = Some(MessageStage::Response);
    draft.conditions = vec![RuleCondition::Field {
        field: RuleMatchField::JsonPath {
            path: "$.scenario".into(),
        },
        operator: RuleMatchOperator::Equals {
            value: scenario.into(),
        },
    }];
    draft.actions = actions;
    draft.one_shot = false;
    draft
}

fn session_for_path<'a>(
    sessions: &'a [SessionDetailViewModel],
    path: &str,
) -> Result<&'a SessionDetailViewModel, Box<dyn Error + Send + Sync>> {
    sessions
        .iter()
        .find(|session| session.summary.target.contains(path))
        .ok_or_else(|| format!("no session captured for {path}").into())
}

fn raw_header_value<'a>(session: &'a SessionDetailViewModel, name: &str) -> Option<&'a [u8]> {
    session
        .response
        .as_ref()?
        .raw_headers
        .iter()
        .find_map(|header| {
            header
                .name_bytes
                .eq_ignore_ascii_case(name.as_bytes())
                .then_some(header.value_bytes.as_slice())
        })
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn Error + Send + Sync>> {
    let data_dir = required_path("EMULATOR_PROXY_GATE_DATA_DIR")?;
    let ready_file = required_path("EMULATOR_PROXY_GATE_READY_FILE")?;
    let report_file = required_path("EMULATOR_PROXY_GATE_REPORT_FILE")?;
    let vpn_report_file = required_path("EMULATOR_PROXY_GATE_VPN_REPORT_FILE")?;
    let host_listener_port = env::var("EMULATOR_PROXY_GATE_HOST_PORT")
        .ok()
        .map(|value| value.parse::<u16>())
        .transpose()?
        .unwrap_or(DEFAULT_HOST_LISTENER_PORT);

    let baseline_body = encode_shift_jis(BASELINE_TEXT)?;
    let response_rule_body = encode_shift_jis(RESPONSE_RULE_TEXT)?;
    let dll_upstream_listener = TcpListener::bind("127.0.0.1:0").await?;
    let dll_upstream_address = dll_upstream_listener.local_addr()?;
    let dll_upstream_task = tokio::spawn(serve_fixtures(
        dll_upstream_listener,
        baseline_body.clone(),
        EXPECTED_DLL_UPSTREAM_REQUEST_COUNT,
        "dll",
    ));
    let transaction_upstream_listener = TcpListener::bind("127.0.0.1:0").await?;
    let transaction_upstream_address = transaction_upstream_listener.local_addr()?;
    let transaction_upstream_task = tokio::spawn(serve_fixtures(
        transaction_upstream_listener,
        baseline_body.clone(),
        EXPECTED_TRANSACTION_UPSTREAM_REQUEST_COUNT,
        "transaction",
    ));

    let host = ApplicationHostBuilder::new(
        &data_dir,
        HostPlatformServices::new(Arc::new(TestOnlyProtector), Arc::new(NoFileDialog)),
        Arc::new(InterceptProxyProfile),
    )
    .build()
    .await?;
    let application = host.application();

    let mut workspace = application
        .workspace_create("TEST ONLY - Emulator Rule Matrix".into())
        .await?;
    workspace.listeners.clear();
    workspace = application.workspace_save(workspace).await?;
    application.workspace_select(workspace.id).await?;

    let mut dll_listener = application.listener_new()?;
    let dll_listener_id = dll_listener.id();
    dll_listener.name = "TEST ONLY - DLL channel".into();
    // Android 的联合 VPN 探针通过模拟器宿主网关 10.0.3.2 访问这两个端口，因此测试
    // Listener 必须监听所有本机地址。它只存在于 mktemp 数据目录和本次门禁进程中。
    dll_listener.bind_address = "0.0.0.0".into();
    dll_listener.port = host_listener_port;
    let dll_http = dll_listener
        .http_mut()
        .expect("new listener defaults to HTTP");
    dll_http.request_body_codec = BodyCodecKind::Utf8;
    dll_http.response_body_codec = BodyCodecKind::ShiftJis;
    dll_http.fixed_server = Some(FixedServerSettings {
        upstream_url: format!("http://{dll_upstream_address}"),
        upstream_tls: UpstreamTlsSettings::default(),
    });
    application
        .listener_save(
            workspace.id,
            workspace.revision.get(),
            dll_listener,
            workspace.certificate_references.clone(),
        )
        .await?;

    workspace = application.workspace_get(workspace.id).await?;
    let mut transaction_listener = application.listener_new()?;
    let transaction_listener_id = transaction_listener.id();
    transaction_listener.name = "TEST ONLY - Transaction channel".into();
    transaction_listener.bind_address = "0.0.0.0".into();
    transaction_listener.port = host_listener_port + 1;
    let transaction_http = transaction_listener
        .http_mut()
        .expect("new listener defaults to HTTP");
    transaction_http.request_body_codec = BodyCodecKind::Utf8;
    transaction_http.response_body_codec = BodyCodecKind::ShiftJis;
    transaction_http.fixed_server = Some(FixedServerSettings {
        upstream_url: format!("http://{transaction_upstream_address}"),
        upstream_tls: UpstreamTlsSettings::default(),
    });
    application
        .listener_save(
            workspace.id,
            workspace.revision.get(),
            transaction_listener,
            workspace.certificate_references.clone(),
        )
        .await?;

    workspace = application.workspace_get(workspace.id).await?;

    let request_modify = path_rule(
        application.rule_new_draft().await?,
        "request header and JSON modification",
        "/matrix/request-modify",
        MessageStage::Request,
        vec![
            RuleAction::SetHeader {
                name: "x-request-rule".into(),
                value: "applied".into(),
            },
            RuleAction::SetJsonField {
                path: "$.amount".into(),
                value_json: "200".into(),
            },
        ],
    );
    application.rule_save(request_modify).await?;

    let response_modify = response_json_rule(
        application.rule_new_draft().await?,
        "response status header and body modification",
        "response-modify",
        vec![
            RuleAction::SetHeader {
                name: "x-response-rule".into(),
                value: "applied".into(),
            },
            RuleAction::CustomHttpStatus { status: 503 },
            RuleAction::ReplaceBodyText {
                text: RESPONSE_RULE_TEXT.into(),
            },
        ],
    );
    application.rule_save(response_modify).await?;

    let mock = path_rule(
        application.rule_new_draft().await?,
        "mock response",
        "/matrix/mock",
        MessageStage::Request,
        vec![RuleAction::Terminal {
            action: RuleTerminalAction::MockResponse {
                status: 202,
                headers: vec![
                    ("content-type".into(), "application/json".into()),
                    ("x-mock-rule".into(), "applied".into()),
                ],
                body_bytes: b"{\"result\":\"MOCK\"}".to_vec(),
            },
        }],
    );
    application.rule_save(mock).await?;

    let mut nth_hit = response_json_rule(
        application.rule_new_draft().await?,
        "second hit one shot",
        "nth-hit",
        vec![RuleAction::SetHeader {
            name: "x-nth-hit".into(),
            value: "second-only".into(),
        }],
    );
    nth_hit.conditions.push(RuleCondition::NthHit { count: 2 });
    nth_hit.one_shot = true;
    let nth_hit_rule = application.rule_save(nth_hit).await?;

    let delay = path_rule(
        application.rule_new_draft().await?,
        "deterministic delay",
        "/matrix/delay",
        MessageStage::Request,
        vec![RuleAction::Delay { milliseconds: 250 }],
    );
    application.rule_save(delay).await?;

    let truncate = response_json_rule(
        application.rule_new_draft().await?,
        "truncate response",
        "truncate",
        vec![RuleAction::Terminal {
            action: RuleTerminalAction::TruncateResponse { bytes: 7 },
        }],
    );
    application.rule_save(truncate).await?;

    let drop_response = path_rule(
        application.rule_new_draft().await?,
        "drop upstream response",
        "/matrix/drop",
        MessageStage::Request,
        vec![RuleAction::Terminal {
            action: RuleTerminalAction::DropUpstreamResponse {
                mode: RuleDropResponseMode::ReadCompleteResponse,
            },
        }],
    );
    application.rule_save(drop_response).await?;

    let disconnect = path_rule(
        application.rule_new_draft().await?,
        "disconnect before upstream",
        "/matrix/disconnect",
        MessageStage::Request,
        vec![RuleAction::Terminal {
            action: RuleTerminalAction::DisconnectBeforeUpstream,
        }],
    );
    application.rule_save(disconnect).await?;

    workspace = application.workspace_get(workspace.id).await?;
    application
        .listener_start(workspace.id, workspace.revision.get(), dll_listener_id)
        .await?;
    workspace = application.workspace_get(workspace.id).await?;
    application
        .listener_start(
            workspace.id,
            workspace.revision.get(),
            transaction_listener_id,
        )
        .await?;
    fs::write(
        &ready_file,
        format!(
            "dll_device_port={DLL_DEVICE_PORT}\ndll_host_port={host_listener_port}\ndll_listener_id={dll_listener_id}\ntransaction_device_port={TRANSACTION_DEVICE_PORT}\ntransaction_host_port={}\ntransaction_listener_id={transaction_listener_id}\n",
            host_listener_port + 1,
        ),
    )?;

    let summaries = tokio::time::timeout(Duration::from_secs(120), async {
        loop {
            let page = application.session_query(session_query()).await?;
            if page.items.len() == EXPECTED_SESSION_COUNT
                && page
                    .items
                    .iter()
                    .all(|summary| summary.completed_at.is_some())
            {
                break Ok::<_, intercept_proxy_application::AppError>(page.items);
            }
            tokio::time::sleep(Duration::from_millis(100)).await;
        }
    })
    .await
    .map_err(|_| "timed out waiting for emulator matrix sessions")??;

    let mut sessions = Vec::new();
    for summary in summaries {
        sessions.push(application.session_get(summary.session_id).await?);
    }

    let vpn_probe = tokio::time::timeout(Duration::from_secs(15), async {
        loop {
            match fs::read(&vpn_report_file) {
                Ok(bytes) => {
                    return serde_json::from_slice::<serde_json::Value>(&bytes)
                        .map_err(|error| Box::new(error) as Box<dyn Error + Send + Sync>);
                }
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                    tokio::time::sleep(Duration::from_millis(50)).await;
                }
                Err(error) => return Err(Box::new(error) as Box<dyn Error + Send + Sync>),
            }
        }
    })
    .await
    .map_err(|_| "timed out waiting for the Android VPN + proxy joint report")??;
    if vpn_probe.get("result").and_then(serde_json::Value::as_str) != Some("PASS") {
        return Err(format!("Android VPN + proxy joint probe failed: {vpn_probe}").into());
    }

    let baseline = session_for_path(&sessions, "/matrix/baseline")?;
    let transaction = session_for_path(&sessions, "/transaction/authorize")?;
    let direct_dll = session_for_path(&sessions, "/direct/dll")?;
    let direct_transaction = session_for_path(&sessions, "/direct/transaction")?;
    let vpn_dll = session_for_path(&sessions, "/vpn/dll")?;
    let vpn_transaction = session_for_path(&sessions, "/vpn/transaction")?;
    let baseline_response = baseline
        .response
        .as_ref()
        .ok_or("baseline has no response")?;
    if baseline_response.http_status != Some(200) || baseline_response.body_bytes != baseline_body {
        return Err("baseline Shift-JIS response did not match the upstream fixture".into());
    }
    let header_order = baseline_response
        .raw_headers
        .iter()
        .filter(|header| header.name_bytes.eq_ignore_ascii_case(b"x-header-order"))
        .map(|header| header.value_bytes.as_slice())
        .collect::<Vec<_>>();
    if header_order != [b"first".as_slice(), b"second".as_slice()] {
        return Err(format!("baseline raw header order differs: {header_order:?}").into());
    }
    let transaction_response = transaction
        .response
        .as_ref()
        .ok_or("transaction has no response")?;
    if transaction_response.http_status != Some(200)
        || transaction_response.body_bytes != baseline_body
        || raw_header_value(transaction, "x-upstream-channel") != Some(b"transaction")
    {
        return Err("Transaction listener did not independently preserve the D48 response".into());
    }
    for (label, session, expected_channel) in [
        ("Direct DLL", direct_dll, b"dll".as_slice()),
        (
            "Direct Transaction",
            direct_transaction,
            b"transaction".as_slice(),
        ),
        ("VPN DLL", vpn_dll, b"dll".as_slice()),
        (
            "VPN Transaction",
            vpn_transaction,
            b"transaction".as_slice(),
        ),
    ] {
        let response = session
            .response
            .as_ref()
            .ok_or_else(|| format!("{label} has no response"))?;
        if response.http_status != Some(200)
            || response.body_bytes != baseline_body
            || raw_header_value(session, "x-upstream-channel") != Some(expected_channel)
        {
            return Err(
                format!("{label} did not preserve D48 through the impaired VPN path").into(),
            );
        }
    }

    let response_modified = session_for_path(&sessions, "/matrix/response-modify")?;
    let response_modified_content = response_modified
        .response
        .as_ref()
        .ok_or("response-modify has no response")?;
    if response_modified_content.http_status != Some(503)
        || response_modified_content.body_bytes != response_rule_body
        || raw_header_value(response_modified, "x-response-rule") != Some(b"applied")
    {
        return Err("response rule was not observable in the captured session".into());
    }

    let mock_session = session_for_path(&sessions, "/matrix/mock")?;
    let mock_response = mock_session
        .response
        .as_ref()
        .ok_or("mock has no response")?;
    if mock_response.http_status != Some(202)
        || mock_response.body_bytes != b"{\"result\":\"MOCK\"}"
    {
        return Err("mock response was not captured as configured".into());
    }

    let nth_sessions = sessions
        .iter()
        .filter(|session| session.summary.target.contains("/matrix/nth-hit"))
        .collect::<Vec<_>>();
    if nth_sessions.len() != 3
        || nth_sessions
            .iter()
            .filter(|session| raw_header_value(session, "x-nth-hit") == Some(b"second-only"))
            .count()
            != 1
    {
        return Err("Nth-hit one-shot rule did not apply exactly once".into());
    }
    let nth_rule = application.rule_get(nth_hit_rule.summary.rule_id).await?;
    if nth_rule.summary.hit_count != 1 || nth_rule.summary.enabled {
        return Err(format!("one-shot state was not persisted: {:?}", nth_rule.summary).into());
    }

    let delay_session = session_for_path(&sessions, "/matrix/delay")?;
    if delay_session.summary.duration_ms.unwrap_or(0) < 200 {
        return Err(format!(
            "delay was not visible in session timing: {:?}",
            delay_session.timings_ms
        )
        .into());
    }

    let dll_upstream_requests = dll_upstream_task.await??;
    let transaction_upstream_requests = transaction_upstream_task.await??;
    let request_modified = dll_upstream_requests
        .iter()
        .find(|request| request.path == "/matrix/request-modify")
        .ok_or("modified request did not reach upstream")?;
    if !request_modified
        .head
        .lines()
        .any(|line| line.eq_ignore_ascii_case("x-request-rule: applied"))
        || serde_json::from_slice::<serde_json::Value>(&request_modified.body)?["amount"] != 200
    {
        return Err(
            format!("upstream did not observe request modification: {request_modified:?}").into(),
        );
    }
    if dll_upstream_requests
        .iter()
        .any(|request| request.path == "/matrix/mock" || request.path == "/matrix/disconnect")
    {
        return Err("mock or disconnect-before-upstream unexpectedly contacted upstream".into());
    }
    if transaction_upstream_requests.len() != 3
        || !transaction_upstream_requests
            .iter()
            .any(|request| request.path == "/transaction/authorize")
        || !transaction_upstream_requests
            .iter()
            .any(|request| request.path == "/direct/transaction")
        || !transaction_upstream_requests
            .iter()
            .any(|request| request.path == "/vpn/transaction")
    {
        return Err(format!(
            "Transaction request reached wrong upstream path: {transaction_upstream_requests:?}"
        )
        .into());
    }

    let capture = application.capture_query(capture_query()).await?;
    if capture.rows.len() < EXPECTED_SESSION_COUNT {
        return Err(format!("capture query exposed only {} rows", capture.rows.len()).into());
    }

    workspace = application.workspace_get(workspace.id).await?;
    application
        .listener_stop(workspace.id, workspace.revision.get(), dll_listener_id)
        .await?;
    workspace = application.workspace_get(workspace.id).await?;
    application
        .listener_stop(
            workspace.id,
            workspace.revision.get(),
            transaction_listener_id,
        )
        .await?;
    host.shutdown().await?;

    if let Some(parent) = report_file.parent() {
        fs::create_dir_all(parent)?;
    }
    let scenario = |path: &str| -> Result<serde_json::Value, Box<dyn Error + Send + Sync>> {
        let matching = sessions
            .iter()
            .filter(|session| session.summary.target.contains(path))
            .collect::<Vec<_>>();
        Ok(serde_json::json!({
            "path": path,
            "session_count": matching.len(),
            "results": matching.iter().map(|session| &session.summary.result).collect::<Vec<_>>(),
            "statuses": matching.iter().map(|session| session.summary.http_status).collect::<Vec<_>>(),
            "matched_rule_ids": matching.iter().map(|session| &session.summary.matched_rule_ids).collect::<Vec<_>>(),
            "rule_traces": matching.iter().map(|session| &session.rule_trace).collect::<Vec<_>>(),
            "durations_ms": matching.iter().map(|session| session.summary.duration_ms).collect::<Vec<_>>(),
        }))
    };
    let report = serde_json::json!({
        "result": "PASS",
        "scope": "TEST ONLY simulated upstream; not A920MAX or real GMO-FG D48 evidence",
        "android_endpoints": {
            "dll": format!("127.0.0.1:{DLL_DEVICE_PORT}"),
            "transaction": format!("127.0.0.1:{TRANSACTION_DEVICE_PORT}"),
        },
        "adb_reverse": {
            "dll": format!("tcp:{DLL_DEVICE_PORT} -> tcp:{host_listener_port}"),
            "transaction": format!("tcp:{TRANSACTION_DEVICE_PORT} -> tcp:{}", host_listener_port + 1),
        },
        "profile": "InterceptProxyProfile",
        "workspace": "dynamically created empty Workspace",
        "listener_ids": {
            "dll": dll_listener_id.to_string(),
            "transaction": transaction_listener_id.to_string(),
        },
        "upstream_fixtures": {
            "dll": dll_upstream_address.to_string(),
            "transaction": transaction_upstream_address.to_string(),
        },
        "session_count": sessions.len(),
        "capture_row_count": capture.rows.len(),
        "scenarios": {
            "baseline_shift_jis_d48": scenario("/matrix/baseline")?,
            "transaction_shift_jis_d48": scenario("/transaction/authorize")?,
            "direct_dll_shift_jis_d48": scenario("/direct/dll")?,
            "direct_transaction_shift_jis_d48": scenario("/direct/transaction")?,
            "vpn_impaired_dll_shift_jis_d48": scenario("/vpn/dll")?,
            "vpn_impaired_transaction_shift_jis_d48": scenario("/vpn/transaction")?,
            "request_header_and_json_modification": scenario("/matrix/request-modify")?,
            "response_status_header_body_modification": scenario("/matrix/response-modify")?,
            "mock_response_without_upstream": scenario("/matrix/mock")?,
            "nth_hit_one_shot": scenario("/matrix/nth-hit")?,
            "delay": scenario("/matrix/delay")?,
            "truncate": scenario("/matrix/truncate")?,
            "drop_response": scenario("/matrix/drop")?,
            "disconnect_before_upstream": scenario("/matrix/disconnect")?,
        },
        "baseline": {
            "http_status": baseline_response.http_status,
            "body_length": baseline_response.body_bytes.len(),
            "body_bytes_equal_upstream": baseline_response.body_bytes == baseline_body,
            "decoded_body_contains_d48": baseline_response.body_text.as_deref().is_some_and(|text| text.contains("D48")),
            "raw_header_order": header_order.iter().map(|value| String::from_utf8_lossy(value)).collect::<Vec<_>>(),
        },
        "request_modification_observed_upstream": {
            "head": request_modified.head,
            "body": String::from_utf8_lossy(&request_modified.body),
        },
        "one_shot_persisted": {
            "hit_count": nth_rule.summary.hit_count,
            "enabled_after_hit": nth_rule.summary.enabled,
        },
        "vpn_joint_probe": vpn_probe,
        "upstream_paths": {
            "dll": dll_upstream_requests.iter().map(|request| &request.path).collect::<Vec<_>>(),
            "transaction": transaction_upstream_requests.iter().map(|request| &request.path).collect::<Vec<_>>(),
        },
    });
    fs::write(report_file, serde_json::to_vec_pretty(&report)?)?;
    println!(
        "EMULATOR_PROXY_GATE_PASS {}",
        serde_json::to_string(&report)?
    );
    Ok(())
}
