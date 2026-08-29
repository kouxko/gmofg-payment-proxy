use intercept_proxy_domain::{ProtocolPackageId, ProtocolPackageRef, ProtocolPackageVersion};

use crate::{LocalResponderCoordinator, LocalResponseOwnershipViolation};

const LOCAL_SCRIPT: &str = r#"
fn frame(reader, context) { throw "frame is owned by the inspector" }

fn decode(origin, context) {
    if context.direction() != "upstream" { throw "downstream decode must be skipped"; }
    let value = document::create();
    value.set("/amount", origin[0]);
    value
}

fn encode(origin, document, context) {
    if context.direction() == "upstream" { return origin; }
    let result = blob(2, 0);
    result[0] = 0x44;
    result[1] = if document.has("/amount") { document.get("/amount") } else { 0 };
    result
}

fn display(document, context) {
    "downstream-html"
}
"#;

fn local_package(script: impl Into<String>) -> crate::CompiledProtocolPackage {
    CompiledProtocolPackageTestBuilder::new()
        .with_script(script)
        .build()
}

fn local_coordinator(package: &crate::CompiledProtocolPackage) -> LocalResponderCoordinator {
    LocalResponderCoordinator::new(
        package,
        "connection-1",
        "listener-1",
        ProtocolRuntimeLimits::default(),
    )
    .unwrap()
}

#[test]
fn local_responder_transforms_request_then_builds_an_independent_response() {
    let package = local_package(LOCAL_SCRIPT);
    let mut coordinator = local_coordinator(&package);
    let request = coordinator
        .decode_request_with_document_transform(vec![7], |mut document| {
            document.set(&JsonPointer::property("amount"), DocumentValue::integer(8).unwrap()).unwrap();
            Ok(document)
        })
        .unwrap();
    let response = coordinator
        .build_response(&request, |mut document| {
            document.set(&JsonPointer::property("amount"), DocumentValue::integer(42).unwrap()).unwrap();
            Ok(document)
        })
        .unwrap();

    assert_eq!(response.written(), &[0x44, 42]);
    assert_eq!(
        response.response_document().resolve(&JsonPointer::property("amount")).unwrap(),
        &DocumentValue::integer(42).unwrap()
    );
    assert_eq!(
        request.document().resolve(&JsonPointer::property("amount")).unwrap(),
        &DocumentValue::integer(8).unwrap()
    );
}

#[test]
fn local_responder_uses_directional_schemas_for_request_and_response() {
    let upstream = DocumentSchemaNode::Object {
        title: Some("Local Request".to_owned()),
        properties: BTreeMap::from([(
            "request_amount".to_owned(),
            DocumentSchemaNode::Number { title: Some("Request Amount".to_owned()) },
        )]),
    };
    let downstream = DocumentSchemaNode::Object {
        title: Some("Local Response".to_owned()),
        properties: BTreeMap::from([(
            "response_amount".to_owned(),
            DocumentSchemaNode::Number { title: Some("Response Amount".to_owned()) },
        )]),
    };
    let script = r#"
fn frame(reader, context) { () }
fn decode(origin, context) {
    let result = document::create();
    result.set("/request_amount", origin[1].to_int());
    result
}
fn encode(origin, document, context) {
    let result = blob(2, 0);
    result[0] = 209;
    result[1] = if document.has("/response_amount") {
        document.get("/response_amount")
    } else { 0 };
    result
}
fn display(document, context) { "<p>directional local response</p>" }
"#;
    let package = CompiledProtocolPackageTestBuilder::new()
        .with_directional_schemas(upstream, downstream)
        .with_script(script)
        .build();
    let mut coordinator = local_coordinator(&package);

    let request = coordinator.decode_request(vec![2, 11]).unwrap();
    assert_eq!(
        request.document().resolve(&JsonPointer::property("request_amount")).unwrap(),
        &DocumentValue::integer(11).unwrap()
    );
    let response = coordinator
        .build_response(&request, |mut document| {
            document
                .set(&JsonPointer::property("response_amount"), DocumentValue::integer(42).unwrap())
                .unwrap();
            Ok(document)
        })
        .unwrap();
    assert_eq!(response.written(), &[209, 42]);
    assert_eq!(
        response.response_document().resolve(&JsonPointer::property("response_amount")).unwrap(),
        &DocumentValue::integer(42).unwrap()
    );
    assert!(response.response_document().resolve(&JsonPointer::property("request_amount")).is_err());
}

#[test]
fn local_response_transform_runs_once_and_starts_from_empty_downstream_document() {
    let package = local_package(LOCAL_SCRIPT);
    let calls = AtomicUsize::new(0);
    let mut coordinator = local_coordinator(&package);
    let request = coordinator.decode_request(vec![9]).unwrap();
    let response = coordinator
        .build_response(&request, |document| {
            calls.fetch_add(1, Ordering::Relaxed);
            Ok(document)
        })
        .unwrap();
    assert_eq!(response.written(), &[0x44, 0]);
    assert_eq!(calls.load(Ordering::Relaxed), 1);
}

#[test]
fn response_display_requires_commit_handle_and_follows_downstream_encode() {
    let package = local_package(LOCAL_SCRIPT);
    let mut enabled = local_coordinator(&package);
    let request = enabled.decode_request(vec![7]).unwrap();
    let response = enabled.build_response(&request, Ok).unwrap();
    let handle = enabled.response_committed(&response).unwrap();
    assert_eq!(
        enabled.render_response_display(&handle).unwrap(),
        ProtocolDisplayResult::UntrustedHtml("downstream-html".to_owned())
    );
    assert_eq!(response.written(), &[0x44, 0]);
}

#[test]
fn display_failure_after_commit_never_changes_response_bytes() {
    let package =
        local_package(LOCAL_SCRIPT.replace("\"downstream-html\"", "throw \"display failed\""));
    let mut coordinator = local_coordinator(&package);
    let request = coordinator.decode_request(vec![7]).unwrap();
    let response = coordinator.build_response(&request, Ok).unwrap();
    let handle = coordinator.response_committed(&response).unwrap();

    assert_eq!(
        coordinator.render_response_display(&handle).unwrap(),
        ProtocolDisplayResult::HexFallback(DisplayFallbackReason::EntryPointFailed)
    );
    assert_eq!(response.written(), &[0x44, 0]);
}

#[test]
fn local_bridge_rejects_foreign_package_schema_connection_and_output() {
    let package = local_package(LOCAL_SCRIPT);
    let mut source = local_coordinator(&package);
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
    let mut foreign = local_coordinator(&foreign_package);
    assert_ownership_violation(
        &foreign.build_response(&request, Ok).unwrap_err(),
        LocalResponseOwnershipViolation::Package,
    );

    let foreign_schema = DocumentSchemaNode::Object {
        title: Some("Foreign".to_owned()),
        properties: BTreeMap::from([(
            "amount".to_owned(),
            DocumentSchemaNode::Number { title: Some("Amount".to_owned()) },
        )]),
    };
    let schema_package = CompiledProtocolPackageTestBuilder::new()
        .with_schema(foreign_schema)
        .with_script(LOCAL_SCRIPT)
        .with_downstream_encode()
        .build();
    let mut schema = local_coordinator(&schema_package);
    assert_ownership_violation(
        &schema.build_response(&request, Ok).unwrap_err(),
        LocalResponseOwnershipViolation::Output,
    );

    let mut connection = LocalResponderCoordinator::new(
        &package,
        "connection-2",
        "listener-1",
        ProtocolRuntimeLimits::default(),
    )
    .unwrap();
    assert_ownership_violation(
        &connection.build_response(&request, Ok).unwrap_err(),
        LocalResponseOwnershipViolation::Connection,
    );

    let mut output = local_coordinator(&package);
    assert_ownership_violation(
        &output.build_response(&request, Ok).unwrap_err(),
        LocalResponseOwnershipViolation::Output,
    );

    let metadata_request = source.decode_request(vec![7]).unwrap();
    let response = source
        .build_response(&metadata_request, |_| {
            Ok(Document::new(DocumentValue::Object(BTreeMap::from([(
                "undeclared".to_owned(),
                DocumentValue::null(),
            )]))))
        })
        .expect("incomplete schema metadata does not reject response fields");
    assert_eq!(
        response
            .response_document()
            .resolve(&JsonPointer::property("undeclared"))
            .unwrap(),
        &DocumentValue::null()
    );
}

#[test]
fn response_and_display_handles_reject_another_coordinator() {
    let package = local_package(LOCAL_SCRIPT);
    let mut source = local_coordinator(&package);
    let request = source.decode_request(vec![7]).unwrap();
    let response = source.build_response(&request, Ok).unwrap();
    let handle = source.response_committed(&response).unwrap();
    let mut foreign = local_coordinator(&package);

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
    let package = local_package(LOCAL_SCRIPT);
    let mut coordinator = local_coordinator(&package);
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
    assert!(
        matches!(
            error,
            ProtocolRuntimeError::LocalResponseOwnershipViolation { violation, .. }
                if *violation == expected
        ),
        "expected {expected:?}, got {error:?}"
    );
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
    );
    let mut empty = local_coordinator(&empty_package);
    let request = empty.decode_request(vec![1]).unwrap();
    assert_eq!(
        empty.build_response(&request, Ok).unwrap_err().code(),
        "LOCAL_RESPONSE_EMPTY"
    );

    let package = local_package(LOCAL_SCRIPT);
    let limits = ProtocolRuntimeLimits::new(100_000, 32, 1024, 1, 100).unwrap();
    let mut oversized =
        LocalResponderCoordinator::new(&package, "connection-1", "listener-1", limits).unwrap();
    let request = oversized.decode_request(vec![1]).unwrap();
    assert!(matches!(
        oversized.build_response(&request, Ok),
        Err(ProtocolRuntimeError::ResourceLimitExceeded {
            entry: ProtocolEntryPoint::Encode,
            limit: ProtocolResourceLimit::BlobBytes,
            ..
        })
    ));

    let failed_package = local_package(LOCAL_SCRIPT.replace(
        "let result = blob(2, 0);",
        "throw \"encode failed\"; let result = blob(2, 0);",
    ));
    let mut failed = local_coordinator(&failed_package);
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
fn cancellation_is_checked_before_and_after_owned_transform() {
    let package = local_package(LOCAL_SCRIPT);
    let cancellation = ProtocolExecutionCancellation::new();
    let mut coordinator = LocalResponderCoordinator::new_with_cancellation(
        &package,
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
    let package = local_package(LOCAL_SCRIPT);
    let mut coordinator = local_coordinator(&package);
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
