use intercept_proxy_protocol_scripting::{
    DirectionExecutionPlan, ProtocolFramingLimits, ProtocolRuntimeLimits,
};
use intercept_proxy_runtime::{SocketPipelineLimits, SocketProcessingFailure};

use super::invalid_limits;

pub(crate) fn pipeline_limits(
    runtime: ProtocolRuntimeLimits,
    framing: ProtocolFramingLimits,
    read_chunk_bytes: usize,
    _upstream: DirectionExecutionPlan,
    _downstream: DirectionExecutionPlan,
) -> Result<SocketPipelineLimits, SocketProcessingFailure> {
    pipeline_limits_for_entry_calls(runtime, framing, read_chunk_bytes, 2)
}

fn pipeline_limits_for_entry_calls(
    runtime: ProtocolRuntimeLimits,
    framing: ProtocolFramingLimits,
    read_chunk_bytes: usize,
    entry_calls: u64,
) -> Result<SocketPipelineLimits, SocketProcessingFailure> {
    processing_budget_ms(runtime.max_wall_time_ms(), entry_calls).ok_or_else(invalid_limits)?;
    let max_buffer_bytes =
        usize::try_from(framing.max_fifo_bytes()).map_err(|_| invalid_limits())?;
    let max_output_bytes = usize::try_from(framing.max_frame_bytes().max(runtime.max_blob_bytes()))
        .map_err(|_| invalid_limits())?;
    SocketPipelineLimits::new(max_buffer_bytes, max_output_bytes, read_chunk_bytes)
}

pub(super) fn processing_budget_ms(max_wall_time_ms: u64, entry_calls: u64) -> Option<u64> {
    const EXTRA_MS: u64 = 250;
    if entry_calls == 0 {
        return None;
    }
    let process = max_wall_time_ms
        .checked_mul(entry_calls)?
        .checked_add(EXTRA_MS)?;
    // 下一次 inspect 的 timeout 也覆盖上一帧提交后的 Display。
    let after_display = max_wall_time_ms.checked_mul(2)?.checked_add(EXTRA_MS)?;
    Some(process.max(after_display))
}
