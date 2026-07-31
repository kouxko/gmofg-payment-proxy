//! 可复现的弱网调度与 HTTP body 节奏控制。
//!
//! 本模块只描述延迟、抖动、限速、间歇阻断和中途断开，不负责规则匹配；所有等待都必须
//! 观察取消令牌，使 stop/restart 不会被人工故障拖住。

mod deterministic_rng;
mod paced_body;
mod schedule;

pub use paced_body::{PacedBody, PacedBodyError};
pub use schedule::{
    IntermittentProfile, JitterProfile, JitterScope, ThrottleProfile, TrafficDirection,
    TrafficSchedule,
};
