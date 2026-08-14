use std::fmt;

use rhai::{Engine, ImmutableString};
use serde::{Deserialize, Serialize};

/// Socket Frame 在代理链路中的固定方向。
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ProtocolDirection {
    /// App -> Proxy -> Server。
    Upstream,
    /// Server -> Proxy -> App。
    Downstream,
}

impl ProtocolDirection {
    /// 返回传给 Rhai `context.direction()` 的稳定小写值。
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Upstream => "upstream",
            Self::Downstream => "downstream",
        }
    }
}

impl fmt::Display for ProtocolDirection {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.as_str())
    }
}

/// Host 调用协议入口时暴露给脚本的固定阶段。
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum ProtocolStage {
    /// Frame/Decode 所在的接收阶段。
    Receive,
    /// 只读生成 UI 内容的展示阶段。
    Display,
    /// Encode 所在的发送阶段。
    Send,
}

impl ProtocolStage {
    const fn as_str(self) -> &'static str {
        match self {
            Self::Receive => "receive",
            Self::Display => "display",
            Self::Send => "send",
        }
    }
}

/// 单次协议入口调用的只读上下文。
///
/// 字段私有且 Rhai 只注册四个 getter，没有构造器、setter、Socket 或发送能力。未来执行器每次调用
/// 都构造新值并使用新 Scope，因此脚本拿到的 Clone 不能成为跨 Frame 的宿主状态。
#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct ProtocolCallContext {
    direction: ProtocolDirection,
    stage: ProtocolStage,
    connection_id: String,
    listener_id: String,
}

impl ProtocolCallContext {
    pub(crate) fn new(
        direction: ProtocolDirection,
        stage: ProtocolStage,
        connection_id: impl Into<String>,
        listener_id: impl Into<String>,
    ) -> Self {
        Self {
            direction,
            stage,
            connection_id: connection_id.into(),
            listener_id: listener_id.into(),
        }
    }
}

pub(super) fn register(engine: &mut Engine) {
    engine
        .register_type_with_name::<ProtocolCallContext>("Context")
        .register_fn("direction", |context: &mut ProtocolCallContext| {
            ImmutableString::from(context.direction.as_str())
        })
        .register_fn("stage", |context: &mut ProtocolCallContext| {
            ImmutableString::from(context.stage.as_str())
        })
        .register_fn("connection_id", |context: &mut ProtocolCallContext| {
            ImmutableString::from(context.connection_id.as_str())
        })
        .register_fn("listener_id", |context: &mut ProtocolCallContext| {
            ImmutableString::from(context.listener_id.as_str())
        });
}
