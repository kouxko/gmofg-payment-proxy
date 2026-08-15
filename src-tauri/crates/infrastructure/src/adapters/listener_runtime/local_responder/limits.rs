use intercept_proxy_protocol_scripting::{
    DirectionExecutionPlan, ProtocolFramingLimits, ProtocolRuntimeLimits,
};
use intercept_proxy_runtime::{SocketFramePumpLimits, SocketProcessingFailure};

use super::super::scripted_relay::frame_pump_limits_for_entry_calls;
use super::invalid_limits;

/// Local process 最多顺序执行 request Decode 与 response Encode，预算按启用入口相加。
pub(crate) fn local_frame_pump_limits(
    runtime: ProtocolRuntimeLimits,
    framing: ProtocolFramingLimits,
    upstream: DirectionExecutionPlan,
    downstream: DirectionExecutionPlan,
) -> Result<SocketFramePumpLimits, SocketProcessingFailure> {
    let entry_calls = u64::from(upstream.decode_enabled())
        .checked_add(u64::from(downstream.encode_enabled()))
        .ok_or_else(invalid_limits)?
        .max(1);
    frame_pump_limits_for_entry_calls(runtime, framing, entry_calls)
}
