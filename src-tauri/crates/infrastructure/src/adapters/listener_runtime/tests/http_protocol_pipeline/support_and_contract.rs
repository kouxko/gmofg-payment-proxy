#[tokio::test]
async fn http_and_document_conditions_gate_both_action_sets_as_one_rule() {
    let listener = http_listener();
    let rule = set_string_rule(
        &listener,
        ProtocolDocumentRuleId::from_uuid(Uuid::from_u128(6)),
        ProtocolRuleStage::ProxyToUpstream,
        1,
        "route",
        vec![DocumentCondition::Equals {
            field: JsonPointer::property("route"),
            value: DocumentValue::String("decoded".into()),
        }],
        "joint",
    );
    let (snapshot, mut workspace) = snapshot(PIPELINE_SCRIPT, &listener, vec![rule]);
    let definition = &mut workspace.rule_definitions[0];
    let mut draft = definition.to_draft();
    let RuleContent::Http(content) = &mut draft.content else {
        panic!("HTTP rule expected");
    };
    content.condition = intercept_proxy_domain::ConditionTree::from_http_conditions(vec![MatchCondition::Field {
        field: MatchField::PathOrRequestType,
        operator: MatchOperator::Equals("/allowed".into()),
    }]);
    let expected_actions = vec![RuleAction::SetHeader {
        name: "x-joint".into(),
        value: "matched".into(),
    }];
    content.actions = expected_actions.clone().into_iter().map(intercept_proxy_domain::UnifiedAction::from).collect();
    definition.update(definition.revision(), draft).unwrap();

    let identity = identity();
    let mut blocked = snapshot.create_upstream(identity.clone()).unwrap();
    let blocked_original = context("POST /blocked HTTP/1.1\r\n\r\n", "wire");
    let document = blocked.decode.decode(&blocked_original).await.unwrap();
    blocked.rules.apply(document).await.unwrap();
    let (blocked_message, blocked_evaluation) =
        execute_joint(&snapshot, &workspace, &identity, false, &blocked_original)
            .await
            .unwrap();
    assert_eq!(blocked_message.body, Bytes::from_static(b"wire|decoded"));
    assert!(blocked_evaluation.composed_actions.is_empty());
    assert!(!blocked_evaluation.traces[0].matched);

    let identity = HttpConnectionIdentity {
        connection_id: Uuid::from_u128(12),
        ..identity
    };
    let mut allowed = snapshot.create_upstream(identity.clone()).unwrap();
    let allowed_original = context("POST /allowed HTTP/1.1\r\n\r\n", "wire");
    let document = allowed.decode.decode(&allowed_original).await.unwrap();
    allowed.rules.apply(document).await.unwrap();
    let (allowed_message, allowed_evaluation) =
        execute_joint(&snapshot, &workspace, &identity, false, &allowed_original)
            .await
            .unwrap();
    assert_eq!(allowed_message.body, Bytes::from_static(b"wire|joint"));
    assert_eq!(allowed_evaluation.composed_actions, expected_actions);
    assert!(allowed_evaluation.traces[0].matched);
}

async fn execute_joint(
    snapshot: &HttpProtocolRuntimeSnapshot,
    workspace: &ProxyWorkspace,
    identity: &HttpConnectionIdentity,
    response: bool,
    original: &HttpContext,
) -> Result<(Message, RuleEvaluation), String> {
    let mut joint = snapshot
        .take_joint_evaluation(identity, response)
        .expect("joint evaluation staged by Document Rules");
    let epoch = RuntimeEpoch::from_uuid(identity.runtime_epoch);
    let terminal = TerminalIdentity {
        source_ip: "127.0.0.1".into(),
        certificate_sha256: String::new(),
    };
    let mut engine = RuleEngine::new(epoch, workspace.http_runtime_rules().unwrap());
    let target = original
        .header
        .lines()
        .next()
        .and_then(|line| line.split_ascii_whitespace().nth(1));
    let evaluation = engine
        .evaluate_with_gate_in_order(
            &MatchContext {
                runtime_epoch: epoch,
                channel: ChannelId::new(snapshot.observation_metadata().listener_id).unwrap(),
                stage: if response {
                    MessageStage::Response
                } else {
                    MessageStage::Request
                },
                terminal: &terminal,
                path_or_request_type: target,
                json_body: None,
            },
            Utc::now(),
            &workspace.http_runtime_rule_execution_order(),
            |rule| joint.gate(rule),
        )
        .map_err(|error| error.to_string())?;
    let mut message = Message::from_raw_http1_head(
        original.header.as_bytes(),
        Bytes::copy_from_slice(original.body.as_bytes()),
    )
    .map_err(|error| error.to_string())?;
    joint.encode_into(&mut message).await?;
    Ok((message, evaluation))
}

#[tokio::test]
async fn decode_failure_stays_in_decode_capability() {
    let listener = http_listener();
    let (snapshot, _) = snapshot(DECODE_FAILURE_SCRIPT, &listener, Vec::new());
    let mut capabilities = snapshot.create_upstream(identity()).unwrap();

    let error = capabilities
        .decode
        .decode(&context("POST / HTTP/1.1\r\n\r\n", "wire"))
        .await
        .unwrap_err();

    assert!(error.message.starts_with("ENTRY_POINT_FAILED\n"));
}

#[tokio::test]
async fn display_failure_is_returned_for_reader_fallback_policy() {
    let listener = http_listener();
    let (snapshot, _) = snapshot(DISPLAY_FAILURE_SCRIPT, &listener, Vec::new());
    let mut capabilities = snapshot.create_downstream(identity()).unwrap();
    let document = capabilities
        .decode
        .decode(&context("HTTP/1.1 200 OK\r\n\r\n", "reply"))
        .await
        .unwrap();

    let error = capabilities.display.display(&document).await.unwrap_err();

    assert!(error.message.starts_with("ENTRY_POINT_FAILED\n"));
}

fn snapshot(
    script: &str,
    listener: &ProxyListener,
    rules: Vec<ProtocolDocumentRuleDefinition>,
) -> (Arc<HttpProtocolRuntimeSnapshot>, ProxyWorkspace) {
    let (result, workspace) = prepare_snapshot(script, listener, rules);
    let snapshot = result.unwrap().expect("HTTP protocol snapshot");
    (snapshot, workspace)
}

fn prepare_snapshot(
    script: &str,
    listener: &ProxyListener,
    rules: Vec<ProtocolDocumentRuleDefinition>,
) -> (
    AppResult<Option<Arc<HttpProtocolRuntimeSnapshot>>>,
    ProxyWorkspace,
) {
    let store = Arc::new(SqliteStore::in_memory().unwrap());
    let packages = Arc::new(ProtocolPackageRepositoryAdapter::with_default_limits(
        Arc::clone(&store),
    ));
    packages.install_zip(&http_package_zip(script)).unwrap();
    packages.set_enabled(&http_package(), true).unwrap();
    let runtime = test_listener_runtime_with_packages(store, packages);
    let created_order_high_water = rules
        .iter()
        .map(ProtocolDocumentRuleDefinition::created_order)
        .max()
        .unwrap_or(0);
    let mut workspace = ProxyWorkspace {
        listeners: vec![listener.clone()],
        rule_created_order_high_water: created_order_high_water,
        ..ProxyWorkspace::default()
    };
    workspace.replace_document_runtime_rules(rules).unwrap();
    workspace.rule_definitions = workspace
        .rule_definitions
        .iter()
        .map(|definition| {
            let RuleContent::Socket(SocketRuleContent {
                package,
                condition,
                actions,
            }) = definition.content()
            else {
                return definition.clone();
            };
            RuleDefinition::restore(
                definition.rule_id(),
                definition.revision(),
                RuleDefinitionDraft {
                    name: definition.name().to_owned(),
                    enabled: definition.enabled(),
                    priority: definition.priority(),
                    listener_id: definition.listener_id(),
                    stage: definition.stage(),
                    content: RuleContent::Http(HttpRuleContent {
                        description: String::new(),
                        condition: condition.clone(),
                        actions: actions.clone(),
                        document: Some(intercept_proxy_domain::HttpDocumentRuleContent {
                            package: package.clone(),
                        }),
                        one_shot: false,
                        hit_count: 0,
                        last_hit_at: None,
                    }),
                },
                definition.created_order(),
            )
            .unwrap()
        })
        .collect();
    let result = HttpProtocolRuntimeSnapshot::prepare(&runtime, &workspace, listener);
    (result, workspace)
}

fn identity() -> HttpConnectionIdentity {
    HttpConnectionIdentity {
        runtime_epoch: Uuid::from_u128(10),
        connection_id: Uuid::from_u128(11),
        peer: "127.0.0.1:12345".into(),
    }
}

fn context(header: &str, body: &str) -> HttpContext {
    HttpContext {
        header: header.into(),
        body: body.into(),
        body_is_utf8: true,
    }
}

fn http_listener() -> ProxyListener {
    ProxyListener {
        data_plane: ListenerDataPlane::Http(HttpListenerSettings {
            body_processing: HttpBodyProcessing::Protocol {
                package: http_package(),
            },
            ..HttpListenerSettings::default()
        }),
        ..ProxyListener::default()
    }
}

#[allow(clippy::too_many_arguments)]
fn set_string_rule(
    listener: &ProxyListener,
    id: ProtocolDocumentRuleId,
    stage: ProtocolRuleStage,
    created_order: u64,
    field: &str,
    mut conditions: Vec<DocumentCondition>,
    value: &str,
) -> ProtocolDocumentRuleDefinition {
    if conditions.is_empty() {
        let decoded_field = match stage {
            ProtocolRuleStage::AppToProxy | ProtocolRuleStage::ProxyToUpstream => "route",
            ProtocolRuleStage::UpstreamToProxy | ProtocolRuleStage::ProxyToApp => "result",
        };
        conditions.push(DocumentCondition::Equals {
            field: JsonPointer::property(decoded_field),
            value: DocumentValue::String("decoded".into()),
        });
    }
    ProtocolDocumentRuleDefinition::new_named_for_stage(
        id,
        format!("{stage:?}"),
        true,
        10,
        created_order,
        listener.id,
        http_package(),
        stage,
        conditions,
        vec![DocumentAction::SetField {
            field: JsonPointer::property(field),
            value: DocumentValue::String(value.into()),
        }],
    )
    .unwrap()
}

fn http_package() -> ProtocolPackageRef {
    ProtocolPackageRef {
        id: ProtocolPackageId::new("http-pipeline-test").unwrap(),
        version: ProtocolPackageVersion::new("1.0.0").unwrap(),
    }
}

fn http_package_zip(script: &str) -> Vec<u8> {
    let mut writer = ZipWriter::new(Cursor::new(Vec::new()));
    for (path, contents) in [
        ("manifest.toml", HTTP_MANIFEST.as_bytes()),
        ("upstream.toml", UPSTREAM_SCHEMA.as_bytes()),
        ("downstream.toml", DOWNSTREAM_SCHEMA.as_bytes()),
        ("protocol.rhai", script.as_bytes()),
        ("display.rhai", script.as_bytes()),
    ] {
        writer
            .start_file(path, SimpleFileOptions::default())
            .unwrap();
        writer.write_all(contents).unwrap();
    }
    writer.finish().unwrap().into_inner()
}
