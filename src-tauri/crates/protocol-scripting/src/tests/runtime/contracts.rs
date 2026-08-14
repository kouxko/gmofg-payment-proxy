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
