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
    let plan = DirectionExecutionPlan::new(ProtocolDirection::Upstream);
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
    let plan = DirectionExecutionPlan::new(ProtocolDirection::Upstream);
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
    let plan = DirectionExecutionPlan::new(ProtocolDirection::Upstream);
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
    let plan = DirectionExecutionPlan::new(ProtocolDirection::Upstream);
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
    let plan = DirectionExecutionPlan::new(ProtocolDirection::Upstream);
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
    let plan = DirectionExecutionPlan::new(ProtocolDirection::Upstream);
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
