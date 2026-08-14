//! Socket 协议包的隔离编译与执行边界。
//!
//! 本 crate 最终负责 ZIP/Manifest 校验、Rhai 编译以及受限入口调用，但不会访问真实 Socket、
//! 数据库、进程或 UI。这样现有 Direct relay 不需要依赖或初始化脚本引擎；只有选择 Scripted 的
//! Listener 才会由外层基础设施显式构造本 crate 的对象。
//!
//! T05 先冻结三个不会依赖具体脚本引擎的基础契约：不可伪造的编译产物句柄、稳定运行时错误和
//! 受校验资源限制。Manifest、ZIP 与 Rhai 实现分别属于后续任务，避免空壳依赖提前进入构建图。

#![deny(missing_docs)]

mod compiled;
mod error;
mod limits;

pub use compiled::CompiledProtocolPackage;
pub use error::{
    ProtocolEntryPoint, ProtocolResourceLimit, ProtocolRuntimeError, ProtocolRuntimeResult,
};
pub use limits::{
    DEFAULT_MAX_BLOB_BYTES, DEFAULT_MAX_CALL_DEPTH, DEFAULT_MAX_OPERATIONS,
    DEFAULT_MAX_STRING_BYTES, DEFAULT_MAX_WALL_TIME_MS, MAX_BLOB_BYTES_LIMIT, MAX_CALL_DEPTH_LIMIT,
    MAX_OPERATIONS_LIMIT, MAX_STRING_BYTES_LIMIT, MAX_WALL_TIME_MS_LIMIT, ProtocolRuntimeLimits,
};

#[cfg(test)]
mod test_support;
#[cfg(test)]
mod tests;
