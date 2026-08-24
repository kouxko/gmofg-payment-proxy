//! 供 Socket transport 使用的无 FIFO 生产切帧入口。

use std::{fmt, sync::Arc};

use crate::{
    CompiledProtocolPackage, ProtocolDirection, ProtocolExecutionCancellation,
    ProtocolRuntimeLimits,
    framing::{
        FramingDecision, ProtocolFramingError, ProtocolFramingLimits, ProtocolReader,
        RhaiFrameDecider, validate_decision,
    },
};

/// 当前只读缓冲区的合法 Frame 判断。
///
/// 数值始终已经通过宿主校验：NeedMore 必须大于当前可用长度，Complete 必须是当前缓冲区内
/// 非空且不超过 Frame 上限的前缀，Reject 原因非空且最多 512 个 UTF-8 字节。
#[derive(Clone, Eq, PartialEq)]
pub enum ProtocolFrameInspection {
    /// 当前 Frame 至少需要缓冲到该总字节数。
    NeedMore {
        /// 从当前 Frame 起点计算的目标总长度。
        total: usize,
    },
    /// 当前缓冲区开头的指定字节数构成一个完整 Frame。
    Complete {
        /// 调用方应精确消费的前缀长度。
        bytes: usize,
    },
    /// 当前字节流不属于本协议或已经无法继续解析。
    Reject {
        /// 经过长度校验的协议作者诊断；不得直接作为公开日志或 UI payload 展示。
        reason: String,
    },
}

impl fmt::Debug for ProtocolFrameInspection {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NeedMore { total } => formatter
                .debug_struct("NeedMore")
                .field("total", total)
                .finish(),
            Self::Complete { bytes } => formatter
                .debug_struct("Complete")
                .field("bytes", bytes)
                .finish(),
            // Reject 文本可能由脚本根据输入生成。Debug 只记录有界长度，实际原因仍可由受控
            // Infrastructure 映射为内部诊断，但不会因 `{:?}` 意外进入普通日志。
            Self::Reject { reason } => formatter
                .debug_struct("Reject")
                .field("reason_bytes", &reason.len())
                .finish_non_exhaustive(),
        }
    }
}

/// 单连接、单方向绑定的无 FIFO Frame Inspector。
///
/// Inspector 保存 Rhai Engine、已编译 Frame 入口、资源限制和不可变 Context 身份，但不保存任何
/// 输入字节或消费位置。每次 [`Self::inspect`] 只读取调用方传入的当前完整缓冲区，因此 Socket
/// transport 仍是 FIFO 和消费顺序的唯一所有者。
pub struct ProtocolFrameInspector {
    decider: RhaiFrameDecider,
    framing_limits: ProtocolFramingLimits,
    package: intercept_proxy_domain::ProtocolPackageRef,
    direction: ProtocolDirection,
    connection_id: String,
    listener_id: String,
}

impl ProtocolFrameInspector {
    /// 使用调用方提供的共享取消句柄创建 Inspector。
    ///
    /// 同一连接方向可以把同一个句柄同时传给 Frame Inspector 和方向执行器，使 Frame、Decode、
    /// Encode、Display 对一次连接关闭或任务取消作出一致响应。
    pub fn new_with_cancellation(
        package: &CompiledProtocolPackage,
        direction: ProtocolDirection,
        connection_id: impl Into<String>,
        listener_id: impl Into<String>,
        runtime_limits: ProtocolRuntimeLimits,
        framing_limits: ProtocolFramingLimits,
        cancellation: ProtocolExecutionCancellation,
    ) -> Self {
        let connection_id = connection_id.into();
        let listener_id = listener_id.into();
        Self {
            decider: RhaiFrameDecider::for_package_with_cancellation(
                package,
                direction,
                connection_id.clone(),
                listener_id.clone(),
                runtime_limits,
                cancellation,
            ),
            framing_limits,
            package: package.package().clone(),
            direction,
            connection_id,
            listener_id,
        }
    }

    /// 检查当前从 Frame 起点开始的完整只读缓冲区快照，不保存也不消费输入。
    ///
    /// 输入超过 FIFO 上限会在调用脚本前 fail-closed。其余路径使用与测试切帧器相同的 Reader、
    /// Rhai Frame 入口和 Decision 校验；普通脚本错误返回
    /// [`ProtocolFramingError::FrameEntryFailed`]，显式取消返回
    /// [`ProtocolFramingError::FrameExecutionCancelled`]。两者都不包含源码、底层错误或输入字节。
    pub fn inspect_owned(
        &mut self,
        buffered: Arc<[u8]>,
    ) -> Result<ProtocolFrameInspection, ProtocolFramingError> {
        if u64::try_from(buffered.len())
            .map_or(true, |length| length > self.framing_limits.max_fifo_bytes())
        {
            return Err(ProtocolFramingError::FifoLimitExceeded {
                maximum: self.framing_limits.max_fifo_bytes(),
            });
        }

        // 调用方跨 blocking worker 边界时已经创建一次 owned 快照；Reader 只克隆 Arc，避免
        // Inspector 再复制不断增长的累计缓冲区。
        let buffered_len = buffered.len();
        let reader = ProtocolReader::new(buffered);
        let decision = self.decider.decide(reader)?;
        let decision = validate_decision(
            decision,
            buffered_len,
            self.framing_limits.max_frame_usize(),
        )?;
        Ok(match decision {
            FramingDecision::NeedMore(total) => ProtocolFrameInspection::NeedMore { total },
            FramingDecision::Complete(bytes) => ProtocolFrameInspection::Complete { bytes },
            FramingDecision::Reject(reason) => ProtocolFrameInspection::Reject { reason },
        })
    }

    #[cfg(test)]
    pub(crate) fn inspect(
        &mut self,
        buffered: &[u8],
    ) -> Result<ProtocolFrameInspection, ProtocolFramingError> {
        self.inspect_owned(Arc::from(buffered))
    }
}

impl fmt::Debug for ProtocolFrameInspector {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        // Engine/AST 可能含脚本文本；Debug 只暴露无 payload 的绑定和限制。
        formatter
            .debug_struct("ProtocolFrameInspector")
            .field("package", &self.package)
            .field("direction", &self.direction)
            .field("connection_id", &self.connection_id)
            .field("listener_id", &self.listener_id)
            .field("framing_limits", &self.framing_limits)
            .finish_non_exhaustive()
    }
}
