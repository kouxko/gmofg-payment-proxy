use std::time::Duration;

use intercept_proxy_domain::ProtocolPackageRef;
use rhai::{Engine, Scope};

use crate::{
    CompiledProtocolPackage, ProtocolExecutionCancellation, ProtocolRuntimeLimits,
    compiler::{CompiledEntry, build_engine},
    host::{
        ProtocolHostApi,
        context::{ProtocolCallContext, ProtocolDirection, ProtocolStage},
    },
};

use super::{
    FrameCallDeadline, FrameDecider, FramingDecision, ProtocolFramingError, ProtocolFramingResult,
    ProtocolReader,
};

/// 已绑定单个协议包、单个方向和单连接 Context 的 Rhai frame 调用器。
///
/// Engine 与 AST 都属于该实例；每次调用只创建局部 Scope，不复用脚本变量。它只负责 T10 的
/// Frame 入口；完整 Frame 随后由 runtime 的单方向执行器处理 Decode/Encode/Display。
pub(crate) struct RhaiFrameDecider {
    engine: Engine,
    entry: CompiledEntry,
    context: ProtocolCallContext,
    package: ProtocolPackageRef,
    deadline: FrameCallDeadline,
    wall_time: Duration,
}

impl RhaiFrameDecider {
    pub(crate) fn for_package(
        package: &CompiledProtocolPackage,
        direction: ProtocolDirection,
        connection_id: impl Into<String>,
        listener_id: impl Into<String>,
        runtime_limits: ProtocolRuntimeLimits,
    ) -> Self {
        Self::for_package_with_cancellation(
            package,
            direction,
            connection_id,
            listener_id,
            runtime_limits,
            ProtocolExecutionCancellation::new(),
        )
    }

    pub(crate) fn for_package_with_cancellation(
        package: &CompiledProtocolPackage,
        direction: ProtocolDirection,
        connection_id: impl Into<String>,
        listener_id: impl Into<String>,
        runtime_limits: ProtocolRuntimeLimits,
        cancellation: ProtocolExecutionCancellation,
    ) -> Self {
        let host = ProtocolHostApi::for_package(package);
        let mut engine = build_engine(runtime_limits);
        host.register(&mut engine);
        let deadline = FrameCallDeadline::install(&mut engine, cancellation);
        let entry = match direction {
            ProtocolDirection::Upstream => package.upstream().frame(),
            ProtocolDirection::Downstream => package.downstream().frame(),
        }
        .clone();
        Self {
            engine,
            entry,
            context: ProtocolCallContext::new(
                direction,
                ProtocolStage::Receive,
                connection_id,
                listener_id,
            ),
            package: package.package().clone(),
            deadline,
            wall_time: Duration::from_millis(runtime_limits.max_wall_time_ms()),
        }
    }
}

impl FrameDecider for RhaiFrameDecider {
    fn decide(&mut self, reader: ProtocolReader) -> ProtocolFramingResult<FramingDecision> {
        let started = self.deadline.arm(self.wall_time).map_err(|()| {
            ProtocolFramingError::FrameExecutionCancelled {
                package: self.package.clone(),
            }
        })?;
        let result = self.engine.call_fn::<FramingDecision>(
            &mut Scope::new(),
            self.entry.ast(),
            self.entry.function().as_str(),
            (reader, self.context.clone()),
        );
        let cancelled = self.deadline.was_cancelled();
        self.deadline.disarm();
        if cancelled {
            return Err(ProtocolFramingError::FrameExecutionCancelled {
                package: self.package.clone(),
            });
        }
        match result {
            Ok(_) if started.elapsed() > self.wall_time => {
                Err(ProtocolFramingError::FrameEntryFailed {
                    package: self.package.clone(),
                })
            }
            Ok(decision) => Ok(decision),
            Err(_) => Err(ProtocolFramingError::FrameEntryFailed {
                package: self.package.clone(),
            }),
        }
    }
}
