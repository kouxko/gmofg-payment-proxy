use std::cell::Cell;

use super::*;

fn cancel_on_check(target: usize) -> impl FnMut() -> bool {
    let checks = Cell::new(0);
    move || {
        let current = checks.get() + 1;
        checks.set(current);
        current == target
    }
}

fn input_with_amount(value: i64) -> Document {
    let mut document = Document::new(schema());
    document.set("amount", DocumentValue::Int(value)).unwrap();
    document
}

#[test]
fn cancellation_before_execution_returns_stable_error_without_touching_input() {
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
        vec![],
        vec![set("amount", DocumentValue::Int(200))],
    );
    let program = program(listener_id, vec![candidate]);
    let input = input_with_amount(100);
    let retained = input.clone();

    let error = program
        .execute_with_cancellation(input, || true)
        .unwrap_err();

    assert_eq!(error.code, ErrorCode::RuleExecutionCancelled);
    assert_eq!(error.message, "Socket Document 规则执行已取消");
    assert_eq!(retained.get("amount").unwrap(), &DocumentValue::Int(100));
}

#[test]
fn cancellation_is_checked_at_each_condition_boundary() {
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
        vec![
            condition("amount", DocumentValue::Int(100)),
            condition("approved", DocumentValue::Bool(true)),
        ],
        vec![set("amount", DocumentValue::Int(200))],
    );
    let program = program(listener_id, vec![candidate]);
    let mut input = input_with_amount(100);
    input.set("approved", DocumentValue::Bool(true)).unwrap();

    // 检查顺序：执行入口、规则边界、条件 1、条件 2。第四次返回 true，证明第二个条件前
    // 仍会响应取消，而不是只在规则开始时检查一次。
    let error = program
        .execute_with_cancellation(input, cancel_on_check(4))
        .unwrap_err();

    assert_eq!(error.code, ErrorCode::RuleExecutionCancelled);
}

#[test]
fn cancellation_during_actions_discards_partially_modified_working_document() {
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
        vec![],
        vec![
            set("amount", DocumentValue::Int(200)),
            DocumentAction::ClearDocument,
        ],
    );
    let program = program(listener_id, vec![candidate]);
    let input = input_with_amount(100);
    let retained = input.clone();

    // 入口、规则、动作 1、动作 2。取消发生时动作 1 已修改私有工作副本，但 API 只返回错误，
    // 调用方持有的原始克隆仍保持不变。
    let error = program
        .execute_with_cancellation(input, cancel_on_check(4))
        .unwrap_err();

    assert_eq!(error.code, ErrorCode::RuleExecutionCancelled);
    assert_eq!(retained.get("amount").unwrap(), &DocumentValue::Int(100));
}

#[test]
fn cancellation_between_rules_never_returns_first_rule_partial_result() {
    let listener_id = ListenerId::new();
    let first = rule(
        1,
        listener_id,
        package("1.2.3"),
        7,
        SocketDirection::Downstream,
        true,
        0,
        1,
        vec![],
        vec![set("amount", DocumentValue::Int(200))],
    );
    let second = rule(
        2,
        listener_id,
        package("1.2.3"),
        7,
        SocketDirection::Downstream,
        true,
        1,
        2,
        vec![],
        vec![set("amount", DocumentValue::Int(300))],
    );
    let program = program(listener_id, vec![first, second]);
    let input = input_with_amount(100);

    // 入口、规则 1、动作 1、规则 2。第四次检查在第二条规则边界取消。
    let error = program
        .execute_with_cancellation(input, cancel_on_check(4))
        .unwrap_err();

    assert_eq!(error.code, ErrorCode::RuleExecutionCancelled);
}
