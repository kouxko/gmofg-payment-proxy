//! 拦截规则、校验与执行引擎。
//!
//! 公开类型仍从本模块导出，内部按“类型、匹配、校验、执行”拆分。这样序列化路径和
//! 调用方 API 保持稳定，同时各文件只承担一种职责。

mod matching;
mod types;
mod validation;

pub use matching::{matches_http_condition, validate_http_condition};
pub use types::*;
pub(crate) use validation::validate_http_rule;
