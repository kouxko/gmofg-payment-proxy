use std::sync::Arc;

use super::*;

#[test]
fn constructor_rejects_every_binding_mismatch_and_duplicate_identity() {
    let listener_id = ListenerId::new();
    let valid = rule(
        1,
        listener_id,
        package("1.2.3"),
        7,
        SocketDirection::Downstream,
        true,
        0,
        1,
        Vec::new(),
        vec![DocumentAction::RecordMatch],
    );
    let mismatches = [
        rule(
            2,
            ListenerId::new(),
            package("1.2.3"),
            7,
            SocketDirection::Downstream,
            true,
            0,
            2,
            Vec::new(),
            vec![DocumentAction::RecordMatch],
        ),
        rule(
            3,
            listener_id,
            package("2.0.0"),
            7,
            SocketDirection::Downstream,
            true,
            0,
            3,
            Vec::new(),
            vec![DocumentAction::RecordMatch],
        ),
        rule(
            4,
            listener_id,
            package("1.2.3"),
            8,
            SocketDirection::Downstream,
            true,
            0,
            4,
            Vec::new(),
            vec![DocumentAction::RecordMatch],
        ),
        rule(
            5,
            listener_id,
            package("1.2.3"),
            7,
            SocketDirection::Upstream,
            true,
            0,
            5,
            Vec::new(),
            vec![DocumentAction::RecordMatch],
        ),
    ];
    for mismatch in mismatches {
        assert_eq!(
            SocketDocumentRuleProgram::new(
                listener_id,
                package("1.2.3"),
                schema(),
                SocketDirection::Downstream,
                vec![mismatch],
            )
            .unwrap_err()
            .code,
            ErrorCode::RuleInvalid
        );
    }
    assert_eq!(
        SocketDocumentRuleProgram::new(
            listener_id,
            package("1.2.3"),
            schema(),
            SocketDirection::Downstream,
            vec![valid.clone(), valid],
        )
        .unwrap_err()
        .code,
        ErrorCode::RuleInvalid
    );
}

#[test]
fn execution_rejects_non_exact_schema_before_any_rule_action() {
    let listener_id = ListenerId::new();
    let candidate = rule(
        1,
        listener_id,
        package("1.2.3"),
        7,
        SocketDirection::Downstream,
        true,
        0,
        1,
        Vec::new(),
        vec![DocumentAction::ClearDocument],
    );
    let program = program(listener_id, vec![candidate]);
    let different_schema = DocumentSchema::new(
        DocumentSchemaId::new("message").unwrap(),
        7,
        "Different title",
        schema().fields().to_vec(),
    )
    .unwrap();
    let mut original = Document::new(different_schema);
    original
        .set("text", DocumentValue::String("unchanged".into()))
        .unwrap();

    assert_eq!(
        program.execute(original.clone()).unwrap_err().code,
        ErrorCode::RuleInvalid
    );
    assert_eq!(
        original.get("text").unwrap(),
        &DocumentValue::String("unchanged".into())
    );
}

#[test]
fn consecutive_and_concurrent_executions_do_not_share_document_state() {
    let listener_id = ListenerId::new();
    let one = rule(
        1,
        listener_id,
        package("1.2.3"),
        7,
        SocketDirection::Downstream,
        true,
        0,
        1,
        vec![condition("amount", DocumentValue::Int(1))],
        vec![set("text", DocumentValue::String("one".into()))],
    );
    let two = rule(
        2,
        listener_id,
        package("1.2.3"),
        7,
        SocketDirection::Downstream,
        true,
        0,
        2,
        vec![condition("amount", DocumentValue::Int(2))],
        vec![set("text", DocumentValue::String("two".into()))],
    );
    let program = Arc::new(program(listener_id, vec![two, one]));

    for amount in [1, 2, 1, 2] {
        assert_isolated_result(&program, amount);
    }
    let handles = (0..16)
        .map(|index| {
            let program = Arc::clone(&program);
            std::thread::spawn(move || assert_isolated_result(&program, index % 2 + 1))
        })
        .collect::<Vec<_>>();
    for handle in handles {
        handle.join().unwrap();
    }
}

fn assert_isolated_result(program: &SocketDocumentRuleProgram, amount: i64) {
    let mut input = Document::new(schema());
    input.set("amount", DocumentValue::Int(amount)).unwrap();
    let result = program.execute(input).unwrap();
    let expected = if amount == 1 { "one" } else { "two" };
    assert_eq!(
        result.document().get("text").unwrap(),
        &DocumentValue::String(expected.into())
    );
    assert_eq!(result.matched_rule_ids().len(), 1);
}

#[test]
fn debug_output_exposes_shape_but_not_document_or_rule_values() {
    let listener_id = ListenerId::new();
    let candidate = rule(
        1,
        listener_id,
        package("1.2.3"),
        7,
        SocketDirection::Downstream,
        true,
        0,
        1,
        Vec::new(),
        vec![set("text", DocumentValue::String("secret-value".into()))],
    );
    let program = program(listener_id, vec![candidate]);
    let execution = program.execute(Document::new(schema())).unwrap();

    let program_debug = format!("{program:?}");
    let execution_debug = format!("{execution:?}");
    assert!(program_debug.contains("rule_count: 1"));
    assert!(!program_debug.contains("secret-value"));
    assert!(!execution_debug.contains("secret-value"));
}
