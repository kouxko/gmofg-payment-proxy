use std::time::Duration;

use intercept_proxy_protocol_scripting::{
    DirectionExecutionPlan, ProtocolFramingLimits, ProtocolRuntimeLimits,
};
use intercept_proxy_runtime::{SocketFramePumpLimits, SocketProcessingFailure};

use super::invalid_limits;

pub(crate) fn frame_pump_limits(
    runtime: ProtocolRuntimeLimits,
    framing: ProtocolFramingLimits,
    upstream: DirectionExecutionPlan,
    downstream: DirectionExecutionPlan,
) -> Result<SocketFramePumpLimits, SocketProcessingFailure> {
    let entry_calls = [upstream, downstream]
        .into_iter()
        .map(|plan| u64::from(plan.decode_enabled()) + u64::from(plan.encode_enabled()))
        .max()
        .unwrap_or(0)
        .max(1);
    frame_pump_limits_for_entry_calls(runtime, framing, entry_calls)
}

pub(crate) fn frame_pump_limits_for_entry_calls(
    runtime: ProtocolRuntimeLimits,
    framing: ProtocolFramingLimits,
    entry_calls: u64,
) -> Result<SocketFramePumpLimits, SocketProcessingFailure> {
    const READ_CHUNK_BYTES: usize = 16 * 1024;
    let processing_ms =
        processing_budget_ms(runtime.max_wall_time_ms(), entry_calls).ok_or_else(invalid_limits)?;
    let max_buffer_bytes =
        usize::try_from(framing.max_fifo_bytes()).map_err(|_| invalid_limits())?;
    let max_output_bytes = usize::try_from(framing.max_frame_bytes().max(runtime.max_blob_bytes()))
        .map_err(|_| invalid_limits())?;
    SocketFramePumpLimits::new(
        max_buffer_bytes,
        max_output_bytes,
        READ_CHUNK_BYTES.min(max_buffer_bytes),
        Duration::from_millis(processing_ms),
    )
}

pub(super) fn processing_budget_ms(max_wall_time_ms: u64, entry_calls: u64) -> Option<u64> {
    const EXTRA_MS: u64 = 250;
    let process = max_wall_time_ms
        .checked_mul(entry_calls.max(1))?
        .checked_add(EXTRA_MS)?;
    // 下一次 inspect 的 timeout 也覆盖上一帧提交后的 Display。
    let after_display = max_wall_time_ms.checked_mul(2)?.checked_add(EXTRA_MS)?;
    Some(process.max(after_display))
}
