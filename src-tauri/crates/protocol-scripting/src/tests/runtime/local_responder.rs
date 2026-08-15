use intercept_proxy_domain::{ProtocolPackageId, ProtocolPackageRef, ProtocolPackageVersion};

use crate::{LocalResponderCoordinator, LocalResponseOwnershipViolation};

const LOCAL_SCRIPT: &str = r#"
fn frame(reader, context) { throw "frame is owned by the inspector" }

fn decode(origin, context) {
    if context.direction() != "upstream" { throw "downstream decode must be skipped"; }
    let value = document::create();
    value.set("amount", origin[0]);
    value
}

fn encode(origin, document, context) {
    if context.direction() != "downstream" { throw "upstream encode must be skipped"; }
    let result = blob(2, 0);
    result[0] = 0x44;
    result[1] = if document.has("amount") { document.get("amount") } else { 0 };
    result
}

fn display(document, context) {
    if context.direction() != "downstream" { throw "request display must be skipped"; }
    "downstream-html"
}
"#;

fn local_package(
    script: impl Into<String>,
    encode: bool,
    display: bool,
) -> crate::CompiledProtocolPackage {
    let mut builder = CompiledProtocolPackageTestBuilder::new().with_script(script);
    if encode {
        builder = builder.with_downstream_encode();
    }
    if display {
        builder = builder.with_display();
    }
    builder.build()
}

fn local_coordinator(
    package: &crate::CompiledProtocolPackage,
    decode: bool,
    encode: bool,
) -> LocalResponderCoordinator {
    LocalResponderCoordinator::new(
        package,
        decode,
        encode,
        "connection-1",
        "listener-1",
        ProtocolRuntimeLimits::default(),
    )
    .unwrap()
}

#[test]
fn local_responder_four_states_clone_or_create_document_and_encode_or_echo() {
    let package = local_package(LOCAL_SCRIPT, true, true);

    for (decode, encode, expected) in [
        (false, false, vec![7]),
        (true, false, vec![7]),
        (false, true, vec![0x44, 42]),
        (true, true, vec![0x44, 42]),
    ] {
        let mut coordinator = local_coordinator(&package, decode, encode);
        let request = coordinator.decode_request(vec![7]).unwrap();
        assert_eq!(request.document().is_some(), decode);
        let response = coordinator
            .build_response(&request, |mut document| {
                document.set("amount", DocumentValue::Int(42)).unwrap();
                Ok(document)
            })
            .unwrap();

        assert_eq!(response.written(), expected);
        assert_eq!(
            response.response_document().get("amount").unwrap(),
            &DocumentValue::Int(42)
        );
        if let Some(document) = request.document() {
            // response 修改的是独立 owned clone，Request Document 仍是只读原值。
            assert_eq!(document.get("amount").unwrap(), &DocumentValue::Int(7));
        }
    }
}

#[test]
fn local_response_transform_runs_once_even_without_decode_and_no_rules_keep_initial_state() {
    let package = local_package(LOCAL_SCRIPT, true, false);
    let calls = AtomicUsize::new(0);
    let mut empty = local_coordinator(&package, false, true);
    let request = empty.decode_request(vec![9]).unwrap();
    let response = empty
        .build_response(&request, |document| {
            calls.fetch_add(1, Ordering::Relaxed);
            Ok(document)
        })
        .unwrap();
    assert_eq!(response.written(), &[0x44, 0]);
    assert!(!response.response_document().has("amount").unwrap());

    let mut cloned = local_coordinator(&package, true, true);
    let request = cloned.decode_request(vec![9]).unwrap();
    let response = cloned
        .build_response(&request, |document| {
            calls.fetch_add(1, Ordering::Relaxed);
            Ok(document)
        })
        .unwrap();
    assert_eq!(response.written(), &[0x44, 9]);
    assert_eq!(calls.load(Ordering::Relaxed), 2);
}

#[test]
fn raw_echo_ignores_response_document_changes_but_keeps_response_snapshot() {
    let package = local_package(LOCAL_SCRIPT, false, false);
    let mut coordinator = local_coordinator(&package, true, false);
    let request = coordinator.decode_request(vec![7, 8]).unwrap();
    let response = coordinator
        .build_response(&request, |mut document| {
            document.set("amount", DocumentValue::Int(99)).unwrap();
            Ok(document)
        })
        .unwrap();

    assert_eq!(response.written(), &[7, 8]);
    assert_eq!(
        response.response_document().get("amount").unwrap(),
        &DocumentValue::Int(99)
    );
    assert_eq!(
        request.document().unwrap().get("amount").unwrap(),
        &DocumentValue::Int(7)
    );
}

#[test]
fn response_display_requires_commit_handle_and_follows_downstream_encode() {
    let package = local_package(LOCAL_SCRIPT, true, true);
    let mut enabled = local_coordinator(&package, true, true);
    let request = enabled.decode_request(vec![7]).unwrap();
    let response = enabled.build_response(&request, Ok).unwrap();
    let handle = enabled.response_committed(&response).unwrap();
    assert_eq!(
        enabled.render_response_display(&handle).unwrap(),
        ProtocolDisplayResult::UntrustedHtml("downstream-html".to_owned())
    );
    assert_eq!(response.written(), &[0x44, 7]);

    let mut disabled = local_coordinator(&package, true, false);
    let request = disabled.decode_request(vec![7]).unwrap();
    let response = disabled.build_response(&request, Ok).unwrap();
    let handle = disabled.response_committed(&response).unwrap();
    assert_eq!(
        disabled.render_response_display(&handle).unwrap(),
        ProtocolDisplayResult::HexFallback(DisplayFallbackReason::EncodeDisabled)
    );

    let no_display_package = local_package(LOCAL_SCRIPT, true, false);
    let mut missing = local_coordinator(&no_display_package, false, true);
    let request = missing.decode_request(vec![7]).unwrap();
    let response = missing.build_response(&request, Ok).unwrap();
    let handle = missing.response_committed(&response).unwrap();
    assert_eq!(
        missing.render_response_display(&handle).unwrap(),
        ProtocolDisplayResult::HexFallback(DisplayFallbackReason::NotDeclared)
    );
}

#[test]
fn display_failure_after_commit_never_changes_response_bytes() {
    let package = local_package(
        LOCAL_SCRIPT.replace("\"downstream-html\"", "throw \"display failed\""),
        true,
        true,
    );
    let mut coordinator = local_coordinator(&package, true, true);
    let request = coordinator.decode_request(vec![7]).unwrap();
    let response = coordinator.build_response(&request, Ok).unwrap();
    let handle = coordinator.response_committed(&response).unwrap();

    assert_eq!(
        coordinator.render_response_display(&handle).unwrap(),
        ProtocolDisplayResult::HexFallback(DisplayFallbackReason::EntryPointFailed)
    );
    assert_eq!(response.written(), &[0x44, 7]);
}

#[test]
fn local_bridge_rejects_foreign_package_schema_connection_and_output() {
    let package = local_package(LOCAL_SCRIPT, true, false);
    let mut source = local_coordinator(&package, true, true);
    let request = source.decode_request(vec![7]).unwrap();

    let foreign_ref = ProtocolPackageRef {
        id: ProtocolPackageId::new("foreign-protocol").unwrap(),
        version: ProtocolPackageVersion::new("1.0.0").unwrap(),
    };
    let foreign_package = CompiledProtocolPackageTestBuilder::new()
        .with_package(foreign_ref)
        .with_script(LOCAL_SCRIPT)
        .with_downstream_encode()
        .build();
    let mut foreign = local_coordinator(&foreign_package, true, true);
    assert_ownership_violation(
        &foreign.build_response(&request, Ok).unwrap_err(),
        LocalResponseOwnershipViolation::Package,
    );

    let foreign_schema = DocumentSchema::new(
        DocumentSchemaId::new("foreign-schema").unwrap(),
        1,
        "Foreign",
        vec![
            DocumentField::new(
                DocumentFieldName::new("amount").unwrap(),
                DocumentFieldType::Int,
                "Amount",
            )
            .unwrap(),
        ],
    )
    .unwrap();
    let schema_package = CompiledProtocolPackageTestBuilder::new()
        .with_schema(foreign_schema)
        .with_script(LOCAL_SCRIPT)
        .with_downstream_encode()
        .build();
    let mut schema = local_coordinator(&schema_package, true, true);
    assert_ownership_violation(
        &schema.build_response(&request, Ok).unwrap_err(),
        LocalResponseOwnershipViolation::Schema,
    );

    let mut connection = LocalResponderCoordinator::new(
        &package,
        true,
        true,
        "connection-2",
        "listener-1",
        ProtocolRuntimeLimits::default(),
    )
    .unwrap();
    assert_ownership_violation(
        &connection.build_response(&request, Ok).unwrap_err(),
        LocalResponseOwnershipViolation::Connection,
    );

    let mut output = local_coordinator(&package, true, true);
    assert_ownership_violation(
        &output.build_response(&request, Ok).unwrap_err(),
        LocalResponseOwnershipViolation::Output,
    );

    assert_ownership_violation(
        &source
            .build_response(&request, |_| {
                Ok(Document::new(schema_package.schema().clone()))
            })
            .unwrap_err(),
        LocalResponseOwnershipViolation::Schema,
    );
}

#[test]
fn response_and_display_handles_reject_another_coordinator() {
    let package = local_package(LOCAL_SCRIPT, true, true);
    let mut source = local_coordinator(&package, true, true);
    let request = source.decode_request(vec![7]).unwrap();
    let response = source.build_response(&request, Ok).unwrap();
    let handle = source.response_committed(&response).unwrap();
    let mut foreign = local_coordinator(&package, true, true);

    assert_ownership_violation(
        &foreign.response_committed(&response).unwrap_err(),
        LocalResponseOwnershipViolation::Output,
    );
    assert_ownership_violation(
        &foreign.render_response_display(&handle).unwrap_err(),
        LocalResponseOwnershipViolation::Output,
    );
}

#[test]
fn each_request_builds_one_response_and_each_response_commits_once() {
    let package = local_package(LOCAL_SCRIPT, true, true);
    let mut coordinator = local_coordinator(&package, true, true);
    let request = coordinator.decode_request(vec![7]).unwrap();
    let response = coordinator.build_response(&request, Ok).unwrap();

    assert_ownership_violation(
        &coordinator.build_response(&request, Ok).unwrap_err(),
        LocalResponseOwnershipViolation::Output,
    );
    let _handle = coordinator.response_committed(&response).unwrap();
    assert_ownership_violation(
        &coordinator.response_committed(&response).unwrap_err(),
        LocalResponseOwnershipViolation::Output,
    );
}

fn assert_ownership_violation(
    error: &ProtocolRuntimeError,
    expected: LocalResponseOwnershipViolation,
) {
    assert!(matches!(
        error,
        ProtocolRuntimeError::LocalResponseOwnershipViolation { violation, .. }
            if *violation == expected
    ));
    assert_eq!(error.code(), "LOCAL_RESPONSE_OWNERSHIP_VIOLATION");
}

#[test]
fn empty_oversized_and_failed_responses_fail_before_commit() {
    let empty_package = local_package(
        r"
fn frame(reader, context) { () }
fn decode(origin, context) { document::create() }
fn encode(origin, document, context) { blob() }
",
        true,
        false,
    );
    let mut empty = local_coordinator(&empty_package, false, true);
    let request = empty.decode_request(vec![1]).unwrap();
    assert_eq!(
        empty.build_response(&request, Ok).unwrap_err().code(),
        "LOCAL_RESPONSE_EMPTY"
    );

    let package = local_package(LOCAL_SCRIPT, true, false);
    let limits = ProtocolRuntimeLimits::new(100_000, 32, 1024, 1, 100).unwrap();
    let mut oversized =
        LocalResponderCoordinator::new(&package, false, true, "connection-1", "listener-1", limits)
            .unwrap();
    let request = oversized.decode_request(vec![1]).unwrap();
    assert!(matches!(
        oversized.build_response(&request, Ok),
        Err(ProtocolRuntimeError::ResourceLimitExceeded {
            entry: ProtocolEntryPoint::Encode,
            limit: ProtocolResourceLimit::BlobBytes,
            ..
        })
    ));

    let failed_package = local_package(
        LOCAL_SCRIPT.replace(
            "let result = blob(2, 0);",
            "throw \"encode failed\"; let result = blob(2, 0);",
        ),
        true,
        false,
    );
    let mut failed = local_coordinator(&failed_package, false, true);
    let request = failed.decode_request(vec![1]).unwrap();
    assert!(matches!(
        failed.build_response(&request, Ok),
        Err(ProtocolRuntimeError::EntryPointFailed {
            entry: ProtocolEntryPoint::Encode,
            ..
        })
    ));
}

#[test]
fn empty_and_oversized_raw_echo_are_rejected() {
    let package = local_package(LOCAL_SCRIPT, false, false);
    let mut empty = local_coordinator(&package, false, false);
    let request = empty.decode_request(Vec::new()).unwrap();
    assert_eq!(
        empty.build_response(&request, Ok).unwrap_err().code(),
        "LOCAL_RESPONSE_EMPTY"
    );

    let limits = ProtocolRuntimeLimits::new(100_000, 32, 1024, 1, 100).unwrap();
    let mut oversized = LocalResponderCoordinator::new(
        &package,
        false,
        false,
        "connection-1",
        "listener-1",
        limits,
    )
    .unwrap();
    let request = oversized.decode_request(vec![1, 2]).unwrap();
    assert!(matches!(
        oversized.build_response(&request, Ok),
        Err(ProtocolRuntimeError::ResourceLimitExceeded {
            limit: ProtocolResourceLimit::BlobBytes,
            ..
        })
    ));
}

#[test]
fn cancellation_is_checked_before_and_after_owned_transform() {
    let package = local_package(LOCAL_SCRIPT, true, false);
    let cancellation = ProtocolExecutionCancellation::new();
    let mut coordinator = LocalResponderCoordinator::new_with_cancellation(
        &package,
        true,
        true,
        "connection-1",
        "listener-1",
        ProtocolRuntimeLimits::default(),
        cancellation.clone(),
    )
    .unwrap();
    let request = coordinator.decode_request(vec![7]).unwrap();
    let calls = AtomicUsize::new(0);
    cancellation.cancel();
    assert_eq!(
        coordinator
            .build_response(&request, |document| {
                calls.fetch_add(1, Ordering::Relaxed);
                Ok(document)
            })
            .unwrap_err()
            .code(),
        "LOCAL_RESPONSE_CANCELLED"
    );
    assert_eq!(calls.load(Ordering::Relaxed), 0);

    cancellation.reset();
    assert_eq!(
        coordinator
            .build_response(&request, |document| {
                cancellation.cancel();
                Ok(document)
            })
            .unwrap_err()
            .code(),
        "LOCAL_RESPONSE_CANCELLED"
    );
}

#[test]
fn local_responder_debug_reports_shape_without_payload_or_script() {
    let package = local_package(LOCAL_SCRIPT, true, true);
    let mut coordinator = local_coordinator(&package, true, true);
    let request = coordinator.decode_request(vec![0xde, 0xad]).unwrap();
    let response = coordinator.build_response(&request, Ok).unwrap();
    let handle = coordinator.response_committed(&response).unwrap();

    for debug in [
        format!("{coordinator:?}"),
        format!("{request:?}"),
        format!("{response:?}"),
        format!("{handle:?}"),
    ] {
        assert!(!debug.contains("fn decode"));
        assert!(!debug.contains("222"));
        assert!(!debug.contains("173"));
    }
}
