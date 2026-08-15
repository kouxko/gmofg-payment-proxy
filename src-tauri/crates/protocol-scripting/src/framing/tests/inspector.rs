fn inspector(
    package: &crate::CompiledProtocolPackage,
    direction: ProtocolDirection,
    runtime_limits: ProtocolRuntimeLimits,
    framing_limits: ProtocolFramingLimits,
) -> ProtocolFrameInspector {
    ProtocolFrameInspector::new(
        package,
        direction,
        "connection-production",
        "listener-production",
        runtime_limits,
        framing_limits,
    )
}

fn assert_send<T: Send>() {}

#[test]
fn production_inspector_returns_need_more_complete_and_reject_with_bound_context() {
    let upstream = r#"
fn frame(reader, context) {
    if context.direction() != "upstream"
        || context.stage() != "receive"
        || context.connection_id() != "connection-production"
        || context.listener_id() != "listener-production" {
        return framing::reject("wrong context");
    }
    if reader.available() == 0 { return framing::need_more(1); }
    if reader.peek_u8(0) == 0xff { return framing::reject("unsupported message"); }
    if reader.available() < 2 { framing::need_more(2) }
    else { framing::complete(2) }
}
fn decode(origin, context) { () }
"#;
    let package = compile_package(upstream, valid_fixed_script());
    let mut inspector = inspector(
        &package,
        ProtocolDirection::Upstream,
        ProtocolRuntimeLimits::default(),
        ProtocolFramingLimits::new(8, 8).unwrap(),
    );

    assert_eq!(
        inspector.inspect(&[]).unwrap(),
        ProtocolFrameInspection::NeedMore { total: 1 }
    );
    assert_eq!(
        inspector.inspect(&[1]).unwrap(),
        ProtocolFrameInspection::NeedMore { total: 2 }
    );
    assert_eq!(
        inspector.inspect(&[1, 2, 3]).unwrap(),
        ProtocolFrameInspection::Complete { bytes: 2 }
    );
    assert_eq!(
        inspector.inspect(&[0xff]).unwrap(),
        ProtocolFrameInspection::Reject {
            reason: "unsupported message".to_owned(),
        }
    );
}

#[test]
fn production_inspector_revalidates_invalid_script_decisions() {
    let bodies = [
        (
            "framing::need_more(reader.available())",
            ProtocolFramingErrorCode::NeedMoreWithoutProgress,
        ),
        (
            "framing::complete(0)",
            ProtocolFramingErrorCode::CompleteEmpty,
        ),
        (
            "framing::complete(reader.available() + 1)",
            ProtocolFramingErrorCode::CompleteOutOfBounds,
        ),
    ];
    for (body, expected) in bodies {
        let script = format!(
            "fn frame(reader, context) {{ {body} }}\nfn decode(origin, context) {{ () }}\n"
        );
        let package = compile_package(&script, valid_fixed_script());
        let mut inspector = inspector(
            &package,
            ProtocolDirection::Upstream,
            ProtocolRuntimeLimits::default(),
            ProtocolFramingLimits::new(8, 8).unwrap(),
        );
        assert_eq!(inspector.inspect(&[1]).unwrap_err().code(), expected);
    }

    let wrong_type = compile_package(
        "fn frame(reader, context) { () }\nfn decode(origin, context) { () }\n",
        valid_fixed_script(),
    );
    let mut inspector = inspector(
        &wrong_type,
        ProtocolDirection::Upstream,
        ProtocolRuntimeLimits::default(),
        ProtocolFramingLimits::new(8, 8).unwrap(),
    );
    assert_eq!(
        inspector.inspect(&[1]).unwrap_err(),
        ProtocolFramingError::FrameEntryFailed {
            package: wrong_type.package().clone(),
        }
    );
}

#[test]
fn production_inspector_rejects_frame_and_fifo_oversize_without_retaining_input() {
    let package = compile_package(
        "fn frame(reader, context) { framing::need_more(5) }\nfn decode(origin, context) { () }\n",
        valid_fixed_script(),
    );
    let limits = ProtocolFramingLimits::new(4, 8).unwrap();
    let mut inspector = inspector(
        &package,
        ProtocolDirection::Upstream,
        ProtocolRuntimeLimits::default(),
        limits,
    );

    assert_eq!(
        inspector.inspect(&[1]).unwrap_err(),
        ProtocolFramingError::FrameTooLarge {
            frame_bytes: 5,
            maximum: 4,
        }
    );
    assert_eq!(
        inspector.inspect(&[0; 9]).unwrap_err(),
        ProtocolFramingError::FifoLimitExceeded { maximum: 8 }
    );
}

#[test]
fn production_inspector_enforces_operation_limit_with_redacted_error() {
    let package = compile_package(
        "fn frame(reader, context) { while true {} }\nfn decode(origin, context) { () }\n",
        valid_fixed_script(),
    );
    let runtime_limits = ProtocolRuntimeLimits::new(100, 32, 1024, 1024, 250).unwrap();
    let mut inspector = inspector(
        &package,
        ProtocolDirection::Upstream,
        runtime_limits,
        ProtocolFramingLimits::new(8, 8).unwrap(),
    );

    assert_eq!(
        inspector.inspect(&[1]).unwrap_err(),
        ProtocolFramingError::FrameEntryFailed {
            package: package.package().clone(),
        }
    );
}

#[test]
fn production_inspector_wall_time_interrupts_sync_rhai_and_rearms_next_call() {
    let script = r"
fn frame(reader, context) {
    if reader.peek_u8(0) == 0 { while true {} }
    else { framing::complete(1) }
}
fn decode(origin, context) { () }
";
    let package = compile_package(script, valid_fixed_script());
    let runtime_limits = ProtocolRuntimeLimits::new(10_000_000, 32, 1024, 1024, 1).unwrap();
    let mut inspector = inspector(
        &package,
        ProtocolDirection::Upstream,
        runtime_limits,
        ProtocolFramingLimits::new(8, 8).unwrap(),
    );
    let started = std::time::Instant::now();

    assert_eq!(
        inspector.inspect(&[0]).unwrap_err(),
        ProtocolFramingError::FrameEntryFailed {
            package: package.package().clone(),
        }
    );
    assert!(started.elapsed() < std::time::Duration::from_secs(2));
    assert_eq!(
        inspector.inspect(&[1]).unwrap(),
        ProtocolFrameInspection::Complete { bytes: 1 }
    );
}

#[test]
fn production_inspector_cancels_nonterminating_frame_and_reset_allows_new_call() {
    let script = r"
fn frame(reader, context) {
    if reader.peek_u8(0) == 0 { while true {} }
    else { framing::complete(1) }
}
fn decode(origin, context) { () }
";
    let package = compile_package(script, valid_fixed_script());
    let runtime_limits = ProtocolRuntimeLimits::new(10_000_000, 32, 1024, 1024, 30_000).unwrap();
    let cancellation = ProtocolExecutionCancellation::new();
    let mut inspector = ProtocolFrameInspector::new_with_cancellation(
        &package,
        ProtocolDirection::Upstream,
        "connection-production",
        "listener-production",
        runtime_limits,
        ProtocolFramingLimits::new(8, 8).unwrap(),
        cancellation.clone(),
    );
    assert!(!inspector.cancellation().is_cancelled());

    let canceller = cancellation.clone();
    let cancel_thread = std::thread::spawn(move || {
        std::thread::sleep(std::time::Duration::from_millis(10));
        canceller.cancel();
    });
    let started = std::time::Instant::now();

    let error = inspector.inspect(&[0]).unwrap_err();
    cancel_thread.join().unwrap();
    assert_eq!(
        error,
        ProtocolFramingError::FrameExecutionCancelled {
            package: package.package().clone(),
        }
    );
    assert!(started.elapsed() < std::time::Duration::from_secs(2));
    cancellation.reset();
    assert_eq!(
        inspector.inspect(&[1]).unwrap(),
        ProtocolFrameInspection::Complete { bytes: 1 }
    );
}

#[test]
fn production_inspector_consecutive_results_depend_only_on_current_buffer() {
    let script = r#"
fn frame(reader, context) {
    if reader.available() < 2 { return framing::need_more(2); }
    if reader.peek_u8(0) == 0xbb { framing::complete(2) }
    else { framing::reject("fresh buffer required") }
}
fn decode(origin, context) { () }
"#;
    let package = compile_package(script, valid_fixed_script());
    let mut inspector = inspector(
        &package,
        ProtocolDirection::Upstream,
        ProtocolRuntimeLimits::default(),
        ProtocolFramingLimits::new(8, 8).unwrap(),
    );

    assert_eq!(
        inspector.inspect(&[0xaa]).unwrap(),
        ProtocolFrameInspection::NeedMore { total: 2 }
    );
    assert_eq!(
        inspector.inspect(&[0xbb, 0xcc]).unwrap(),
        ProtocolFrameInspection::Complete { bytes: 2 }
    );
    assert_eq!(
        inspector.inspect(&[0xaa, 0xbb]).unwrap(),
        ProtocolFrameInspection::Reject {
            reason: "fresh buffer required".to_owned(),
        }
    );
}

#[test]
fn production_inspector_debug_does_not_expose_script_or_buffer() {
    let package = compile_package(
        "fn frame(reader, context) { framing::reject(\"secret-script-literal\") }\nfn decode(origin, context) { () }\n",
        valid_fixed_script(),
    );
    let mut inspector = inspector(
        &package,
        ProtocolDirection::Upstream,
        ProtocolRuntimeLimits::default(),
        ProtocolFramingLimits::new(8, 8).unwrap(),
    );
    let _ = inspector.inspect(b"secret-buffer");
    let debug = format!("{inspector:?}");
    let reject_debug = format!(
        "{:?}",
        ProtocolFrameInspection::Reject {
            reason: "secret-reject".to_owned(),
        }
    );

    assert!(debug.contains("connection-production"));
    assert!(!debug.contains("secret-script-literal"));
    assert!(!debug.contains("secret-buffer"));
    assert!(reject_debug.contains("reason_bytes: 13"));
    assert!(!reject_debug.contains("secret-reject"));

    assert_send::<ProtocolFrameInspector>();
}
