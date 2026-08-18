use super::*;

#[tokio::test]
#[expect(
    clippy::too_many_lines,
    reason = "single real-TCP contract proves all four ordered stages and both schemas"
)]
async fn real_tcp_relay_applies_all_four_rule_stages_in_order_with_directional_schemas() {
    let upstream = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let upstream_port = upstream.local_addr().unwrap().port();
    let listener_port = reserve_port().await;
    let listener = listener(listener_port, upstream_port);
    let (workspace, rule_ids) = four_stage_workspace(&listener);
    let upstream_task = tokio::spawn(async move {
        let (mut stream, _) = upstream.accept().await.unwrap();
        let mut request = [0_u8; 2];
        stream.read_exact(&mut request).await.unwrap();
        assert_eq!(request, [161, 31]);
        stream.write_all(&[2, 22]).await.unwrap();
        stream.shutdown().await.unwrap();
    });

    let store = Arc::new(SqliteStore::in_memory().unwrap());
    let packages = Arc::new(ProtocolPackageRepositoryAdapter::with_default_limits(
        Arc::clone(&store),
    ));
    packages.install_zip(&four_stage_package_zip()).unwrap();
    packages.set_enabled(&package(), true).unwrap();
    let captures = Arc::new(crate::adapters::SocketCaptureRepositoryAdapter::new(
        Arc::clone(&store),
    ));
    let runtime = test_listener_runtime_with_packages(store, packages);
    runtime.set_socket_capture_repository(Arc::clone(&captures));
    runtime.start(workspace, listener.clone()).await.unwrap();

    let mut client = TcpStream::connect(("127.0.0.1", listener_port))
        .await
        .unwrap();
    client.write_all(&[2, 11]).await.unwrap();
    client.shutdown().await.unwrap();
    let mut response = Vec::new();
    client.read_to_end(&mut response).await.unwrap();
    assert_eq!(response, [209, 42]);
    upstream_task.await.unwrap();

    let page = wait_for_two(&captures).await;
    let records = page
        .rows
        .into_iter()
        .map(|row| captures.get_detail(row.capture_id).unwrap().record)
        .collect::<Vec<_>>();
    let upstream = records
        .iter()
        .find_map(|record| match &record.payload {
            SocketCapturePayload::RelayFrame(frame)
                if frame.direction == ProtocolDirection::Upstream =>
            {
                Some(frame)
            }
            _ => None,
        })
        .unwrap();
    assert_eq!(upstream.schema.id.as_str(), "upstream-message");
    assert_eq!(upstream.schema.version, 11);
    assert_eq!(upstream.origin, [2, 11]);
    assert_eq!(upstream.written, [161, 31]);
    assert_eq!(upstream.stages.len(), 2);
    assert_eq!(upstream.stages[0].stage, ProtocolRuleStage::AppToProxy);
    assert_eq!(upstream.stages[1].stage, ProtocolRuleStage::ProxyToUpstream);
    assert_eq!(
        upstream.stages[0].document.get("amount").unwrap(),
        &SocketCaptureDocumentValue::Int(SocketCaptureInteger::from_i64(21))
    );
    assert_eq!(upstream.stages[0].matched_rule_ids, rule_ids[..1]);
    assert_eq!(
        upstream.stages[1].document.get("amount").unwrap(),
        &SocketCaptureDocumentValue::Int(SocketCaptureInteger::from_i64(31))
    );
    assert_eq!(upstream.stages[1].matched_rule_ids, rule_ids[1..2]);
    assert_eq!(
        upstream.display,
        SocketDisplayResult::UntrustedHtml {
            html: "<p>upstream</p>".to_owned()
        }
    );

    let downstream = records
        .iter()
        .find_map(|record| match &record.payload {
            SocketCapturePayload::RelayFrame(frame)
                if frame.direction == ProtocolDirection::Downstream =>
            {
                Some(frame)
            }
            _ => None,
        })
        .unwrap();
    assert_eq!(downstream.schema.id.as_str(), "downstream-message");
    assert_eq!(downstream.schema.version, 22);
    assert_eq!(downstream.origin, [2, 22]);
    assert_eq!(downstream.written, [209, 42]);
    assert_eq!(downstream.stages.len(), 2);
    assert_eq!(
        downstream.stages[0].stage,
        ProtocolRuleStage::UpstreamToProxy
    );
    assert_eq!(downstream.stages[1].stage, ProtocolRuleStage::ProxyToApp);
    assert_eq!(
        downstream.stages[0].document.get("amount").unwrap(),
        &SocketCaptureDocumentValue::Int(SocketCaptureInteger::from_i64(32))
    );
    assert_eq!(downstream.stages[0].matched_rule_ids, rule_ids[2..3]);
    assert_eq!(
        downstream.stages[1].document.get("amount").unwrap(),
        &SocketCaptureDocumentValue::Int(SocketCaptureInteger::from_i64(42))
    );
    assert_eq!(downstream.stages[1].matched_rule_ids, rule_ids[3..]);
    assert_eq!(
        downstream.display,
        SocketDisplayResult::UntrustedHtml {
            html: "<p>downstream</p>".to_owned()
        }
    );

    runtime.stop(listener.id).await.unwrap();
}

fn four_stage_workspace(listener: &ProxyListener) -> (ProxyWorkspace, [ProtocolDocumentRuleId; 4]) {
    let specifications = [
        (ProtocolRuleStage::AppToProxy, 11, 21),
        (ProtocolRuleStage::ProxyToUpstream, 21, 31),
        (ProtocolRuleStage::UpstreamToProxy, 22, 32),
        (ProtocolRuleStage::ProxyToApp, 32, 42),
    ];
    let mut rules = Vec::new();
    let mut rule_ids = Vec::new();
    for (index, (stage, expected, replacement)) in specifications.into_iter().enumerate() {
        let rule_id = ProtocolDocumentRuleId::new();
        rules.push(
            ProtocolDocumentRuleDefinition::new_named_for_stage(
                rule_id,
                format!("four-stage-{stage:?}"),
                true,
                10,
                index as u64 + 1,
                listener.id,
                package(),
                match stage.direction() {
                    ProtocolDirection::Upstream => 11,
                    ProtocolDirection::Downstream => 22,
                },
                stage,
                vec![DocumentCondition::Equals {
                    field: DocumentFieldName::new("amount").unwrap(),
                    value: DocumentValue::Int(expected),
                }],
                vec![DocumentAction::SetField {
                    field: DocumentFieldName::new("amount").unwrap(),
                    value: DocumentValue::Int(replacement),
                }],
            )
            .unwrap(),
        );
        rule_ids.push(rule_id);
    }
    (
        ProxyWorkspace {
            listeners: vec![listener.clone()],
            protocol_rule_created_order_high_water: 4,
            protocol_rules: rules,
            ..ProxyWorkspace::default()
        },
        rule_ids.try_into().unwrap(),
    )
}

fn four_stage_package_zip() -> Vec<u8> {
    const MANIFEST: &str = r#"
api = 1

[package]
id = "runtime-matrix"
name = "Four Stage Runtime Matrix"
version = "1.0.0"

[document.upstream]
schema = "upstream.toml"
display = "display"

[document.downstream]
schema = "downstream.toml"
display = "display"

[hooks.upstream]
frame = "frame"
decode = "decode"
encode = "encode"

[hooks.downstream]
frame = "frame"
decode = "decode"
encode = "encode"
"#;
    const UPSTREAM_SCHEMA: &str = r#"
id = "upstream-message"
version = 11
title = "Upstream Message"

[[fields]]
name = "amount"
label = "Amount"
type = "int"
"#;
    const DOWNSTREAM_SCHEMA: &str = r#"
id = "downstream-message"
version = 22
title = "Downstream Message"

[[fields]]
name = "amount"
label = "Amount"
type = "int"
"#;
    const SCRIPT: &str = r#"
fn frame(reader, context) {
    if reader.available() < 2 { framing::need_more(2) } else { framing::complete(2) }
}

fn decode(origin, context) {
    let result = document::create();
    result.set("amount", origin[1]);
    result
}

fn encode(origin, document, context) {
    let result = origin;
    result[0] = if context.direction() == "upstream" { 161 } else { 209 };
    result[1] = document.get("amount");
    result
}
"#;
    const DISPLAY: &str = r#"
fn display(document, context) {
    if context.direction() == "upstream" { "<p>upstream</p>" } else { "<p>downstream</p>" }
}
"#;

    let mut writer = ZipWriter::new(Cursor::new(Vec::new()));
    for (path, contents) in [
        ("manifest.toml", MANIFEST.as_bytes()),
        ("upstream.toml", UPSTREAM_SCHEMA.as_bytes()),
        ("downstream.toml", DOWNSTREAM_SCHEMA.as_bytes()),
        ("protocol.rhai", SCRIPT.as_bytes()),
        ("display.rhai", DISPLAY.as_bytes()),
    ] {
        writer
            .start_file(path, SimpleFileOptions::default())
            .unwrap();
        writer.write_all(contents).unwrap();
    }
    writer.finish().unwrap().into_inner()
}
