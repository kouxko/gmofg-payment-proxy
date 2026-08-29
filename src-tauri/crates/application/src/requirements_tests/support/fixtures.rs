use super::*;

#[derive(Debug)]
pub(in crate::requirements_tests) struct Utf8TestCodec;

impl BodyCodec for Utf8TestCodec {
    fn id(&self) -> &'static str {
        "utf-8-test"
    }

    fn name(&self) -> &'static str {
        "UTF-8 Test"
    }

    fn decode(&self, bytes: &[u8]) -> Result<String, ProductError> {
        String::from_utf8(bytes.to_vec())
            .map_err(|error| ProductError::new("BODY_DECODE_FAILED", error.to_string()))
    }

    fn encode(&self, text: &str) -> Result<Vec<u8>, ProductError> {
        Ok(text.as_bytes().to_vec())
    }
}

pub(in crate::requirements_tests) fn breakpoint_validator() -> BreakpointValidator {
    BreakpointValidator::new(Arc::new(Utf8TestCodec))
}

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
                pending_breakpoint: pending,
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
        breakpoint_draft: pending.then(|| content(b"draft")),
    }
}

pub(in crate::requirements_tests) fn breakpoint(
    id: BreakpointId,
    epoch: RuntimeEpoch,
    second: u32,
) -> BreakpointDetailViewModel {
    BreakpointDetailViewModel {
        summary: BreakpointSummaryViewModel {
            breakpoint_id: id,
            session_id: Uuid::new_v4(),
            runtime_epoch: epoch,
            stage: MessageStage::Request,
            title: "请求断点·发送至服务器前".into(),
            terminal_ip: "10.0.0.1".into(),
            channel: test_channel("alpha"),
            channel_text: "Alpha".into(),
            method: "POST".into(),
            target: "/payment".into(),
            waiting_since: timestamp(second),
            certificate_fingerprint_suffix: "A1:B2".into(),
            state: BreakpointState::Pending,
            state_text: String::new(),
            ui_tone: UiTone::Neutral,
            revision: 7,
        },
        original: content(br#"{"a":1}"#),
        effective: content(br#"{"a":1}"#),
        can_resolve: true,
        resolve_disabled_reason: None,
        available_actions: Vec::new(),
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
        breakpoint_id: None,
        can_go_to_breakpoint: false,
        breakpoint_disabled_reason: Some(DisabledReason {
            code: "NO_BREAKPOINT".into(),
            message: "该会话没有待处理断点。".into(),
        }),
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
            path: "manifest.toml".into(),
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
        upstream_schema: portable_schema("Portable Message"),
        downstream_schema: portable_schema("Portable Response"),
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
    let direction = if local_responder {
        ProtocolDirection::Downstream
    } else {
        ProtocolDirection::Upstream
    };
    workspace
        .replace_document_runtime_rules(vec![
            ProtocolDocumentRuleDefinition::new(
                ProtocolDocumentRuleId::new(),
                true,
                -10,
                41,
                listener_id,
                package,
                direction,
                vec![
                    document_equals("text", DocumentValue::String("sale".into())),
                    document_equals("amount", DocumentValue::integer(1234).unwrap()),
                    document_equals("approved", DocumentValue::Boolean(true)),
                    document_equals("raw", DocumentValue::byte_array(vec![0, 1, 2, 255])),
                ],
                vec![
                    DocumentAction::RecordMatch,
                    document_set("text", DocumentValue::String("reply".into())),
                    document_set("amount", DocumentValue::integer(4321).unwrap()),
                    document_set("approved", DocumentValue::Boolean(false)),
                    document_set("raw", DocumentValue::byte_array(vec![9, 8, 7])),
                ],
            )
            .unwrap(),
        ])
        .unwrap();
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
                intercept_proxy_domain::RuleContent::Http(content) if content.document.is_none()
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
                intercept_proxy_domain::RuleContent::Http(content) if content.document.is_none()
            )
        })
        .collect()
}

fn document_equals(name: &str, value: DocumentValue) -> DocumentCondition {
    DocumentCondition::Equals {
        field: JsonPointer::property(name),
        value,
    }
}

fn document_set(name: &str, value: DocumentValue) -> DocumentAction {
    DocumentAction::SetField {
        field: JsonPointer::property(name),
        value,
    }
}

// DATA-008, TEST-CAPACITY: logical bytes are deterministic and use lengths, not allocations.
