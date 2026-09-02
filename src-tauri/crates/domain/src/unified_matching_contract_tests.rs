use crate::{
    BooleanPredicate, Condition, Document, DocumentMatchPath, DocumentPredicate, HttpHeader,
    MatchContext, MatchField, MatchOperator, MessageStage, RuleId, RuntimeEpoch, TerminalIdentity,
    UnifiedRuleProgram,
};

fn context<'a>(
    terminal: &'a TerminalIdentity,
    method: &'a str,
    request_target: &'a str,
    headers: &'a [HttpHeader<'a>],
) -> MatchContext<'a> {
    MatchContext {
        runtime_epoch: RuntimeEpoch::new(),
        channel: crate::ChannelId::new("alpha").expect("channel"),
        stage: MessageStage::Request,
        terminal,
        method: Some(method),
        request_target: Some(request_target),
        headers,
    }
}

#[test]
fn http_method_header_and_request_target_use_typed_contracts() {
    let terminal = TerminalIdentity {
        source_ip: "127.0.0.1".into(),
        certificate_sha256: String::new(),
    };
    let headers = [
        HttpHeader::new(b"X-Mode", b"ignored"),
        HttpHeader::new(b"x-mode", b"phase18-enabled"),
    ];
    let context = context(&terminal, "POST", "/customer/42?mode=full", &headers);

    assert!(
        crate::matches_http_condition(
            &MatchField::Method,
            &MatchOperator::Equals("POST".into()),
            &context,
        )
        .expect("method")
    );
    assert!(
        crate::matches_http_condition(
            &MatchField::Header("/x-mode".into()),
            &MatchOperator::EndsWith("enabled".into()),
            &context,
        )
        .expect("duplicate header ANY")
    );
    assert!(
        crate::matches_http_condition(
            &MatchField::RequestTarget,
            &MatchOperator::Wildcard("/customer/*?mode=full".into()),
            &context,
        )
        .expect("request target")
    );
}

#[test]
fn document_wildcard_path_matches_any_single_level_only() {
    let document = Document::parse_json(
        r#"{"customer":{"first":{"active":false},"second":{"active":true},"deep":{"child":{"active":true}}}}"#,
    )
    .expect("document");
    let condition = Condition::DocumentPattern {
        path: DocumentMatchPath::parse("/customer/*/active").expect("match path"),
        predicate: DocumentPredicate::Boolean(BooleanPredicate::Equal(true)),
    };

    assert!(crate::matches_document_condition(&condition, &document).expect("wildcard match"));

    let too_shallow = Condition::DocumentPattern {
        path: DocumentMatchPath::parse("/customer/*/child/active").expect("match path"),
        predicate: DocumentPredicate::Boolean(BooleanPredicate::Equal(true)),
    };
    assert!(crate::matches_document_condition(&too_shallow, &document).expect("exact depth"));

    let does_not_cross_levels = Condition::DocumentPattern {
        path: DocumentMatchPath::parse("/customer/*/active/value").expect("match path"),
        predicate: DocumentPredicate::Boolean(BooleanPredicate::Equal(true)),
    };
    assert!(
        !crate::matches_document_condition(&does_not_cross_levels, &document)
            .expect("wildcard must not cross levels")
    );
}

#[test]
fn method_rejects_non_equal_operator_and_header_name_requires_one_pointer_segment() {
    let invalid_method =
        crate::validate_http_condition(&MatchField::Method, &MatchOperator::Contains("OS".into()))
            .expect_err("method contains must fail");
    assert_eq!(invalid_method.code, crate::ErrorCode::RuleInvalid);

    let invalid_header = crate::validate_http_condition(
        &MatchField::Header("/group/x-mode".into()),
        &MatchOperator::Equals("enabled".into()),
    )
    .expect_err("header is exactly one pointer segment");
    assert_eq!(invalid_header.code, crate::ErrorCode::RuleInvalid);
    assert!(
        crate::validate_http_condition(
            &MatchField::Header("/x mode".into()),
            &MatchOperator::Equals("enabled".into()),
        )
        .is_err()
    );

    let terminal = TerminalIdentity {
        source_ip: "127.0.0.1".into(),
        certificate_sha256: String::new(),
    };
    let missing_metadata = MatchContext {
        runtime_epoch: RuntimeEpoch::new(),
        channel: crate::ChannelId::new("missing-metadata").expect("channel"),
        stage: MessageStage::Response,
        terminal: &terminal,
        method: None,
        request_target: None,
        headers: &[],
    };
    assert!(
        crate::matches_http_condition(
            &MatchField::RequestTarget,
            &MatchOperator::Equals("/".into()),
            &missing_metadata,
        )
        .is_err()
    );
}

#[test]
fn unified_program_rejects_unknown_rule_id_instead_of_matching_it() {
    let program = UnifiedRuleProgram::new(Vec::new()).expect("empty program");
    let mut document = Document::parse_json(r"{}").expect("document");

    let apply_error = program
        .evaluate_and_apply_rule_with_http(RuleId::new(), &mut document, |_, _| Ok(false))
        .expect_err("unknown rule must not match or mutate");
    assert_eq!(apply_error.code, crate::ErrorCode::RuleInvalid);

    let evaluate_error = program
        .evaluate_rule_with_http(RuleId::new(), &document, |_, _| Ok(false))
        .expect_err("unknown rule must not match");
    assert_eq!(evaluate_error.code, crate::ErrorCode::RuleInvalid);
}
