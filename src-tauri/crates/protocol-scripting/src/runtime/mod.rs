//! 完整 Frame 的 Decode、Encode 与 Display 执行边界。
//!
//! 本模块不读取 Socket，也不决定 Frame 边界。调用方先用 framing 层取得完整 Frame，再把该 Frame
//! 交给单方向执行器。网络输出与 UI 展示被刻意拆成两个调用：Display 即使失败，也不能撤销或改变
//! 已经由 Encode 决定的线路字节。

mod deadline;
mod executor;
mod output;
mod plan;

pub use executor::ProtocolDirectionExecutor;
pub use output::{DisplayFallbackReason, ProtocolDisplayResult, ProtocolFrameOutput};
pub use plan::DirectionExecutionPlan;
