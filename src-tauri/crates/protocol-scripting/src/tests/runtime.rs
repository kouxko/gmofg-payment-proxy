use std::sync::atomic::{AtomicUsize, Ordering};

use intercept_proxy_domain::{
    DirectionProcessingOptions, Document, DocumentField, DocumentFieldName, DocumentFieldType,
    DocumentSchema, DocumentSchemaId, DocumentValue,
};

use crate::{
    DirectionExecutionPlan, DisplayFallbackReason, ProtocolDirection, ProtocolDirectionExecutor,
    ProtocolDisplayResult, ProtocolEntryPoint, ProtocolResourceLimit, ProtocolRuntimeError,
    ProtocolRuntimeLimits, test_support::CompiledProtocolPackageTestBuilder,
};

const VALID_SCRIPT: &str = r#"
fn frame(reader, context) { () }

fn decode(origin, context) {
    if context.stage() != "receive" { throw "wrong decode stage"; }
    let value = document::create();
    if context.direction() == "upstream" {
        value.set("amount", 7);
    } else {
        value.set("amount", 8);
    }
    value
}

fn encode(origin, document, context) {
    if context.stage() != "send" { throw "wrong encode stage"; }
    let result = blob(2, 0);
    result[0] = if context.direction() == "upstream" { 0x55 } else { 0x44 };
    result[1] = if document.has("amount") { document.get("amount") } else { 0 };
    result
}

fn display(document, context) {
    if context.stage() != "display" { throw "wrong display stage"; }
    if context.direction() == "upstream" { "upstream-html" } else { "downstream-html" }
}
"#;

fn options(decode_enabled: bool, encode_enabled: bool) -> DirectionProcessingOptions {
    DirectionProcessingOptions {
        decode_enabled,
        encode_enabled,
    }
}

fn package_with_all_entries() -> crate::CompiledProtocolPackage {
    CompiledProtocolPackageTestBuilder::new()
        .with_script(VALID_SCRIPT)
        .with_upstream_encode()
        .with_downstream_encode()
        .with_display()
        .build()
}

fn executor(
    package: &crate::CompiledProtocolPackage,
    direction: ProtocolDirection,
    decode_enabled: bool,
    encode_enabled: bool,
) -> ProtocolDirectionExecutor {
    let plan =
        DirectionExecutionPlan::new(package, direction, options(decode_enabled, encode_enabled))
            .unwrap();
    ProtocolDirectionExecutor::new(
        package,
        plan,
        "connection-1",
        "listener-1",
        ProtocolRuntimeLimits::default(),
    )
    .unwrap()
}

#[test]
fn upstream_four_states_have_exact_document_output_and_display_semantics() {
    let package = package_with_all_entries();
    let origin = vec![0x10, 0x20];

    for (decode, encode, expected_written, expected_display) in [
        (
            false,
            false,
            origin.clone(),
            ProtocolDisplayResult::HexFallback(DisplayFallbackReason::EncodeDisabled),
        ),
        (
            true,
            false,
            origin.clone(),
            ProtocolDisplayResult::HexFallback(DisplayFallbackReason::EncodeDisabled),
        ),
        (
            false,
            true,
            vec![0x55, 0],
            ProtocolDisplayResult::UntrustedHtml("upstream-html".to_owned()),
        ),
        (
            true,
            true,
            vec![0x55, 7],
            ProtocolDisplayResult::UntrustedHtml("upstream-html".to_owned()),
        ),
    ] {
        let mut executor = executor(&package, ProtocolDirection::Upstream, decode, encode);
        let output = executor.execute_frame(origin.clone()).unwrap();

        assert_eq!(output.origin(), origin);
        assert_eq!(output.written(), expected_written);
        assert_eq!(output.decoded_document().is_some(), decode);
        if let Some(document) = output.decoded_document() {
            assert_eq!(document.get("amount").unwrap(), &DocumentValue::Int(7));
        }
        assert_eq!(executor.render_display(&output), expected_display);
    }
}

#[test]
fn all_sixteen_bidirectional_switch_combinations_are_isolated() {
    let package = package_with_all_entries();
    for mask in 0_u8..16 {
        let up_decode = mask & 1 != 0;
        let up_encode = mask & 2 != 0;
        let down_decode = mask & 4 != 0;
        let down_encode = mask & 8 != 0;
        let mut upstream = executor(&package, ProtocolDirection::Upstream, up_decode, up_encode);
        let mut downstream = executor(
            &package,
            ProtocolDirection::Downstream,
            down_decode,
            down_encode,
        );

        let up = upstream.execute_frame(vec![1]).unwrap();
        let down = downstream.execute_frame(vec![2]).unwrap();
        assert_eq!(up.decoded_document().is_some(), up_decode);
        assert_eq!(down.decoded_document().is_some(), down_decode);
        let expected_up = if up_encode {
            vec![0x55, if up_decode { 7 } else { 0 }]
        } else {
            vec![1]
        };
        let expected_down = if down_encode {
            vec![0x44, if down_decode { 8 } else { 0 }]
        } else {
            vec![2]
        };
        assert_eq!(up.written(), expected_up);
        assert_eq!(down.written(), expected_down);
        assert_eq!(
            upstream.render_display(&up),
            if up_encode {
                ProtocolDisplayResult::UntrustedHtml("upstream-html".to_owned())
            } else {
                ProtocolDisplayResult::HexFallback(DisplayFallbackReason::EncodeDisabled)
            }
        );
        assert_eq!(
            downstream.render_display(&down),
            if down_encode {
                ProtocolDisplayResult::UntrustedHtml("downstream-html".to_owned())
            } else {
                ProtocolDisplayResult::HexFallback(DisplayFallbackReason::EncodeDisabled)
            }
        );
    }
}

#[test]
fn document_rules_run_only_after_decode_and_before_encode() {
    let package = package_with_all_entries();
    let calls = AtomicUsize::new(0);
    let mut enabled = executor(&package, ProtocolDirection::Upstream, true, true);
    let output = enabled
        .execute_frame_with_rules(vec![1], |document| {
            calls.fetch_add(1, Ordering::Relaxed);
            document.set("amount", DocumentValue::Int(42)).unwrap();
            Ok(())
        })
        .unwrap();
    assert_eq!(calls.load(Ordering::Relaxed), 1);
    assert_eq!(output.written(), &[0x55, 42]);
    assert_eq!(
        output.decoded_document().unwrap().get("amount").unwrap(),
        &DocumentValue::Int(42)
    );

    let mut decode_off = executor(&package, ProtocolDirection::Upstream, false, true);
    let output = decode_off
        .execute_frame_with_rules(vec![1], |_| {
            calls.fetch_add(1, Ordering::Relaxed);
            Ok(())
        })
        .unwrap();
    assert_eq!(calls.load(Ordering::Relaxed), 1);
    assert_eq!(output.written(), &[0x55, 0]);
}

#[test]
fn rules_cannot_replace_the_document_with_another_package_schema() {
    let package = package_with_all_entries();
    let other_schema = DocumentSchema::new(
        DocumentSchemaId::new("other-schema").unwrap(),
        1,
        "Other",
        vec![
            DocumentField::new(
                DocumentFieldName::new("trace").unwrap(),
                DocumentFieldType::String,
                "Trace",
            )
            .unwrap(),
        ],
    )
    .unwrap();
    let mut executor = executor(&package, ProtocolDirection::Upstream, true, true);
    let result = executor.execute_frame_with_rules(vec![1], |document| {
        *document = Document::new(other_schema);
        Ok(())
    });

    assert!(matches!(
        result,
        Err(ProtocolRuntimeError::EntryPointFailed {
            entry: ProtocolEntryPoint::Decode,
            ..
        })
    ));
}

#[test]
fn encode_configuration_requires_manifest_entry_but_disabled_encode_forwards_origin() {
    let package = CompiledProtocolPackageTestBuilder::new().build();
    assert_eq!(
        DirectionExecutionPlan::new(&package, ProtocolDirection::Upstream, options(false, true))
            .unwrap_err(),
        ProtocolRuntimeError::EntryPointUnavailable {
            package: package.package().clone(),
            direction: ProtocolDirection::Upstream,
            entry: ProtocolEntryPoint::Encode,
        }
    );

    let plan =
        DirectionExecutionPlan::new(&package, ProtocolDirection::Upstream, options(true, false))
            .unwrap();
    let mut executor = ProtocolDirectionExecutor::new(
        &package,
        plan,
        "connection",
        "listener",
        ProtocolRuntimeLimits::default(),
    )
    .unwrap();
    let output = executor.execute_frame(vec![0xaa]).unwrap();
    assert_eq!(output.written(), &[0xaa]);
    assert_eq!(
        executor.render_display(&output),
        ProtocolDisplayResult::HexFallback(DisplayFallbackReason::EncodeDisabled)
    );
}

#[test]
fn executor_revalidates_a_plan_against_the_actual_package() {
    let capable = package_with_all_entries();
    let plan =
        DirectionExecutionPlan::new(&capable, ProtocolDirection::Upstream, options(false, true))
            .unwrap();
    let incapable = CompiledProtocolPackageTestBuilder::new().build();

    assert!(matches!(
        ProtocolDirectionExecutor::new(
            &incapable,
            plan,
            "connection",
            "listener",
            ProtocolRuntimeLimits::default(),
        ),
        Err(ProtocolRuntimeError::EntryPointUnavailable {
            direction: ProtocolDirection::Upstream,
            entry: ProtocolEntryPoint::Encode,
            ..
        })
    ));
}

#[test]
fn missing_and_failed_display_fall_back_without_changing_network_output() {
    let without_display = CompiledProtocolPackageTestBuilder::new()
        .with_script(VALID_SCRIPT)
        .with_upstream_encode()
        .build();
    let mut without_display_executor =
        executor(&without_display, ProtocolDirection::Upstream, true, true);
    let output = without_display_executor.execute_frame(vec![1]).unwrap();
    assert_eq!(output.written(), &[0x55, 7]);
    assert_eq!(
        without_display_executor.render_display(&output),
        ProtocolDisplayResult::HexFallback(DisplayFallbackReason::NotDeclared)
    );

    for display_body in ["throw \"display failed\"", "123"] {
        let script = VALID_SCRIPT.replace(
            "if context.direction() == \"upstream\" { \"upstream-html\" } else { \"downstream-html\" }",
            display_body,
        );
        let package = CompiledProtocolPackageTestBuilder::new()
            .with_script(script)
            .with_upstream_encode()
            .with_display()
            .build();
        let mut executor = executor(&package, ProtocolDirection::Upstream, true, true);
        let output = executor.execute_frame(vec![1]).unwrap();
        assert_eq!(output.written(), &[0x55, 7]);
        assert_eq!(
            executor.render_display(&output),
            ProtocolDisplayResult::HexFallback(DisplayFallbackReason::EntryPointFailed)
        );
        assert_eq!(output.written(), &[0x55, 7]);
    }
}

#[test]
fn display_rejects_output_from_another_executor_even_with_the_same_schema() {
    let package = package_with_all_entries();
    let mut first = executor(&package, ProtocolDirection::Upstream, true, true);
    let mut second = executor(&package, ProtocolDirection::Upstream, true, true);
    let output = first.execute_frame(vec![1]).unwrap();

    assert_eq!(
        second.render_display(&output),
        ProtocolDisplayResult::HexFallback(DisplayFallbackReason::EntryPointFailed)
    );
    assert_eq!(
        first.render_display(&output),
        ProtocolDisplayResult::UntrustedHtml("upstream-html".to_owned())
    );
}

#[test]
fn wrong_decode_and_encode_return_types_fail_closed() {
    let wrong_decode = CompiledProtocolPackageTestBuilder::new()
        .with_script(VALID_SCRIPT.replace("value\n}", "123\n}"))
        .build();
    let mut wrong_decode_executor =
        executor(&wrong_decode, ProtocolDirection::Upstream, true, false);
    assert!(matches!(
        wrong_decode_executor.execute_frame(vec![1]),
        Err(ProtocolRuntimeError::EntryPointFailed {
            entry: ProtocolEntryPoint::Decode,
            ..
        })
    ));

    let wrong_encode = CompiledProtocolPackageTestBuilder::new()
        .with_script(VALID_SCRIPT.replace("result\n}", "123\n}"))
        .with_upstream_encode()
        .build();
    let mut executor = executor(&wrong_encode, ProtocolDirection::Upstream, true, true);
    assert!(matches!(
        executor.execute_frame(vec![1]),
        Err(ProtocolRuntimeError::EntryPointFailed {
            entry: ProtocolEntryPoint::Encode,
            ..
        })
    ));
}

#[test]
fn operation_blob_string_and_wall_time_limits_are_typed_and_do_not_leak_scope() {
    let operation_script = VALID_SCRIPT.replace(
        "let value = document::create();",
        "let i = 0; while i < 10000 { i += 1; } let value = document::create();",
    );
    let package = CompiledProtocolPackageTestBuilder::new()
        .with_script(operation_script)
        .build();
    let limits = ProtocolRuntimeLimits::new(100, 32, 1024, 1024, 100).unwrap();
    let plan =
        DirectionExecutionPlan::new(&package, ProtocolDirection::Upstream, options(true, false))
            .unwrap();
    let mut executor =
        ProtocolDirectionExecutor::new(&package, plan, "connection", "listener", limits).unwrap();
    assert!(matches!(
        executor.execute_frame(vec![1]),
        Err(ProtocolRuntimeError::ResourceLimitExceeded {
            entry: ProtocolEntryPoint::Decode,
            limit: ProtocolResourceLimit::Operations,
            ..
        })
    ));
    assert!(matches!(
        executor.execute_frame(vec![1]),
        Err(ProtocolRuntimeError::ResourceLimitExceeded {
            limit: ProtocolResourceLimit::Operations,
            ..
        })
    ));

    let normal = package_with_all_entries();
    let limits = ProtocolRuntimeLimits::new(10_000, 32, 5, 1, 100).unwrap();
    let plan =
        DirectionExecutionPlan::new(&normal, ProtocolDirection::Upstream, options(true, false))
            .unwrap();
    let mut blob_limited =
        ProtocolDirectionExecutor::new(&normal, plan, "connection", "listener", limits).unwrap();
    assert!(matches!(
        blob_limited.execute_frame(vec![1, 2]),
        Err(ProtocolRuntimeError::ResourceLimitExceeded {
            limit: ProtocolResourceLimit::BlobBytes,
            ..
        })
    ));
}

#[test]
fn call_depth_and_encode_blob_limits_are_classified_by_entry() {
    let recursive = VALID_SCRIPT.replace(
        "fn decode(origin, context) {",
        "fn recurse(value) { recurse(value + 1) }\nfn decode(origin, context) { recurse(0);",
    );
    let package = CompiledProtocolPackageTestBuilder::new()
        .with_script(recursive)
        .build();
    let limits = ProtocolRuntimeLimits::new(100_000, 4, 1024, 1024, 100).unwrap();
    let plan =
        DirectionExecutionPlan::new(&package, ProtocolDirection::Upstream, options(true, false))
            .unwrap();
    let mut executor =
        ProtocolDirectionExecutor::new(&package, plan, "connection", "listener", limits).unwrap();
    assert!(matches!(
        executor.execute_frame(vec![1]),
        Err(ProtocolRuntimeError::ResourceLimitExceeded {
            entry: ProtocolEntryPoint::Decode,
            limit: ProtocolResourceLimit::CallDepth,
            ..
        })
    ));

    let oversized_encode =
        VALID_SCRIPT.replace("let result = blob(2, 0);", "let result = blob(3, 0);");
    let package = CompiledProtocolPackageTestBuilder::new()
        .with_script(oversized_encode)
        .with_upstream_encode()
        .build();
    let limits = ProtocolRuntimeLimits::new(100_000, 32, 1024, 2, 100).unwrap();
    let plan =
        DirectionExecutionPlan::new(&package, ProtocolDirection::Upstream, options(true, true))
            .unwrap();
    let mut executor =
        ProtocolDirectionExecutor::new(&package, plan, "connection", "listener", limits).unwrap();
    assert!(matches!(
        executor.execute_frame(vec![1]),
        Err(ProtocolRuntimeError::ResourceLimitExceeded {
            entry: ProtocolEntryPoint::Encode,
            limit: ProtocolResourceLimit::BlobBytes,
            ..
        })
    ));
}

#[test]
fn display_string_and_wall_time_limits_only_produce_hex_fallback() {
    let long_display = r#"
fn frame(reader, context) { () }
fn decode(origin, context) { document::create() }
fn encode(origin, document, context) { blob(1, 0) }
fn display(document, context) { "123456" }
"#;
    let package = CompiledProtocolPackageTestBuilder::new()
        .with_script(long_display)
        .with_upstream_encode()
        .with_display()
        .build();
    let limits = ProtocolRuntimeLimits::new(100_000, 32, 5, 1024, 100).unwrap();
    let plan =
        DirectionExecutionPlan::new(&package, ProtocolDirection::Upstream, options(false, true))
            .unwrap();
    let mut executor = ProtocolDirectionExecutor::new(&package, plan, "c", "l", limits).unwrap();
    let output = executor.execute_frame(vec![1]).unwrap();
    assert_eq!(output.written(), &[0]);
    assert_eq!(
        executor.render_display(&output),
        ProtocolDisplayResult::HexFallback(DisplayFallbackReason::ResourceLimitExceeded(
            ProtocolResourceLimit::StringBytes
        ))
    );

    let looping_display = VALID_SCRIPT.replace(
        "if context.direction() == \"upstream\" { \"upstream-html\" } else { \"downstream-html\" }",
        "let i = 0; while true { i += 1; } \"unreachable\"",
    );
    let package = CompiledProtocolPackageTestBuilder::new()
        .with_script(looping_display)
        .with_upstream_encode()
        .with_display()
        .build();
    let limits = ProtocolRuntimeLimits::new(10_000_000, 32, 1024, 1024, 1).unwrap();
    let plan =
        DirectionExecutionPlan::new(&package, ProtocolDirection::Upstream, options(true, true))
            .unwrap();
    let mut executor =
        ProtocolDirectionExecutor::new(&package, plan, "connection", "listener", limits).unwrap();
    let output = executor.execute_frame(vec![1]).unwrap();
    assert_eq!(
        executor.render_display(&output),
        ProtocolDisplayResult::HexFallback(DisplayFallbackReason::ResourceLimitExceeded(
            ProtocolResourceLimit::WallTimeMs
        ))
    );
    // 超时调用解除 deadline；后续网络结果仍保持已编码内容，且不会携带上一 Scope 的局部变量。
    assert_eq!(output.written(), &[0x55, 7]);
}

#[test]
fn empty_encode_blob_is_valid_and_html_is_explicitly_untrusted() {
    let script = VALID_SCRIPT
        .replace("let result = blob(2, 0);", "let result = blob();")
        .replace("result[0] = if context.direction() == \"upstream\" { 0x55 } else { 0x44 };", "")
        .replace(
            "result[1] = if document.has(\"amount\") { document.get(\"amount\") } else { 0 };",
            "",
        )
        .replace(
            "if context.direction() == \"upstream\" { \"upstream-html\" } else { \"downstream-html\" }",
            "\"<script>untrusted()</script>\"",
        );
    let package = CompiledProtocolPackageTestBuilder::new()
        .with_script(script)
        .with_upstream_encode()
        .with_display()
        .build();
    let mut executor = executor(&package, ProtocolDirection::Upstream, true, true);
    let output = executor.execute_frame(vec![1]).unwrap();

    assert!(output.written().is_empty());
    assert_eq!(
        executor.render_display(&output),
        ProtocolDisplayResult::UntrustedHtml("<script>untrusted()</script>".to_owned())
    );
}

#[test]
fn direction_has_stable_wire_values() {
    for (direction, wire) in [
        (ProtocolDirection::Upstream, "upstream"),
        (ProtocolDirection::Downstream, "downstream"),
    ] {
        assert_eq!(direction.as_str(), wire);
        assert_eq!(direction.to_string(), wire);
        assert_eq!(serde_json::to_value(direction).unwrap(), wire);
        assert_eq!(
            serde_json::from_value::<ProtocolDirection>(wire.into()).unwrap(),
            direction
        );
    }
}

#[test]
fn executor_debug_is_safe_and_does_not_include_script_source() {
    let package = package_with_all_entries();
    let executor = executor(&package, ProtocolDirection::Upstream, true, true);
    let debug = format!("{executor:?}");

    assert!(debug.contains("test-protocol"));
    assert!(debug.contains("connection-1"));
    assert!(!debug.contains("fn decode"));
    assert!(!debug.contains("upstream-html"));
}
