use rhai::{Array, Dynamic, ImmutableString, Scope};

use super::common::engine;
use crate::host::context::{ProtocolCallContext, ProtocolDirection, ProtocolStage};

#[test]
fn context_reports_both_directions_and_all_three_stages_exactly() {
    for (direction, direction_text) in [
        (ProtocolDirection::Upstream, "upstream"),
        (ProtocolDirection::Downstream, "downstream"),
    ] {
        for (stage, stage_text) in [
            (ProtocolStage::Receive, "receive"),
            (ProtocolStage::Display, "display"),
            (ProtocolStage::Send, "send"),
        ] {
            let mut scope = Scope::new();
            scope.push(
                "context",
                ProtocolCallContext::new(direction, stage, "connection-123", "listener-abc"),
            );
            let values = engine()
                .eval_with_scope::<Array>(
                    &mut scope,
                    "[context.direction(), context.stage(), context.connection_id(), context.listener_id()]",
                )
                .unwrap();
            assert_eq!(text(&values[0]), direction_text);
            assert_eq!(text(&values[1]), stage_text);
            assert_eq!(text(&values[2]), "connection-123");
            assert_eq!(text(&values[3]), "listener-abc");
        }
    }
}

#[test]
fn context_has_no_script_constructor_setters_or_external_capabilities() {
    for script in [
        "Context()",
        "context::create()",
        "context.set_direction(\"downstream\")",
        "context.socket()",
        "context.send(blob())",
    ] {
        let mut scope = Scope::new();
        scope.push(
            "context",
            ProtocolCallContext::new(
                ProtocolDirection::Upstream,
                ProtocolStage::Receive,
                "connection-1",
                "listener-1",
            ),
        );
        assert!(
            engine()
                .eval_with_scope::<Dynamic>(&mut scope, script)
                .is_err(),
            "unexpected Context capability: {script}"
        );
    }
}

#[test]
fn context_getters_do_not_mutate_the_host_value() {
    let original = ProtocolCallContext::new(
        ProtocolDirection::Downstream,
        ProtocolStage::Display,
        "connection-stable",
        "listener-stable",
    );
    let mut scope = Scope::new();
    scope.push("context", original.clone());
    let _ = engine()
        .eval_with_scope::<Dynamic>(
            &mut scope,
            "context.direction(); context.stage(); context.connection_id(); context.listener_id();",
        )
        .unwrap();
    assert_eq!(
        scope.get_value::<ProtocolCallContext>("context"),
        Some(original)
    );
}

fn text(value: &Dynamic) -> String {
    value.clone_cast::<ImmutableString>().into_owned()
}
