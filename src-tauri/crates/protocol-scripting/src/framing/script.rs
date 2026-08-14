use intercept_proxy_domain::ProtocolPackageRef;
use rhai::{Engine, Scope};

use crate::{
    CompiledProtocolPackage, ProtocolRuntimeLimits,
    compiler::{CompiledEntry, build_engine},
    host::{
        ProtocolHostApi,
        context::{ProtocolCallContext, ProtocolDirection, ProtocolStage},
    },
};

use super::{
    FrameDecider, FramingDecision, ProtocolFramingError, ProtocolFramingResult, ProtocolReader,
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
}

impl RhaiFrameDecider {
    pub(crate) fn for_package(
        package: &CompiledProtocolPackage,
        direction: ProtocolDirection,
        connection_id: impl Into<String>,
        listener_id: impl Into<String>,
        runtime_limits: ProtocolRuntimeLimits,
    ) -> Self {
        let host = ProtocolHostApi::for_package(package);
        let mut engine = build_engine(runtime_limits);
        host.register(&mut engine);
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
        }
    }
}

impl FrameDecider for RhaiFrameDecider {
    fn decide(&mut self, reader: ProtocolReader) -> ProtocolFramingResult<FramingDecision> {
        self.engine
            .call_fn::<FramingDecision>(
                &mut Scope::new(),
                self.entry.ast(),
                self.entry.function().as_str(),
                (reader, self.context.clone()),
            )
            .map_err(|_| ProtocolFramingError::FrameEntryFailed {
                package: self.package.clone(),
            })
    }
}
