use crate::ProtocolDirection;

/// 入口冻结后的单方向 Decode/Encode 执行计划。
///
/// Display 没有独立开关；只要运行时产生了可展示 Document，就调用协议包声明的公共 Display。
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct DirectionExecutionPlan {
    direction: ProtocolDirection,
}

impl DirectionExecutionPlan {
    /// 从已编译包建立不可变计划。Manifest 已保证所需入口完整。
    #[must_use]
    pub const fn new(direction: ProtocolDirection) -> Self {
        Self { direction }
    }

    /// 返回该计划绑定的固定 Socket 方向。
    #[must_use]
    pub const fn direction(self) -> ProtocolDirection {
        self.direction
    }
}
