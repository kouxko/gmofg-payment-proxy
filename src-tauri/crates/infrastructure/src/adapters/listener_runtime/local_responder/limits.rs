use intercept_proxy_protocol_scripting::{
    DirectionExecutionPlan, ProtocolFramingLimits, ProtocolRuntimeLimits,
};
use intercept_proxy_runtime::{SocketFramePumpLimits, SocketProcessingFailure};

use super::super::scripted_relay::frame_pump_limits_for_entry_calls;
/// Local process 最多顺序执行 request Decode 与 response Encode，预算按启用入口相加。
pub(crate) fn local_frame_pump_limits(
    runtime: ProtocolRuntimeLimits,
    framing: ProtocolFramingLimits,
    _upstream: DirectionExecutionPlan,
    _downstream: DirectionExecutionPlan,
) -> Result<SocketFramePumpLimits, SocketProcessingFailure> {
    frame_pump_limits_for_entry_calls(runtime, framing, 2)
}
