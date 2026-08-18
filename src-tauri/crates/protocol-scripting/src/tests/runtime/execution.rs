#[test]
fn upstream_executes_decode_encode_and_display_as_one_complete_chain() {
    let package = package_with_all_entries();
    let origin = vec![0x10, 0x20];
    let mut executor = executor(&package, ProtocolDirection::Upstream);
    let output = executor.execute_frame(origin.clone()).unwrap();

    assert_eq!(output.origin(), origin);
    assert_eq!(output.written(), &[0x55, 7]);
    assert_eq!(
        output.decoded_document().unwrap().get("amount").unwrap(),
        &DocumentValue::Int(7)
    );
    assert_eq!(
        executor.render_display(&output),
        ProtocolDisplayResult::UntrustedHtml("upstream-html".to_owned())
    );
}

#[test]
fn bidirectional_complete_chains_are_isolated() {
    let package = package_with_all_entries();
    let mut upstream = executor(&package, ProtocolDirection::Upstream);
    let mut downstream = executor(&package, ProtocolDirection::Downstream);

    let up = upstream.execute_frame(vec![1]).unwrap();
    let down = downstream.execute_frame(vec![2]).unwrap();
    assert_eq!(up.written(), &[0x55, 7]);
    assert_eq!(down.written(), &[0x44, 8]);
    assert_eq!(
        upstream.render_display(&up),
        ProtocolDisplayResult::UntrustedHtml("upstream-html".to_owned())
    );
    assert_eq!(
        downstream.render_display(&down),
        ProtocolDisplayResult::UntrustedHtml("downstream-html".to_owned())
    );
}

#[test]
fn document_rules_run_only_after_decode_and_before_encode() {
    let package = package_with_all_entries();
    let calls = AtomicUsize::new(0);
    let mut enabled = executor(&package, ProtocolDirection::Upstream);
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
}

#[test]
fn owned_document_transform_is_atomic() {
    let package = package_with_all_entries();
    let mut enabled = executor(&package, ProtocolDirection::Upstream);
    let output = enabled
        .execute_frame_with_document_transform(vec![1], |mut document| {
            document.set("amount", DocumentValue::Int(42)).unwrap();
            Ok(document)
        })
        .unwrap();
    assert_eq!(output.written(), &[0x55, 42]);
}

#[test]
fn unchanged_message_document_preserves_the_exact_original_body() {
    let package = package_with_all_entries();
    let origin = vec![0x10, 0x20, 0x30];
    let mut executor = executor(&package, ProtocolDirection::Upstream);

    let output = executor
        .execute_message_with_document_transform(origin.clone(), Ok)
        .unwrap();

    assert_eq!(output.origin(), origin);
    assert_eq!(output.written(), origin);
    assert_eq!(output.decoded_document(), Some(output.execution_document()));
}

#[test]
fn changed_message_document_is_encoded_once() {
    let package = package_with_all_entries();
    let mut executor = executor(&package, ProtocolDirection::Upstream);

    let output = executor
        .execute_message_with_document_transform(vec![0x10], |mut document| {
            document.set("amount", DocumentValue::Int(42)).unwrap();
            Ok(document)
        })
        .unwrap();

    assert_eq!(output.written(), &[0x55, 42]);
    assert_eq!(
        output.decoded_document().unwrap().get("amount").unwrap(),
        &DocumentValue::Int(7)
    );
    assert_eq!(
        output.execution_document().get("amount").unwrap(),
        &DocumentValue::Int(42)
    );
}

#[test]
fn frame_output_debug_exposes_shape_without_payload_or_document_values() {
    let package = package_with_all_entries();
    let mut executor = executor(&package, ProtocolDirection::Upstream);
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
    let mut executor = executor(&package, ProtocolDirection::Upstream);
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
fn failed_display_falls_back_without_changing_network_output() {
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
        let mut executor = executor(&package, ProtocolDirection::Upstream);
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
    let mut first = executor(&package, ProtocolDirection::Upstream);
    let mut second = executor(&package, ProtocolDirection::Upstream);
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
    let mut wrong_decode_executor = executor(&wrong_decode, ProtocolDirection::Upstream);
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
    let mut executor = executor(&wrong_encode, ProtocolDirection::Upstream);
    assert!(matches!(
        executor.execute_frame(vec![1]),
        Err(ProtocolRuntimeError::EntryPointFailed {
            entry: ProtocolEntryPoint::Encode,
            ..
        })
    ));
}
