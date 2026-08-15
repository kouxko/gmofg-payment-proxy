#[test]
fn cancellation_interrupts_nonterminating_decode_and_reset_rearms_executor() {
    let script = r#"
fn frame(reader, context) { () }
fn decode(origin, context) {
    if origin[0] == 0 { while true {} }
    let value = document::create();
    value.set("amount", origin[0]);
    value
}
"#;
    let package = CompiledProtocolPackageTestBuilder::new()
        .with_script(script)
        .build();
    let plan =
        DirectionExecutionPlan::new(&package, ProtocolDirection::Upstream, options(true, false))
            .unwrap();
    let cancellation = ProtocolExecutionCancellation::new();
    let limits = ProtocolRuntimeLimits::new(10_000_000, 32, 1024, 1024, 30_000).unwrap();
    let mut executor = ProtocolDirectionExecutor::new_with_cancellation(
        &package,
        plan,
        "connection",
        "listener",
        limits,
        cancellation.clone(),
    )
    .unwrap();

    let canceller = cancellation.clone();
    let cancel_thread = std::thread::spawn(move || {
        std::thread::sleep(std::time::Duration::from_millis(10));
        canceller.cancel();
        // reset 紧跟 cancel，模拟连接对象迅速复用控制器。generation 变化仍必须终止旧调用，
        // 但 reset 后开始的新调用应立即可用。
        canceller.reset();
    });
    let started = std::time::Instant::now();
    let error = executor.execute_frame(vec![0]).unwrap_err();
    cancel_thread.join().unwrap();

    assert_eq!(
        error,
        ProtocolRuntimeError::ExecutionCancelled {
            package: package.package().clone(),
            entry: ProtocolEntryPoint::Decode,
        }
    );
    assert!(started.elapsed() < std::time::Duration::from_secs(2));
    assert!(!cancellation.is_cancelled());
    let output = executor.execute_frame(vec![7]).unwrap();
    assert_eq!(
        output.decoded_document().unwrap().get("amount").unwrap(),
        &DocumentValue::Int(7)
    );
}

#[test]
fn cancellation_interrupts_nonterminating_encode_and_reset_rearms_executor() {
    let script = r#"
fn frame(reader, context) { () }
fn decode(origin, context) {
    let value = document::create();
    value.set("amount", origin[0]);
    value
}
fn encode(origin, document, context) {
    if origin[0] == 0 { while true {} }
    origin
}
"#;
    let package = CompiledProtocolPackageTestBuilder::new()
        .with_script(script)
        .with_upstream_encode()
        .build();
    let plan =
        DirectionExecutionPlan::new(&package, ProtocolDirection::Upstream, options(true, true))
            .unwrap();
    let cancellation = ProtocolExecutionCancellation::new();
    let limits = ProtocolRuntimeLimits::new(10_000_000, 32, 1024, 1024, 30_000).unwrap();
    let mut executor = ProtocolDirectionExecutor::new_with_cancellation(
        &package,
        plan,
        "connection",
        "listener",
        limits,
        cancellation.clone(),
    )
    .unwrap();

    let canceller = executor.cancellation();
    let cancel_thread = std::thread::spawn(move || {
        std::thread::sleep(std::time::Duration::from_millis(10));
        canceller.cancel();
    });
    let started = std::time::Instant::now();
    let error = executor.execute_frame(vec![0]).unwrap_err();
    cancel_thread.join().unwrap();

    assert_eq!(
        error,
        ProtocolRuntimeError::ExecutionCancelled {
            package: package.package().clone(),
            entry: ProtocolEntryPoint::Encode,
        }
    );
    assert!(started.elapsed() < std::time::Duration::from_secs(2));
    cancellation.reset();
    assert_eq!(executor.execute_frame(vec![7]).unwrap().written(), &[7]);
}

#[test]
fn pre_cancelled_handle_fails_closed_and_display_uses_same_direction_handle() {
    let package = package_with_all_entries();
    let plan =
        DirectionExecutionPlan::new(&package, ProtocolDirection::Upstream, options(true, true))
            .unwrap();
    let cancellation = ProtocolExecutionCancellation::new();
    let mut executor = ProtocolDirectionExecutor::new_with_cancellation(
        &package,
        plan,
        "connection",
        "listener",
        ProtocolRuntimeLimits::default(),
        cancellation.clone(),
    )
    .unwrap();

    cancellation.cancel();
    assert!(matches!(
        executor.execute_frame(vec![1]),
        Err(ProtocolRuntimeError::ExecutionCancelled {
            entry: ProtocolEntryPoint::Decode,
            ..
        })
    ));

    cancellation.reset();
    let output = executor.execute_frame(vec![1]).unwrap();
    cancellation.cancel();
    assert_eq!(
        executor.render_display(&output),
        ProtocolDisplayResult::HexFallback(DisplayFallbackReason::EntryPointFailed)
    );
    cancellation.reset();
    assert_eq!(
        executor.render_display(&output),
        ProtocolDisplayResult::UntrustedHtml("upstream-html".to_owned())
    );
}
