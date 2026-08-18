use super::*;

#[tokio::test]
async fn unchanged_document_preserves_original_body_and_content_length_without_encode() {
    let (pipeline, observations, _) = pipeline(ENCODE_MUST_NOT_RUN_SCRIPT, Vec::new());
    let original_body = Bytes::from_static(b"{ \"route\" : \"wire-format\" }\n");
    let mut message = Message::from_raw_http1_head(
        b"POST /pay HTTP/1.1\r\nHost:\texample.test\r\ncOnTeNt-LeNgTh: 29 \t\r\n\r\n",
        original_body.clone(),
    )
    .unwrap();
    let original_headers = message.headers.clone();

    pipeline
        .request(&test_http_context(), &mut message)
        .await
        .unwrap();

    assert_eq!(message.body, original_body);
    assert_eq!(message.headers, original_headers);
    assert_eq!(message.declared_content_length(), Some(29));
    assert!(!message.body_modified);
    let records = observations.records();
    assert_eq!(records.len(), 1);
    assert_eq!(records[0].final_body, original_body);
    assert_eq!(records[0].observation.origin_body, original_body);
    assert_eq!(records[0].observation.written_body, original_body);
}

#[tokio::test]
async fn record_only_match_preserves_exact_body_and_headers_without_encode() {
    let listener = http_listener();
    let rule_id = ProtocolDocumentRuleId::from_uuid(Uuid::from_u128(20));
    let rule = ProtocolDocumentRuleDefinition::new_named_for_stage(
        rule_id,
        "record-only".to_owned(),
        true,
        10,
        1,
        listener.id,
        http_package(),
        1,
        ProtocolRuleStage::AppToProxy,
        vec![DocumentCondition::Equals {
            field: DocumentFieldName::new("route").unwrap(),
            value: DocumentValue::String("decoded".into()),
        }],
        vec![DocumentAction::RecordMatch],
    )
    .unwrap();
    let (pipeline, observations) =
        pipeline_for_listener(ENCODE_MUST_NOT_RUN_SCRIPT, &listener, vec![rule]);
    let original_body = Bytes::from_static(b"exact-wire-body");
    let mut message = Message::from_raw_http1_head(
        b"POST /pay HTTP/1.1\r\nContent-Length:\t15\r\nX-Test: keep\r\n\r\n",
        original_body.clone(),
    )
    .unwrap();
    let original_headers = message.headers.clone();

    pipeline
        .request(&test_http_context(), &mut message)
        .await
        .unwrap();

    assert_eq!(message.body, original_body);
    assert_eq!(message.headers, original_headers);
    assert!(!message.body_modified);
    let records = observations.records();
    assert_eq!(records.len(), 1);
    assert_eq!(
        records[0].observation.stages[0].matched_rule_ids,
        vec![rule_id]
    );
}

#[tokio::test]
async fn setting_field_to_existing_value_preserves_exact_body_without_encode() {
    let listener = http_listener();
    let rule_id = ProtocolDocumentRuleId::from_uuid(Uuid::from_u128(21));
    let rule = set_string_rule(
        &listener,
        rule_id,
        ProtocolRuleStage::AppToProxy,
        1,
        "route",
        Vec::new(),
        "decoded",
    );
    let (pipeline, observations) =
        pipeline_for_listener(ENCODE_MUST_NOT_RUN_SCRIPT, &listener, vec![rule]);
    let original_body = Bytes::from_static(b"same-document-wire");
    let mut message = request_message(original_body.clone());

    pipeline
        .request(&test_http_context(), &mut message)
        .await
        .unwrap();

    assert_eq!(message.body, original_body);
    assert!(!message.body_modified);
    let records = observations.records();
    assert_eq!(records.len(), 1);
    assert_eq!(
        records[0].observation.stages[0].matched_rule_ids,
        vec![rule_id]
    );
}

#[tokio::test]
async fn request_runs_app_to_proxy_before_proxy_to_upstream_and_encodes_once() {
    let listener = http_listener();
    let first_id = ProtocolDocumentRuleId::from_uuid(Uuid::from_u128(1));
    let second_id = ProtocolDocumentRuleId::from_uuid(Uuid::from_u128(2));
    let rules = vec![
        set_string_rule(
            &listener,
            first_id,
            ProtocolRuleStage::AppToProxy,
            1,
            "route",
            Vec::new(),
            "after_app",
        ),
        set_string_rule(
            &listener,
            second_id,
            ProtocolRuleStage::ProxyToUpstream,
            2,
            "route",
            vec![DocumentCondition::Equals {
                field: DocumentFieldName::new("route").unwrap(),
                value: DocumentValue::String("after_app".into()),
            }],
            "after_proxy",
        ),
    ];
    let (pipeline, observations) = pipeline_for_listener(PIPELINE_SCRIPT, &listener, rules);
    let mut message = request_message(Bytes::from_static(b"wire"));

    pipeline
        .request(&test_http_context(), &mut message)
        .await
        .unwrap();

    assert_eq!(message.body, Bytes::from_static(b"wire|after_proxy"));
    assert_eq!(message.declared_content_length(), Some(16));
    assert!(message.body_modified);
    let records = observations.records();
    assert_eq!(records.len(), 1);
    assert_eq!(records[0].direction, ProtocolDirection::Upstream);
    assert_eq!(records[0].final_body, message.body);
    assert_eq!(records[0].observation.origin_body, b"wire");
    assert_eq!(records[0].observation.written_body, b"wire|after_proxy");
    assert_eq!(
        records[0]
            .observation
            .stages
            .iter()
            .map(|stage| (stage.stage, stage.matched_rule_ids.clone()))
            .collect::<Vec<_>>(),
        vec![
            (ProtocolRuleStage::AppToProxy, vec![first_id]),
            (ProtocolRuleStage::ProxyToUpstream, vec![second_id]),
        ]
    );
    assert_eq!(
        records[0].observation.display,
        HttpProtocolDisplayViewModel::UntrustedHtml {
            html: "<p>upstream:after_proxy</p>".into(),
        }
    );
    assert_eq!(
        records[0].observation.stages[0]
            .document
            .get("route")
            .unwrap(),
        &DocumentValue::String("after_app".into())
    );
    assert_eq!(
        records[0].observation.stages[0].display,
        HttpProtocolDisplayViewModel::UntrustedHtml {
            html: "<p>upstream:after_app</p>".into(),
        }
    );
    assert_eq!(
        records[0].observation.stages[1]
            .document
            .get("route")
            .unwrap(),
        &DocumentValue::String("after_proxy".into())
    );
    assert_eq!(
        records[0].observation.stages[1].display,
        HttpProtocolDisplayViewModel::UntrustedHtml {
            html: "<p>upstream:after_proxy</p>".into(),
        }
    );
}

#[tokio::test]
async fn response_runs_upstream_to_proxy_before_proxy_to_app_with_downstream_schema() {
    let listener = http_listener();
    let first_id = ProtocolDocumentRuleId::from_uuid(Uuid::from_u128(3));
    let second_id = ProtocolDocumentRuleId::from_uuid(Uuid::from_u128(4));
    let rules = vec![
        set_string_rule(
            &listener,
            first_id,
            ProtocolRuleStage::UpstreamToProxy,
            1,
            "result",
            Vec::new(),
            "after_server",
        ),
        set_string_rule(
            &listener,
            second_id,
            ProtocolRuleStage::ProxyToApp,
            2,
            "result",
            vec![DocumentCondition::Equals {
                field: DocumentFieldName::new("result").unwrap(),
                value: DocumentValue::String("after_server".into()),
            }],
            "after_app",
        ),
    ];
    let (pipeline, observations) = pipeline_for_listener(PIPELINE_SCRIPT, &listener, rules);
    let mut message = response_message(Bytes::from_static(b"reply"));

    pipeline
        .response(&test_http_context(), &mut message)
        .await
        .unwrap();

    assert_eq!(message.body, Bytes::from_static(b"reply|after_app"));
    assert_eq!(message.declared_content_length(), Some(15));
    assert!(message.body_modified);
    let records = observations.records();
    assert_eq!(records.len(), 1);
    assert_eq!(records[0].direction, ProtocolDirection::Downstream);
    assert_eq!(records[0].final_body, message.body);
    assert_eq!(records[0].observation.origin_body, b"reply");
    assert_eq!(records[0].observation.written_body, b"reply|after_app");
    assert_eq!(
        records[0].observation.document.schema().id().as_str(),
        "http-downstream"
    );
    assert_eq!(
        records[0]
            .observation
            .stages
            .iter()
            .map(|stage| (stage.stage, stage.matched_rule_ids.clone()))
            .collect::<Vec<_>>(),
        vec![
            (ProtocolRuleStage::UpstreamToProxy, vec![first_id]),
            (ProtocolRuleStage::ProxyToApp, vec![second_id]),
        ]
    );
    assert_eq!(
        records[0].observation.display,
        HttpProtocolDisplayViewModel::UntrustedHtml {
            html: "<p>downstream:after_app</p>".into(),
        }
    );
    assert_eq!(
        records[0].observation.stages[0]
            .document
            .get("result")
            .unwrap(),
        &DocumentValue::String("after_server".into())
    );
    assert_eq!(
        records[0].observation.stages[0].display,
        HttpProtocolDisplayViewModel::UntrustedHtml {
            html: "<p>downstream:after_server</p>".into(),
        }
    );
    assert_eq!(
        records[0].observation.stages[1]
            .document
            .get("result")
            .unwrap(),
        &DocumentValue::String("after_app".into())
    );
}

#[tokio::test]
async fn request_and_response_keep_upstream_and_downstream_schemas_isolated() {
    let (pipeline, observations, _) = pipeline(PIPELINE_SCRIPT, Vec::new());
    let context = test_http_context();
    let mut request = request_message(Bytes::from_static(b"request"));
    let mut response = response_message(Bytes::from_static(b"response"));

    pipeline.request(&context, &mut request).await.unwrap();
    pipeline.response(&context, &mut response).await.unwrap();

    let records = observations.records();
    assert_eq!(records.len(), 2);
    assert_eq!(
        records[0].observation.document.schema().id().as_str(),
        "http-upstream"
    );
    assert_eq!(
        records[1].observation.document.schema().id().as_str(),
        "http-downstream"
    );
    assert_eq!(
        records[0].observation.document.get("route").unwrap(),
        &DocumentValue::String("decoded".into())
    );
    assert_eq!(
        records[1].observation.document.get("result").unwrap(),
        &DocumentValue::String("decoded".into())
    );
}

#[tokio::test]
async fn ordinary_http_rules_run_before_protocol_body_processing_in_both_directions() {
    let listener = http_listener();
    let rules = vec![
        set_string_rule(
            &listener,
            ProtocolDocumentRuleId::from_uuid(Uuid::from_u128(31)),
            ProtocolRuleStage::AppToProxy,
            1,
            "route",
            Vec::new(),
            "protocol-request",
        ),
        set_string_rule(
            &listener,
            ProtocolDocumentRuleId::from_uuid(Uuid::from_u128(32)),
            ProtocolRuleStage::UpstreamToProxy,
            2,
            "result",
            Vec::new(),
            "protocol-response",
        ),
    ];
    let (pipeline, observations) = pipeline_for_listener_with_inner(
        PIPELINE_SCRIPT,
        &listener,
        rules,
        Arc::new(RewritingHttpPipeline {
            request_body: Bytes::from_static(b"ordinary-request"),
            response_body: Bytes::from_static(b"ordinary-response"),
        }),
    );
    let context = test_http_context();
    let mut request = request_message(Bytes::from_static(b"original-request"));
    let mut response = response_message(Bytes::from_static(b"original-response"));

    pipeline.request(&context, &mut request).await.unwrap();
    pipeline.response(&context, &mut response).await.unwrap();

    assert_eq!(
        request.body,
        Bytes::from_static(b"ordinary-request|protocol-request")
    );
    assert_eq!(
        response.body,
        Bytes::from_static(b"ordinary-response|protocol-response")
    );
    let records = observations.records();
    assert_eq!(records.len(), 2);
    assert_eq!(records[0].observation.origin_body, b"ordinary-request");
    assert_eq!(records[1].observation.origin_body, b"ordinary-response");
}
