use intercept_proxy_domain::DirectionProcessingOptions;

use crate::{
    CompiledProtocolPackage, ProtocolDirection, ProtocolEntryPoint, ProtocolRuntimeError,
    ProtocolRuntimeResult,
};

/// Listener 冻结后的单方向 Decode/Encode/Display 执行计划。
///
/// Display 没有独立开关：只有 Encode 已启用且 Manifest 声明了公共 Display 入口时才调用。Encode
/// 被配置为启用但 Manifest 未声明时，构造计划立即失败，避免运行时静默退化为原样转发。
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct DirectionExecutionPlan {
    direction: ProtocolDirection,
    decode_enabled: bool,
    encode_enabled: bool,
    display_enabled: bool,
}

impl DirectionExecutionPlan {
    /// 根据已编译包能力和 Listener 方向开关建立不可变计划。
    pub fn new(
        package: &CompiledProtocolPackage,
        direction: ProtocolDirection,
        options: DirectionProcessingOptions,
    ) -> ProtocolRuntimeResult<Self> {
        let encode_declared = match direction {
            ProtocolDirection::Upstream => package.supports_upstream_encode(),
            ProtocolDirection::Downstream => package.supports_downstream_encode(),
        };
        if options.encode_enabled && !encode_declared {
            return Err(ProtocolRuntimeError::EntryPointUnavailable {
                package: package.package().clone(),
                direction,
                entry: ProtocolEntryPoint::Encode,
            });
        }
        Ok(Self {
            direction,
            decode_enabled: options.decode_enabled,
            encode_enabled: options.encode_enabled,
            display_enabled: options.encode_enabled && package.supports_display(),
        })
    }

    /// 返回该计划绑定的固定 Socket 方向。
    #[must_use]
    pub const fn direction(self) -> ProtocolDirection {
        self.direction
    }

    /// 返回是否调用 Decode。
    #[must_use]
    pub const fn decode_enabled(self) -> bool {
        self.decode_enabled
    }

    /// 返回是否调用 Encode。
    #[must_use]
    pub const fn encode_enabled(self) -> bool {
        self.encode_enabled
    }

    /// 返回是否在输出确定后调用公共 Display。
    #[must_use]
    pub const fn display_enabled(self) -> bool {
        self.display_enabled
    }
}
