use super::*;

pub(in crate::requirements_tests) fn test_channel(id: &str) -> ChannelId {
    ChannelId::new(id).unwrap()
}

pub(in crate::requirements_tests) fn valid_settings_draft() -> SettingsDraft {
    SettingsDraft {
        channels: vec![
            ChannelSettingsDraft {
                id: test_channel("alpha"),
                display_name: "Alpha".into(),
                enabled: true,
                port: 20_001,
                upstream_url: "https://alpha.example.test".into(),
            },
            ChannelSettingsDraft {
                id: test_channel("beta"),
                display_name: "Beta".into(),
                enabled: true,
                port: 20_002,
                upstream_url: "https://beta.example.test".into(),
            },
        ],
        ..SettingsDraft::default()
    }
}

pub(in crate::requirements_tests) fn timestamp(second: u32) -> DateTime<Utc> {
    Utc.with_ymd_and_hms(2026, 7, 28, 0, 0, second)
        .single()
        .expect("valid test time")
}

pub(in crate::requirements_tests) fn content(body: &[u8]) -> MessageContentViewModel {
    MessageContentViewModel {
        http_status: None,
        start_line_bytes: Vec::new(),
        raw_headers: Vec::new(),
        headers: BTreeMap::from([("content-type".into(), vec!["application/json".into()])]),
        body_text: Some(String::from_utf8_lossy(body).into_owned()),
        body_bytes: body.to_vec(),
        json: None,
        content_length: body.len(),
        media_type: Some("application/json".into()),
        charset: None,
        content_kind: MessageContentKind::Json,
        codec_id: Some("utf-8".into()),
        decode_error: None,
        query_string: None,
        protocol: None,
        protocol_failure: None,
    }
}

pub(in crate::requirements_tests) fn session(
    id: SessionId,
    second: u32,
    pending: bool,
    body: &[u8],
) -> SessionRecord {
    SessionRecord {
        detail: SessionDetailViewModel {
            summary: SessionSummaryViewModel {
                session_id: id,
                request_id: format!("REQ-{second}"),
                started_at: timestamp(second),
                completed_at: (!pending).then(|| timestamp(second)),
                terminal_ip: format!("10.0.0.{second}"),
                channel: test_channel("alpha"),
                channel_text: "Alpha".into(),
                method: "POST".into(),
                target: "/payment".into(),
                http_status: None,
                result: "成功".into(),
                ui_tone: UiTone::Positive,
                duration_ms: Some(u64::from(second)),
                matched_rule_ids: Vec::new(),
                request_size_bytes: body.len() as u64,
                response_size_bytes: 0,
                revision: 1,
            },
            runtime_epoch: Uuid::nil(),
            connection_id: "connection".into(),
            certificate_fingerprint: "fingerprint".into(),
            upstream_host: "example.test".into(),
            app_to_proxy_tls: "TLS 1.2".into(),
            proxy_to_server_tls: "TLS 1.2".into(),
            final_action: "转发".into(),
            timings_ms: BTreeMap::new(),
            request: Some(content(body)),
            response: None,
            rule_trace: vec!["规则轨迹".into()],
        },
    }
}

pub(in crate::requirements_tests) fn capture_row(event_id: u64) -> CaptureRowViewModel {
    CaptureRowViewModel {
        event_id,
        runtime_epoch: Uuid::nil(),
        session_id: Uuid::new_v4(),
        occurred_at: timestamp(0),
        terminal_ip: "10.0.0.1".into(),
        channel: test_channel("alpha"),
        channel_text: "交易".into(),
        stage: MessageStage::Request,
        stage_text: "请求".into(),
        method: "POST".into(),
        target: "/payment".into(),
        http_status: None,
        result: "成功".into(),
        ui_tone: UiTone::Positive,
        duration_ms: Some(1),
        matched_rule_ids: Vec::new(),
        size_bytes: 1,
    }
}

pub(in crate::requirements_tests) fn protocol_package(
    id: &str,
    version: &str,
) -> ProtocolPackageRef {
    ProtocolPackageRef {
        id: ProtocolPackageId::new(id).unwrap(),
        version: ProtocolPackageVersion::new(version).unwrap(),
    }
}

pub(in crate::requirements_tests) fn portable_protocol_package(
    package: ProtocolPackageRef,
    enabled: bool,
) -> PortableApplicationProtocolPackage {
    PortableApplicationProtocolPackage {
        package,
        files: vec![PortableProtocolPackageFile {
            path: "manifest.json".into(),
            contents_base64: "bWFuaWZlc3Q=".into(),
        }],
        enabled,
    }
}

pub(in crate::requirements_tests) fn protocol_package_description(
    package: ProtocolPackageRef,
) -> ProtocolPackageDescriptionViewModel {
    ProtocolPackageDescriptionViewModel {
        package,
        kind: ProtocolPackageKindViewModel::Socket,
        capabilities: ProtocolPackageCapabilitiesViewModel {
            upstream: ProtocolPackageDirectionCapabilitiesViewModel {
                frame: true,
                decode: true,
                encode: true,
            },
            downstream: ProtocolPackageDirectionCapabilitiesViewModel {
                frame: true,
                decode: true,
                encode: true,
            },
            display: true,
        },
        upstream_schema: Some(portable_schema("Portable Message")),
        downstream_schema: Some(portable_schema("Portable Response")),
    }
}

fn portable_schema(title: &str) -> ProtocolPackageSchemaViewModel {
    ProtocolPackageSchemaViewModel {
        root: intercept_proxy_domain::DocumentSchemaNode::Object {
            title: Some(title.to_owned()),
            properties: std::collections::BTreeMap::from([
                (
                    "text".to_owned(),
                    intercept_proxy_domain::DocumentSchemaNode::String { title: None },
                ),
                (
                    "amount".to_owned(),
                    intercept_proxy_domain::DocumentSchemaNode::Number { title: None },
                ),
                (
                    "approved".to_owned(),
                    intercept_proxy_domain::DocumentSchemaNode::Boolean { title: None },
                ),
                (
                    "raw".to_owned(),
                    intercept_proxy_domain::DocumentSchemaNode::Array {
                        title: None,
                        items: Box::new(intercept_proxy_domain::DocumentSchemaNode::Number {
                            title: None,
                        }),
                    },
                ),
            ]),
        },
    }
}

pub(in crate::requirements_tests) fn scripted_workspace(
    package: ProtocolPackageRef,
    local_responder: bool,
) -> ProxyWorkspace {
    let mut workspace = ProxyWorkspace::default();
    let listener = &mut workspace.listeners[0];
    let topology = if local_responder {
        SocketTopology::LocalResponder(SocketLocalResponderTopology {
            downstream_security: SocketDownstreamSecurity::Tcp,
        })
    } else {
        SocketTopology::Relay(SocketRelayTopology {
            upstream: SocketEndpoint {
                host: "127.0.0.1".into(),
                port: 9_001,
            },
            security: SocketRelaySecurity::Transparent,
        })
    };
    listener.data_plane = ListenerDataPlane::Socket(SocketRelaySettings {
        topology,
        maximum_connections: 8,
        runtime_limits: intercept_proxy_domain::SocketRuntimeLimits::default(),
        processing: SocketPayloadProcessing::Scripted(ScriptedSocketProcessing {
            package: package.clone(),
        }),
    });
    let listener_id = listener.id;
    let stage = if local_responder {
        RuleStage::ProxyToApp
    } else {
        RuleStage::ProxyToUpstream
    };
    workspace.rule_definitions = vec![
        RuleDefinition::create(
            RuleDefinitionDraft {
                name: "Scripted fixture rule".into(),
                enabled: true,
                priority: -10,
                listener_id,
                stage,
                one_shot: false,
                content: RuleContent::Socket(intercept_proxy_domain::SocketRuleContent {
                    package,
                    conditions: vec![
                        document_equals("text", DocumentValue::String("sale".into())),
                        document_equals("amount", DocumentValue::integer(1234).unwrap()),
                        document_equals("approved", DocumentValue::Boolean(true)),
                    ],
                    actions: vec![
                        UnifiedAction::RecordMatch,
                        document_set("text", DocumentValue::String("reply".into())),
                        document_set("amount", DocumentValue::integer(4321).unwrap()),
                        document_set("approved", DocumentValue::Boolean(false)),
                        document_set("raw", DocumentValue::byte_array(vec![9, 8, 7])),
                    ],
                }),
            },
            41,
        )
        .unwrap(),
    ];
    workspace.rule_created_order_high_water = 41;
    workspace.validate().unwrap();
    workspace
}

pub(in crate::requirements_tests) fn http_rule_definitions(
    workspace: &ProxyWorkspace,
) -> Vec<&intercept_proxy_domain::RuleDefinition> {
    workspace
        .rule_definitions
        .iter()
        .filter(|rule| {
            matches!(
                rule.content(),
                intercept_proxy_domain::RuleContent::Http(content)
                    if !intercept_proxy_domain::contains_document_condition(&content.conditions)
                        && !content.actions.iter().any(|action| matches!(
                            action,
                            intercept_proxy_domain::UnifiedAction::Document(_)
                        ))
            )
        })
        .collect()
}

pub(in crate::requirements_tests) fn protocol_rule_definitions(
    workspace: &ProxyWorkspace,
) -> Vec<&intercept_proxy_domain::RuleDefinition> {
    workspace
        .rule_definitions
        .iter()
        .filter(|rule| {
            !matches!(
                rule.content(),
                intercept_proxy_domain::RuleContent::Http(content)
                    if !intercept_proxy_domain::contains_document_condition(&content.conditions)
                        && !content.actions.iter().any(|action| matches!(
                            action,
                            intercept_proxy_domain::UnifiedAction::Document(_)
                        ))
            )
        })
        .collect()
}

fn document_equals(name: &str, value: DocumentValue) -> Condition {
    Condition::Document {
        path: JsonPointer::property(name),
        predicate: match value {
            DocumentValue::String(value) => DocumentPredicate::String(StringPredicate {
                operator: StringOperator::Equal,
                value,
            }),
            DocumentValue::Number(value) => DocumentPredicate::Number(NumberPredicate {
                operator: NumberOperator::Equal,
                value,
            }),
            DocumentValue::Boolean(value) => {
                DocumentPredicate::Boolean(BooleanPredicate::Equal(value))
            }
            DocumentValue::Null(()) => DocumentPredicate::NullEqual,
            DocumentValue::Object(_) | DocumentValue::Array(_) => {
                panic!("fixture equality requires a scalar value")
            }
        },
    }
}

fn document_set(name: &str, value: DocumentValue) -> UnifiedAction {
    UnifiedAction::Document(DocumentMutation::Set {
        path: JsonPointer::property(name),
        value,
    })
}

// DATA-008, TEST-CAPACITY: logical bytes are deterministic and use lengths, not allocations.
