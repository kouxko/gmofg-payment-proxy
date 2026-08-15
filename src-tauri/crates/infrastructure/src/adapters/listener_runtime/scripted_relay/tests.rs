//! Scripted Relay processor 资源预算、取消与提交状态回归。

use intercept_proxy_protocol_scripting::ProtocolExecutionCancellation;

use super::{CancelOnDrop, DirectionCommand, discard_skipped_command, processing_budget_ms};

#[test]
fn command_future_drop_cancels_only_while_the_guard_is_armed() {
    let cancelled = ProtocolExecutionCancellation::new();
    drop(CancelOnDrop::new(cancelled.clone()));
    assert!(cancelled.is_cancelled());

    let completed = ProtocolExecutionCancellation::new();
    let mut guard = CancelOnDrop::new(completed.clone());
    guard.disarm();
    drop(guard);
    assert!(!completed.is_cancelled());
}

#[test]
fn processing_budget_covers_display_then_frame_and_decode_then_encode() {
    assert_eq!(processing_budget_ms(10_000, 1), Some(20_250));
    assert_eq!(processing_budget_ms(10_000, 2), Some(20_250));
    assert_eq!(processing_budget_ms(250, 0), Some(750));
}

#[test]
fn processing_budget_overflow_fails_closed() {
    assert_eq!(processing_budget_ms(u64::MAX, 1), None);
    assert_eq!(processing_budget_ms(u64::MAX / 2 + 1, 2), None);
}

#[test]
fn skipped_display_consumes_pending_output_without_running_script() {
    let mut pending_output = Some("committed frame");

    discard_skipped_command(&mut pending_output, &DirectionCommand::CommitDisplay);

    assert_eq!(pending_output, None);
}
