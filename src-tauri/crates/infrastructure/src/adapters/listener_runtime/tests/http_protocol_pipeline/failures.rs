use super::*;

#[tokio::test]
async fn empty_request_body_skips_protocol_execution_and_observation() {
    let (pipeline, observations, _) = pipeline(PIPELINE_SCRIPT, Vec::new());
    let mut message = request_message(Bytes::new());

    pipeline
        .request(&test_http_context(), &mut message)
        .await
        .unwrap();

    assert!(observations.records().is_empty());
    assert_eq!(message.body, Bytes::new());
    assert!(!message.body_modified);
}

#[tokio::test]
async fn empty_response_body_skips_protocol_execution_and_observation() {
    let (pipeline, observations, _) = pipeline(PIPELINE_SCRIPT, Vec::new());
    let mut message = response_message(Bytes::new());

    pipeline
        .response(&test_http_context(), &mut message)
        .await
        .unwrap();

    assert!(observations.records().is_empty());
    assert_eq!(message.body, Bytes::new());
    assert!(!message.body_modified);
}

#[tokio::test]
async fn non_utf8_body_is_rejected_after_persisting_failure_evidence() {
    let (pipeline, observations, _) = pipeline(PIPELINE_SCRIPT, Vec::new());
    let original = Bytes::from_static(&[0xff, 0xfe]);
    let mut message = request_message(original.clone());

    let error = pipeline
        .request(&test_http_context(), &mut message)
        .await
        .unwrap_err();

    assert_eq!(error.code, ErrorCode::ConfigInvalid.as_str());
    assert_eq!(message.body, original);
    assert!(!message.body_modified);
    assert!(observations.records().is_empty());
    let failures = observations.failures();
    assert_eq!(failures.len(), 1);
    assert_eq!(failures[0].origin_body, original);
    assert_eq!(
        failures[0].direction,
        intercept_proxy_domain::ProtocolDirection::Upstream
    );
    assert_eq!(failures[0].kind, HttpProtocolFailureKind::InputNotUtf8);
    assert_eq!(failures[0].code, "HTTP_BODY_NOT_UTF8");
    assert_eq!(failures[0].stage, None);
}

#[tokio::test]
async fn decode_failure_preserves_original_request_body_headers_and_observation_state() {
    let (pipeline, observations, _) = pipeline(DECODE_FAILURE_SCRIPT, Vec::new());
    let original_body = Bytes::from_static(b"wire");
    let mut message = request_message(original_body.clone());
    let original_headers = message.headers.clone();

    let error = pipeline
        .request(&test_http_context(), &mut message)
        .await
        .unwrap_err();

    assert_eq!(error.code, ErrorCode::Internal.as_str());
    assert!(error.message.contains("上行"));
    assert!(error.message.contains("ENTRY_POINT_FAILED"));
    assert_eq!(message.body, original_body);
    assert_eq!(message.headers, original_headers);
    assert!(!message.body_modified);
    assert!(observations.records().is_empty());
    let failures = observations.failures();
    assert_eq!(failures.len(), 1);
    assert_eq!(failures[0].origin_body, original_body);
    assert_eq!(failures[0].kind, HttpProtocolFailureKind::DecodeFailed);
    assert_eq!(failures[0].code, "ENTRY_POINT_FAILED");
    assert_eq!(failures[0].stage, None);
}

#[tokio::test]
async fn decode_failure_preserves_original_response_body_headers_and_observation_state() {
    let (pipeline, observations, _) = pipeline(DECODE_FAILURE_SCRIPT, Vec::new());
    let original_body = Bytes::from_static(b"reply");
    let mut message = response_message(original_body.clone());
    let original_headers = message.headers.clone();

    let error = pipeline
        .response(&test_http_context(), &mut message)
        .await
        .unwrap_err();

    assert_eq!(error.code, ErrorCode::Internal.as_str());
    assert!(error.message.contains("下行"));
    assert!(error.message.contains("ENTRY_POINT_FAILED"));
    assert_eq!(message.body, original_body);
    assert_eq!(message.headers, original_headers);
    assert!(!message.body_modified);
    assert!(observations.records().is_empty());
    let failures = observations.failures();
    assert_eq!(failures.len(), 1);
    assert_eq!(failures[0].origin_body, original_body);
    assert_eq!(
        failures[0].direction,
        intercept_proxy_domain::ProtocolDirection::Downstream
    );
    assert_eq!(failures[0].kind, HttpProtocolFailureKind::DecodeFailed);
}

#[tokio::test]
async fn encode_failure_after_rule_change_preserves_original_request_body_and_headers() {
    let listener = http_listener();
    let rule = set_string_rule(
        &listener,
        ProtocolDocumentRuleId::from_uuid(Uuid::from_u128(22)),
        ProtocolRuleStage::AppToProxy,
        1,
        "route",
        Vec::new(),
        "changed",
    );
    let (pipeline, observations) =
        pipeline_for_listener(ENCODE_FAILURE_SCRIPT, &listener, vec![rule]);
    let original_body = Bytes::from_static(b"wire");
    let mut message = request_message(original_body.clone());
    let original_headers = message.headers.clone();

    let error = pipeline
        .request(&test_http_context(), &mut message)
        .await
        .unwrap_err();

    assert_eq!(error.code, ErrorCode::Internal.as_str());
    assert!(error.message.contains("上行"));
    assert!(error.message.contains("ENTRY_POINT_FAILED"));
    assert_eq!(message.body, original_body);
    assert_eq!(message.headers, original_headers);
    assert!(!message.body_modified);
    assert!(observations.records().is_empty());
    let failures = observations.failures();
    assert_eq!(failures.len(), 1);
    assert_eq!(failures[0].origin_body, original_body);
    assert_eq!(failures[0].kind, HttpProtocolFailureKind::EncodeFailed);
    assert_eq!(failures[0].code, "ENTRY_POINT_FAILED");
    assert_eq!(failures[0].stage, None);
}

#[tokio::test]
async fn non_utf8_encode_output_preserves_original_request_body_and_headers() {
    let listener = http_listener();
    let rule = set_string_rule(
        &listener,
        ProtocolDocumentRuleId::from_uuid(Uuid::from_u128(23)),
        ProtocolRuleStage::AppToProxy,
        1,
        "route",
        Vec::new(),
        "changed",
    );
    let (pipeline, observations) =
        pipeline_for_listener(NON_UTF8_ENCODE_SCRIPT, &listener, vec![rule]);
    let original_body = Bytes::from_static(b"wire");
    let mut message = request_message(original_body.clone());
    let original_headers = message.headers.clone();

    let error = pipeline
        .request(&test_http_context(), &mut message)
        .await
        .unwrap_err();

    assert_eq!(error.code, ErrorCode::Internal.as_str());
    assert!(error.message.contains("非 UTF-8"));
    assert_eq!(message.body, original_body);
    assert_eq!(message.headers, original_headers);
    assert!(!message.body_modified);
    assert!(observations.records().is_empty());
    let failures = observations.failures();
    assert_eq!(failures.len(), 1);
    assert_eq!(failures[0].origin_body, original_body);
    assert_eq!(failures[0].kind, HttpProtocolFailureKind::OutputNotUtf8);
    assert_eq!(failures[0].code, "HTTP_PROTOCOL_OUTPUT_NOT_UTF8");
}

#[tokio::test]
async fn display_failure_falls_back_without_blocking_body_rewrite_or_observation() {
    let listener = http_listener();
    let rule = set_string_rule(
        &listener,
        ProtocolDocumentRuleId::from_uuid(Uuid::from_u128(24)),
        ProtocolRuleStage::AppToProxy,
        1,
        "route",
        Vec::new(),
        "changed",
    );
    let (pipeline, observations) =
        pipeline_for_listener(DISPLAY_FAILURE_SCRIPT, &listener, vec![rule]);
    let mut message = request_message(Bytes::from_static(b"wire"));

    pipeline
        .request(&test_http_context(), &mut message)
        .await
        .unwrap();

    assert_eq!(message.body, Bytes::from_static(b"wire|changed"));
    assert_eq!(message.declared_content_length(), Some(12));
    assert!(message.body_modified);
    let records = observations.records();
    assert_eq!(records.len(), 1);
    assert_eq!(
        records[0].observation.display,
        HttpProtocolDisplayViewModel::HexFallback {
            reason: HttpProtocolDisplayFallbackReason::EntryPointFailed,
        }
    );
    assert!(records[0].observation.stages.iter().all(|stage| {
        stage.display
            == HttpProtocolDisplayViewModel::HexFallback {
                reason: HttpProtocolDisplayFallbackReason::EntryPointFailed,
            }
    }));
}
