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
            ProtocolDisplayResult::UntrustedHtml("upstream-html".to_owned()),
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
            if up_decode || up_encode {
                ProtocolDisplayResult::UntrustedHtml("upstream-html".to_owned())
            } else {
                ProtocolDisplayResult::HexFallback(DisplayFallbackReason::EncodeDisabled)
            }
        );
        assert_eq!(
            downstream.render_display(&down),
            if down_decode || down_encode {
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
fn owned_document_transform_is_atomic_and_skipped_when_decode_is_disabled() {
    let package = package_with_all_entries();
    let mut enabled = executor(&package, ProtocolDirection::Upstream, true, true);
    let output = enabled
        .execute_frame_with_document_transform(vec![1], |mut document| {
            document.set("amount", DocumentValue::Int(42)).unwrap();
            Ok(document)
        })
        .unwrap();
    assert_eq!(output.written(), &[0x55, 42]);

    let mut decode_off = executor(&package, ProtocolDirection::Upstream, false, true);
    let output = decode_off
        .execute_frame_with_document_transform(vec![1], |_| {
            Err(ProtocolRuntimeError::DocumentTransformFailed {
                package: package.package().clone(),
            })
        })
        .unwrap();
    assert_eq!(output.written(), &[0x55, 0]);
}

#[test]
fn frame_output_debug_exposes_shape_without_payload_or_document_values() {
    let package = package_with_all_entries();
    let mut executor = executor(&package, ProtocolDirection::Upstream, true, true);
    let output = executor
        .execute_frame_with_document_transform(vec![0xde, 0xad], |mut document| {
            document.set("amount", DocumentValue::Int(0x55aa)).unwrap();
            Ok(document)
        })
        .unwrap();

    let debug = format!("{output:?}");
    assert!(debug.contains("origin_bytes: 2"));
    assert!(!debug.contains("222"));
    assert!(!debug.contains("173"));
    assert!(!debug.contains("21930"));
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
