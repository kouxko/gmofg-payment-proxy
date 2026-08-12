use intercept_proxy_application::MessageContentKind;
use intercept_proxy_domain::BodyCodecKind;

use crate::adapters::body_codecs::resolve_message_codec;

#[test]
fn product_codec_error_codes_and_json_syntax_classification_are_stable() {
    let decode = decode_body(&StableErrorCodec, b"wire").expect_err("decode must fail");
    assert_eq!(decode.code, "PRODUCT_DECODE_FAILED");
    assert_eq!(decode.message, "decode failed");

    let encode = encode_body(&StableErrorCodec, "text").expect_err("encode must fail");
    assert_eq!(encode.code, "PRODUCT_ENCODE_FAILED");
    assert_eq!(encode.message, "encode failed");

    let json = decode_json(&Utf8BodyCodec, b"{invalid").expect_err("JSON must fail");
    assert_eq!(json.code, "JSON_INVALID");
    assert!(json.message.contains("not valid JSON"));
}

#[test]
fn content_view_uses_declared_shift_jis_for_vendor_json_and_preserves_query() {
    let (encoded, _, had_errors) = encoding_rs::SHIFT_JIS.encode(r#"{"result":"成功"}"#);
    assert!(!had_errors);
    let message = Message {
        start_line: "POST /payment?terminal=920&retry=1 HTTP/1.1".into(),
        headers: vec![RawHeader::new(
            Bytes::from_static(b"Content-Type"),
            Bytes::from_static(b"application/vnd.gmo.result+json; charset=Windows-31J"),
        )],
        body: Bytes::copy_from_slice(&encoded),
        body_modified: false,
    };

    let codec = resolve_message_codec(BodyCodecKind::Auto, &message);
    let view = content_view(codec.as_ref(), &message);

    assert_eq!(
        view.media_type.as_deref(),
        Some("application/vnd.gmo.result+json")
    );
    assert_eq!(view.charset.as_deref(), Some("windows-31j"));
    assert_eq!(view.content_kind, MessageContentKind::Json);
    assert_eq!(view.codec_id.as_deref(), Some("auto:shift-jis"));
    assert_eq!(view.body_text.as_deref(), Some(r#"{"result":"成功"}"#));
    assert_eq!(view.json.as_ref().unwrap()["result"], "成功");
    assert_eq!(view.query_string.as_deref(), Some("terminal=920&retry=1"));
    assert!(view.decode_error.is_none());
}

#[test]
fn content_view_classifies_xml_text_binary_and_unknown_without_guessing_json() {
    let cases = [
        (
            "application/soap+xml; charset=utf-8",
            MessageContentKind::Xml,
            true,
        ),
        ("text/plain; charset=UTF8", MessageContentKind::Text, true),
        (
            "application/octet-stream",
            MessageContentKind::Binary,
            false,
        ),
        ("application/problem", MessageContentKind::Unknown, false),
    ];
    for (content_type, expected_kind, expects_text) in cases {
        let message = Message {
            start_line: "PUT /resource HTTP/1.1".into(),
            headers: vec![RawHeader::new(
                Bytes::from_static(b"Content-Type"),
                Bytes::copy_from_slice(content_type.as_bytes()),
            )],
            body: Bytes::from_static(br#"{"looks":"json"}"#),
            body_modified: false,
        };

        let codec = resolve_message_codec(BodyCodecKind::Auto, &message);
        let view = content_view(codec.as_ref(), &message);

        assert_eq!(view.content_kind, expected_kind, "{content_type}");
        assert_eq!(view.body_text.is_some(), expects_text, "{content_type}");
        assert!(view.json.is_none(), "{content_type}");
        assert_eq!(view.body_bytes, br#"{"looks":"json"}"#);
    }
}

#[test]
fn content_view_reports_unsupported_or_invalid_declared_charset() {
    for (content_type, body, expected_error) in [
        (
            "application/json; charset=iso-8859-1",
            b"{}".as_slice(),
            "unsupported charset",
        ),
        (
            "text/plain; charset=shift_jis",
            b"\x82".as_slice(),
            "invalid Shift-JIS",
        ),
    ] {
        let message = Message {
            start_line: "DELETE /resource HTTP/1.1".into(),
            headers: vec![RawHeader::new(
                Bytes::from_static(b"Content-Type"),
                Bytes::copy_from_slice(content_type.as_bytes()),
            )],
            body: Bytes::copy_from_slice(body),
            body_modified: false,
        };

        let codec = resolve_message_codec(BodyCodecKind::Auto, &message);
        let view = content_view(codec.as_ref(), &message);

        assert!(view.body_text.is_none());
        assert!(view.json.is_none());
        assert!(
            view.decode_error
                .as_deref()
                .unwrap()
                .contains(expected_error)
        );
        assert_eq!(view.body_bytes, body);
    }
}

#[test]
fn product_request_classifier_receives_canonical_wire_metadata() {
    #[derive(Debug)]
    struct InspectingClassifier;

    impl RequestClassifier for InspectingClassifier {
        fn classify(
            &self,
            message: ProductMessageContext<'_>,
        ) -> intercept_proxy_product_api::ClassifiedRequest {
            assert_eq!(message.channel_id, "alpha");
            assert_eq!(message.start_line, b"POST /vendor HTTP/1.1");
            assert_eq!(
                message
                    .headers
                    .iter()
                    .map(|header| (header.name, header.value))
                    .collect::<Vec<_>>(),
                vec![
                    (b"X-Vendor".as_slice(), b"value\x80\xff".as_slice()),
                    (b"x-vendor".as_slice(), b"second".as_slice()),
                ]
            );
            assert_eq!(message.body, b"opaque");
            intercept_proxy_product_api::ClassifiedRequest {
                request_id: Some("product-id".into()),
                request_type: Some("product-type".into()),
            }
        }
    }

    let channel = ChannelId::new("alpha").expect("channel");
    let message = Message {
        start_line: "POST /vendor HTTP/1.1".into(),
        headers: vec![
            RawHeader::new(
                Bytes::from_static(b"X-Vendor"),
                Bytes::from_static(b"value\x80\xff"),
            ),
            RawHeader::new(
                Bytes::from_static(b"x-vendor"),
                Bytes::from_static(b"second"),
            ),
        ],
        body: Bytes::from_static(b"opaque"),
        body_modified: false,
    };

    let classified = classify_request(&InspectingClassifier, &channel, &message);
    assert_eq!(classified.request_id.as_deref(), Some("product-id"));
    assert_eq!(classified.request_type.as_deref(), Some("product-type"));
}

#[test]
fn forward_modified_uses_canonical_wire_headers_instead_of_lossy_display_projection() {
    let original = Message {
        start_line: "HTTP/1.1 299 Vendor Specific Result".into(),
        headers: vec![
            RawHeader::new(
                Bytes::from_static(b"X-Trace"),
                Bytes::from_static(b"first\x80"),
            ),
            RawHeader::new(
                Bytes::from_static(b"x-Other"),
                Bytes::from_static(b"middle\xff"),
            ),
            RawHeader::new(
                Bytes::from_static(b"x-TRACE"),
                Bytes::from_static(b"second"),
            ),
            RawHeader::new(Bytes::from_static(b"x-Other"), Bytes::from_static(b"last")),
        ],
        body: Bytes::from_static(b"old"),
        body_modified: false,
    };
    let mut edited = content_view(&Utf8BodyCodec, &original);
    edited.body_bytes = b"new".to_vec();
    edited.body_text = Some("new".into());
    edited.content_length = 3;
    let decision = BreakpointDecision {
        breakpoint_id: Uuid::new_v4(),
        expected_revision: 1,
        kind: BreakpointDecisionKind::ForwardModified,
        message: Some(edited),
        delay_ms: None,
        http_status: None,
        content_length_delta: None,
        truncate_at: None,
    };
    let mut effective = original.clone();

    let faults = apply_breakpoint_decision(
        &Utf8BodyCodec,
        AppMessageStage::Response,
        &original,
        &mut effective,
        &decision,
    )
    .expect("forward modified");

    assert!(faults.is_empty());
    assert_eq!(effective.start_line, original.start_line);
    assert_eq!(effective.headers, original.headers);
    assert_eq!(effective.body, Bytes::from_static(b"new"));
    assert_eq!(
        effective.reconstruct(),
        Bytes::from_static(
            b"HTTP/1.1 299 Vendor Specific Result\r\n\
X-Trace: first\x80\r\n\
x-Other: middle\xff\r\n\
x-TRACE: second\r\n\
x-Other: last\r\n\r\nnew"
        )
    );
}

#[test]
fn forward_modified_preserves_unedited_header_ows_byte_for_byte() {
    let original = Message::from_raw_http1_head(
        b"POST /ows HTTP/1.1\r\n\
X-Mixed:\t  value \t\r\n\
X-Compact:value\r\n\r\n",
        Bytes::from_static(b"body"),
    )
    .expect("raw message");
    let view = content_view(&Utf8BodyCodec, &original);

    let effective = proxy_message(&view, &original.start_line).expect("effective message");

    assert_eq!(effective.reconstruct(), original.reconstruct());
}

#[test]
fn forward_modified_rejects_non_ows_wire_metadata_from_the_frontend() {
    let original =
        Message::from_raw_http1_head(b"POST /ows HTTP/1.1\r\nX-Test: value\r\n\r\n", Bytes::new())
            .expect("raw message");
    let mut view = content_view(&Utf8BodyCodec, &original);
    view.raw_headers[0].leading_ows_bytes = b"\r\nInjected: ".to_vec();

    let error = proxy_message(&view, &original.start_line).expect_err("invalid OWS");

    assert_eq!(error.code, ErrorCode::ConfigInvalid.as_str());
}

#[test]
fn breakpoint_ipc_cannot_replace_the_rust_owned_start_line_or_use_status_600() {
    let original = Message::from_raw_http1_head(
        b"POST /safe HTTP/1.1\r\nHost: example.test\r\n\r\n",
        Bytes::new(),
    )
    .expect("raw message");
    let mut view = content_view(&Utf8BodyCodec, &original);
    view.start_line_bytes = b"POST / HTTP/1.1\r\nX-Injected: value".to_vec();

    let reconstructed = proxy_message(&view, &original.start_line).expect("safe message");
    assert_eq!(reconstructed.start_line, original.start_line);
    assert!(
        !reconstructed
            .reconstruct()
            .windows(10)
            .any(|window| window == b"X-Injected")
    );

    view.http_status = Some(600);
    let error = proxy_message(&view, "HTTP/1.1 200 OK").expect_err("invalid status");
    assert_eq!(error.code, ErrorCode::ConfigInvalid.as_str());
}

#[test]
fn forward_modified_merges_header_edits_and_applies_changed_http_status() {
    let original = Message {
        start_line: "HTTP/1.1 299 Vendor Specific Result".into(),
        headers: vec![
            RawHeader::new(
                Bytes::from_static(b"X-Keep"),
                Bytes::from_static(b"binary\x80\xff"),
            ),
            RawHeader::new(Bytes::from_static(b"X-Edit"), Bytes::from_static(b"old")),
            RawHeader::new(Bytes::from_static(b"x-remove"), Bytes::from_static(b"gone")),
        ],
        body: Bytes::from_static(b"body"),
        body_modified: false,
    };
    let mut edited = content_view(&Utf8BodyCodec, &original);
    edited.http_status = Some(503);
    edited.headers.insert("X-Edit".into(), vec!["new".into()]);
    edited.headers.remove("x-remove");
    edited.headers.insert("X-Added".into(), vec!["yes".into()]);
    let decision = BreakpointDecision {
        breakpoint_id: Uuid::new_v4(),
        expected_revision: 1,
        kind: BreakpointDecisionKind::ForwardModified,
        message: Some(edited),
        delay_ms: None,
        http_status: None,
        content_length_delta: None,
        truncate_at: None,
    };
    let mut effective = original.clone();

    apply_breakpoint_decision(
        &Utf8BodyCodec,
        AppMessageStage::Response,
        &original,
        &mut effective,
        &decision,
    )
    .expect("forward modified");

    assert_eq!(effective.start_line, "HTTP/1.1 503 Vendor Specific Result");
    assert_eq!(effective.http_status(), Some(503));
    assert_eq!(
        effective
            .headers
            .iter()
            .map(|header| (header.name.as_ref(), header.value.as_ref()))
            .collect::<Vec<_>>(),
        vec![
            (b"X-Keep".as_slice(), b"binary\x80\xff".as_slice()),
            (b"X-Edit".as_slice(), b"new".as_slice()),
            (b"X-Added".as_slice(), b"yes".as_slice()),
        ],
        "untouched wire fields stay exact while edited/deleted/added fields follow the UI"
    );
}

#[test]
fn mixed_case_header_edit_and_delete_apply_to_one_case_insensitive_field_group() {
    let raw = vec![
        RawHttpHeaderViewModel {
            name_bytes: b"X-Trace".to_vec(),
            value_bytes: b"first".to_vec(),
            leading_ows_bytes: b" ".to_vec(),
            trailing_ows_bytes: Vec::new(),
        },
        RawHttpHeaderViewModel {
            name_bytes: b"X-Keep".to_vec(),
            value_bytes: b"binary\x80\xff".to_vec(),
            leading_ows_bytes: b" ".to_vec(),
            trailing_ows_bytes: Vec::new(),
        },
        RawHttpHeaderViewModel {
            name_bytes: b"x-TRACE".to_vec(),
            value_bytes: b"second".to_vec(),
            leading_ows_bytes: b" ".to_vec(),
            trailing_ows_bytes: Vec::new(),
        },
        RawHttpHeaderViewModel {
            name_bytes: b"X-Remove".to_vec(),
            value_bytes: b"gone".to_vec(),
            leading_ows_bytes: b" ".to_vec(),
            trailing_ows_bytes: Vec::new(),
        },
    ];
    let mut edited = display_headers(&raw);
    assert_eq!(
        edited.get("X-Trace"),
        Some(&vec!["first".into(), "second".into()])
    );
    // Simulate an editor returning a different casing for the same field.
    edited.insert("x-trace".into(), vec!["replacement".into()]);
    edited.remove("X-Remove");

    let merged = merge_edited_headers(&raw, &edited).expect("valid wire whitespace");

    assert_eq!(
        merged
            .iter()
            .map(|header| (header.name.as_ref(), header.value.as_ref()))
            .collect::<Vec<_>>(),
        vec![
            (b"x-trace".as_slice(), b"replacement".as_slice()),
            (b"X-Keep".as_slice(), b"binary\x80\xff".as_slice()),
        ]
    );
}

#[test]
fn intentional_wire_faults_are_not_reported_as_internal_errors() {
    assert_eq!(result_text("INCORRECT_CONTENT_LENGTH"), "规则终止");
    assert_eq!(result_text("TRUNCATED_RESPONSE"), "截断");
}

#[derive(Debug)]
struct RejectingCodec;

impl BodyCodec for RejectingCodec {
    fn id(&self) -> &'static str {
        "rejecting"
    }

    fn name(&self) -> &'static str {
        "Rejecting Codec"
    }

    fn decode(&self, _: &[u8]) -> Result<String, intercept_proxy_product_api::ProductError> {
        Ok(String::new())
    }

    fn encode(&self, _: &str) -> Result<Vec<u8>, intercept_proxy_product_api::ProductError> {
        Err(intercept_proxy_product_api::ProductError::new(
            "PRODUCT_SPECIFIC_CODE",
            "rejected",
        ))
    }
}
