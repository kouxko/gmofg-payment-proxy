use rhai::{Engine, OptimizationLevel};

use crate::ProtocolRuntimeLimits;

/// 单个协议包允许声明的脚本函数总量。
///
/// ZIP 条目数已有独立门禁；本限制进一步避免一个很小的脚本文件生成过大的函数表。
pub(crate) const MAX_SCRIPT_FUNCTIONS: usize = 512;
/// 单个协议包允许嵌入的 Rhai 模块数量。
pub(crate) const MAX_SCRIPT_MODULES: usize = 64;

const MAX_SCRIPT_VARIABLES: usize = 512;
const MAX_SCRIPT_MAP_PROPERTIES: usize = 512;
const MAX_EXPRESSION_DEPTH: usize = 64;
const MAX_FUNCTION_EXPRESSION_DEPTH: usize = 32;

/// Host API v1 明确不提供的动态解释、输出、时钟、文件、网络、进程和环境能力名称。
pub(crate) const FORBIDDEN_SCRIPT_SYMBOLS: &[&str] = &[
    "eval",
    "print",
    "debug",
    "timestamp",
    "open",
    "read_file",
    "write_file",
    "http",
    "socket",
    "process",
    "exec",
    "spawn",
    "env",
];

/// 创建不带文件、网络、进程或 UI 能力的 Rhai Engine。
///
/// `Engine::new` 只提供 Rhai 标准语言包；宿主对象要到 T09/T10 才显式注册。这里额外关闭能够在
/// 脚本运行时再次解释源码的 `eval`，以及可能向宿主输出内容的 `print/debug`。模块解析器由调用方
/// 随后强制替换成包内内存解析器，因此不会使用 Rhai 默认的文件系统解析器。
pub(crate) fn build_engine(limits: ProtocolRuntimeLimits) -> Engine {
    let mut engine = Engine::new();
    engine
        .set_allow_anonymous_fn(false)
        // 先保留原始 AST 供安全策略完整遍历；通过后 Compiler 会显式执行 Simple 优化。
        .set_optimization_level(OptimizationLevel::None)
        .set_max_operations(limits.max_operations())
        .set_max_call_levels(limit_as_usize(limits.max_call_depth()))
        .set_max_string_size(limit_as_usize(limits.max_string_bytes()))
        .set_max_array_size(limit_as_usize(limits.max_blob_bytes()))
        .set_max_map_size(MAX_SCRIPT_MAP_PROPERTIES)
        .set_max_variables(MAX_SCRIPT_VARIABLES)
        .set_max_functions(MAX_SCRIPT_FUNCTIONS)
        .set_max_modules(MAX_SCRIPT_MODULES)
        .set_max_expr_depths(MAX_EXPRESSION_DEPTH, MAX_FUNCTION_EXPRESSION_DEPTH);
    for symbol in FORBIDDEN_SCRIPT_SYMBOLS {
        engine.disable_symbol(*symbol);
    }
    engine
}

fn limit_as_usize(value: u64) -> usize {
    usize::try_from(value).unwrap_or(usize::MAX)
}
