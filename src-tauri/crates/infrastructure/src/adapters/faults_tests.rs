use super::*;

#[derive(Debug)]
struct TestBodyCodec {
    reject_marker: Option<&'static str>,
}

impl BodyCodec for TestBodyCodec {
    fn id(&self) -> &'static str {
        "test"
    }

    fn name(&self) -> &'static str {
        "Test Codec"
    }

    fn decode(&self, bytes: &[u8]) -> Result<String, intercept_proxy_product_api::ProductError> {
        std::str::from_utf8(bytes)
            .map(str::to_owned)
            .map_err(|error| {
                intercept_proxy_product_api::ProductError::new(
                    "BODY_DECODE_FAILED",
                    error.to_string(),
                )
            })
    }

    fn encode(&self, text: &str) -> Result<Vec<u8>, intercept_proxy_product_api::ProductError> {
        if self
            .reject_marker
            .is_some_and(|marker| text.contains(marker))
        {
            return Err(intercept_proxy_product_api::ProductError::new(
                "BODY_ENCODE_FAILED",
                "test codec rejected marker",
            ));
        }
        Ok(text.as_bytes().to_vec())
    }
}

#[test]
fn required_terminal_faults_use_domain_compatible_stages() {
    let definitions = generic_template_definitions();
    let ids = definitions
        .iter()
        .map(|definition| definition.view.template_id.as_str())
        .collect::<Vec<_>>();
    assert_eq!(
        ids,
        intercept_proxy_product_api::STANDARD_FAULT_CAPABILITY_IDS,
        "product-api capability contract and infrastructure actions must stay aligned"
    );
    for required in [
        "drop_upstream_response",
        "upstream_connect_timeout",
        "upstream_write_timeout",
        "upstream_read_timeout",
        "throttle_upstream",
        "throttle_downstream",
        "jitter_upstream",
        "jitter_downstream",
        "intermittent_upstream",
        "intermittent_downstream",
        "disconnect_upstream_mid_body",
        "disconnect_downstream_mid_body",
    ] {
        assert!(ids.contains(&required), "missing template {required}");
    }
    assert_eq!(
        drop_response(&BTreeMap::from([(
            "close_after_request_write".into(),
            FaultParameterValue::Boolean(false),
        )]))
        .expect("drop")
        .0,
        MessageStage::Request
    );
    assert_eq!(
        write_timeout(&BTreeMap::from([(
            "milliseconds".into(),
            FaultParameterValue::Integer(70_000),
        )]))
        .expect("write")
        .0,
        MessageStage::Request
    );
    assert_eq!(
        read_timeout(&BTreeMap::from([(
            "milliseconds".into(),
            FaultParameterValue::Integer(70_000),
        )]))
        .expect("read")
        .0,
        MessageStage::Request
    );
}

#[test]
fn empty_product_catalog_exposes_the_complete_generic_catalog() {
    let templates = template_definitions(&[]).expect("generic fault catalog");
    let ids = templates
        .iter()
        .map(|definition| definition.view.template_id.as_str())
        .collect::<Vec<_>>();

    assert_eq!(
        ids,
        intercept_proxy_product_api::STANDARD_FAULT_CAPABILITY_IDS
    );
    assert!(templates.iter().all(|definition| {
        definition.view.default_channel
            == intercept_proxy_domain::ChannelId::new("default").unwrap()
    }));
}

#[test]
fn mock_keeps_text_while_invalid_json_uses_injected_codec() {
    let codec = TestBodyCodec {
        reject_marker: Some("🧪"),
    };
    let mock_parameters = BTreeMap::from([
        ("status".into(), FaultParameterValue::Integer(200)),
        (
            "body".into(),
            FaultParameterValue::Json("{\"結果\":\"成功\"}".into()),
        ),
    ]);
    let (_, mock) = mock_response(&mock_parameters, &codec).expect("mock");
    let HttpAction::Terminal(TerminalAction::MockResponse { body, .. }) = mock else {
        panic!("mock response action");
    };
    assert_eq!(body, "{\"結果\":\"成功\"}");

    let invalid_parameters = BTreeMap::from([(
        "body".into(),
        FaultParameterValue::Text("{\"結果\":".into()),
    )]);
    let (_, invalid) = invalid_json(&invalid_parameters, &codec).expect("invalid");
    let HttpAction::Terminal(TerminalAction::InvalidJson { body_bytes }) = invalid else {
        panic!("invalid json action");
    };
    assert_eq!(codec.decode(&body_bytes).expect("decode"), "{\"結果\":");

    let unencodable = BTreeMap::from([
        ("status".into(), FaultParameterValue::Integer(200)),
        (
            "body".into(),
            FaultParameterValue::Json("{\"value\":\"🧪\"}".into()),
        ),
    ]);
    let (_, unencodable_mock) =
        mock_response(&unencodable, &codec).expect("MockResponse remains text until runtime");
    let HttpAction::Terminal(TerminalAction::MockResponse { body, .. }) = unencodable_mock else {
        panic!("mock response action");
    };
    assert_eq!(body, "{\"value\":\"🧪\"}");
    assert_eq!(
        invalid_json(
            &BTreeMap::from([("body".into(), FaultParameterValue::Text("🧪{".into()),)]),
            &codec
        )
        .expect_err("strict encoding")
        .view_model
        .code,
        "BODY_ENCODE_FAILED"
    );
    assert_eq!(
        invalid_json(
            &BTreeMap::from([("body".into(), FaultParameterValue::Text("{}".into()),)]),
            &codec
        )
        .expect_err("must remain invalid")
        .view_model
        .code,
        "RULE_INVALID"
    );
}

#[test]
fn every_template_exposes_matching_typed_defaults_and_schema() {
    for definition in generic_template_definitions() {
        assert_eq!(
            definition.view.default_channel,
            intercept_proxy_domain::ChannelId::new("default").unwrap()
        );
        assert_eq!(definition.view.default_priority, 100);
        assert_eq!(
            definition.view.default_parameters.len(),
            definition.view.parameter_schema.len(),
            "{}",
            definition.view.template_id
        );
        for field in &definition.view.parameter_schema {
            let value = definition
                .view
                .default_parameters
                .get(&field.key)
                .unwrap_or_else(|| {
                    panic!(
                        "{} is missing default for {}",
                        definition.view.template_id, field.key
                    )
                });
            assert!(
                matches!(
                    (&field.kind, value),
                    (FaultParameterKind::Boolean, FaultParameterValue::Boolean(_))
                        | (FaultParameterKind::Integer, FaultParameterValue::Integer(_))
                        | (FaultParameterKind::Text, FaultParameterValue::Text(_))
                        | (FaultParameterKind::Json, FaultParameterValue::Json(_))
                ),
                "{} has mismatched default for {}",
                definition.view.template_id,
                field.key
            );
        }
    }
}

// FAULT-001~007, ACTION-001~013, TEST-FAULT:
// every visible template default must compile into the shared domain rule engine.
#[test]
fn every_template_default_produces_a_domain_valid_action_for_its_declared_stage() {
    let codec = TestBodyCodec {
        reject_marker: None,
    };
    for definition in generic_template_definitions() {
        let (stage, action) = definition
            .action
            .invoke(&definition.view.default_parameters, &codec)
            .unwrap_or_else(|error| {
                panic!(
                    "{} default parameters failed: {error}",
                    definition.view.template_id
                )
            });
        let domain_stage = rule_stage(stage).unwrap_or_else(|error| {
            panic!(
                "{} default targets a non-rule stage: {error}",
                definition.view.template_id
            )
        });
        let draft = intercept_proxy_domain::RuleDefinitionDraft {
            name: definition.view.name.clone(),
            enabled: true,
            priority: definition.view.default_priority,
            listener_id: intercept_proxy_domain::ListenerId::new(),
            stage: domain_stage,
            content: intercept_proxy_domain::RuleContent::Http(
                intercept_proxy_domain::HttpRuleContent {
                    description: definition.view.behavior_text.clone(),
                    condition: intercept_proxy_domain::Condition::Http {
                        field: intercept_proxy_domain::MatchField::Method,
                        operator: intercept_proxy_domain::MatchOperator::Equals("GET".into()),
                    },
                    action: match action {
                        intercept_proxy_domain::HttpAction::Terminal(action) => {
                            intercept_proxy_domain::UnifiedAction::Terminal(action)
                        }
                        action => intercept_proxy_domain::UnifiedAction::Http(action),
                    },
                },
            ),
        };
        intercept_proxy_domain::RuleDefinition::create(draft, 1).unwrap_or_else(|error| {
            panic!(
                "{} default does not produce a valid domain rule: {error}",
                definition.view.template_id
            )
        });
    }
}

#[test]
fn wrong_boolean_number_and_body_types_return_stable_field_errors() {
    let codec = TestBodyCodec {
        reject_marker: None,
    };
    let cases = [
        (
            drop_response(&BTreeMap::from([(
                "close_after_request_write".into(),
                FaultParameterValue::Text("false".into()),
            )])),
            "parameters.close_after_request_write",
        ),
        (
            request_delay(&BTreeMap::from([(
                "milliseconds".into(),
                FaultParameterValue::Text("70000".into()),
            )])),
            "parameters.milliseconds",
        ),
        (
            mock_response(
                &BTreeMap::from([
                    ("status".into(), FaultParameterValue::Integer(200)),
                    ("body".into(), FaultParameterValue::Boolean(false)),
                ]),
                &codec,
            ),
            "parameters.body",
        ),
    ];

    for (result, expected_field) in cases {
        let error = result.expect_err("wrong parameter type must fail");
        assert_eq!(error.view_model.code, "RULE_INVALID");
        assert_eq!(error.view_model.message, "故障参数无效。");
        assert_eq!(
            error
                .view_model
                .field_errors
                .keys()
                .map(String::as_str)
                .collect::<Vec<_>>(),
            vec![expected_field]
        );
    }
}

#[test]
fn missing_required_parameter_does_not_use_a_fallback() {
    let error = request_delay(&BTreeMap::new()).expect_err("missing milliseconds");
    assert_eq!(error.view_model.code, "RULE_INVALID");
    assert_eq!(
        error.view_model.field_errors["parameters.milliseconds"],
        vec!["缺少必填参数 milliseconds。"]
    );
}
