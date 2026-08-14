use std::fmt;

use serde::Serialize;
use thiserror::Error;

use crate::{PackageFilePath, ProtocolFunctionName, ProtocolPackageParseError};

/// Rhai 脚本编译和入口静态校验的稳定错误分类。
///
/// 这些代码是 UI/应用层可以依赖的机器契约。Rhai 自身的英文错误文本不是产品契约，可能随依赖版本
/// 改变，也可能包含脚本片段，因此不会被保存在公开错误中。
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum ProtocolScriptCompileErrorCode {
    /// 脚本文件不是 UTF-8 文本。
    ScriptNotUtf8,
    /// Rhai 语法无效。
    ScriptSyntaxInvalid,
    /// 脚本使用了宿主明确关闭的能力，例如动态 `eval` 或模块顶层任意计算。
    ForbiddenApi,
    /// `import` 路径不是安全的包根相对 Rhai 路径。
    ModulePathInvalid,
    /// `import` 指向的包内模块不存在。
    ModuleMissing,
    /// 静态模块导入图存在环。
    ModuleCycle,
    /// 模块顶层初始化语句执行失败。
    ModuleInitializationFailed,
    /// 编译或模块初始化触发了操作数、调用深度、模块数或数据大小门禁。
    CompilationLimitExceeded,
    /// Manifest 声明的入口函数不存在。
    EntryPointMissing,
    /// 入口存在但不是宿主可调用的公开顶层函数。
    EntryPointNotPublic,
    /// 入口函数参数数量与 Host API 契约不一致。
    EntryPointArityMismatch,
}

impl ProtocolScriptCompileErrorCode {
    /// 返回持久化和前端映射使用的稳定机器码。
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::ScriptNotUtf8 => "SCRIPT_NOT_UTF8",
            Self::ScriptSyntaxInvalid => "SCRIPT_SYNTAX_INVALID",
            Self::ForbiddenApi => "FORBIDDEN_API",
            Self::ModulePathInvalid => "MODULE_PATH_INVALID",
            Self::ModuleMissing => "MODULE_MISSING",
            Self::ModuleCycle => "MODULE_CYCLE",
            Self::ModuleInitializationFailed => "MODULE_INITIALIZATION_FAILED",
            Self::CompilationLimitExceeded => "COMPILATION_LIMIT_EXCEEDED",
            Self::EntryPointMissing => "ENTRY_POINT_MISSING",
            Self::EntryPointNotPublic => "ENTRY_POINT_NOT_PUBLIC",
            Self::EntryPointArityMismatch => "ENTRY_POINT_ARITY_MISMATCH",
        }
    }
}

impl fmt::Display for ProtocolScriptCompileErrorCode {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.as_str())
    }
}

/// 对协议作者安全的 Rhai 编译诊断。
///
/// `file` 只可能来自已通过 T06/T07 校验的包内相对路径；`entry` 只可能来自 Manifest 中已经校验的
/// Rhai 标识符。错误不会携带脚本内容、原始 `import` 文本、本机绝对路径或 Rhai error source。
#[derive(Clone, Debug, Eq, Error, PartialEq, Serialize)]
#[error("协议脚本编译失败（{code}）")]
pub struct ProtocolScriptCompileError {
    code: ProtocolScriptCompileErrorCode,
    #[serde(skip_serializing_if = "Option::is_none")]
    file: Option<PackageFilePath>,
    #[serde(skip_serializing_if = "Option::is_none")]
    entry: Option<ProtocolFunctionName>,
    #[serde(skip_serializing_if = "Option::is_none")]
    line: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    column: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    expected_arity: Option<u8>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    available_arities: Vec<usize>,
}

impl ProtocolScriptCompileError {
    /// 返回稳定错误分类。
    #[must_use]
    pub const fn code(&self) -> ProtocolScriptCompileErrorCode {
        self.code
    }

    /// 返回发生错误的安全包内路径；无法确定文件时为 `None`。
    #[must_use]
    pub const fn file(&self) -> Option<&PackageFilePath> {
        self.file.as_ref()
    }

    /// 返回校验失败的 Manifest 入口名；语法或模块错误时通常为 `None`。
    #[must_use]
    pub const fn entry(&self) -> Option<&ProtocolFunctionName> {
        self.entry.as_ref()
    }

    /// 返回 Rhai 报告的 1-based 行号。
    #[must_use]
    pub const fn line(&self) -> Option<usize> {
        match self.line {
            Some(line) => Some(line as usize),
            None => None,
        }
    }

    /// 返回 Rhai 报告的 1-based 字符列号。
    #[must_use]
    pub const fn column(&self) -> Option<usize> {
        match self.column {
            Some(column) => Some(column as usize),
            None => None,
        }
    }

    /// 返回 Host API 对该入口要求的参数数量。
    #[must_use]
    pub const fn expected_arity(&self) -> Option<usize> {
        match self.expected_arity {
            Some(arity) => Some(arity as usize),
            None => None,
        }
    }

    /// 返回脚本中同名公开顶层函数实际声明的参数数量，结果已排序去重。
    #[must_use]
    pub fn available_arities(&self) -> &[usize] {
        &self.available_arities
    }

    pub(crate) fn script(
        code: ProtocolScriptCompileErrorCode,
        file: PackageFilePath,
        position: rhai::Position,
    ) -> Self {
        Self {
            code,
            file: Some(file),
            entry: None,
            line: position.line().and_then(|line| u32::try_from(line).ok()),
            column: position
                .position()
                .and_then(|column| u32::try_from(column).ok()),
            expected_arity: None,
            available_arities: Vec::new(),
        }
    }

    pub(crate) fn module_without_file(code: ProtocolScriptCompileErrorCode) -> Self {
        Self {
            code,
            file: None,
            entry: None,
            line: None,
            column: None,
            expected_arity: None,
            available_arities: Vec::new(),
        }
    }

    pub(crate) fn entry_failure(
        code: ProtocolScriptCompileErrorCode,
        file: PackageFilePath,
        entry: ProtocolFunctionName,
        expected_arity: usize,
        mut available_arities: Vec<usize>,
    ) -> Self {
        available_arities.sort_unstable();
        available_arities.dedup();
        Self {
            code,
            file: Some(file),
            entry: Some(entry),
            line: None,
            column: None,
            expected_arity: Some(u8::try_from(expected_arity).unwrap_or(u8::MAX)),
            available_arities,
        }
    }
}

pub(crate) fn error_from_rhai(
    file: PackageFilePath,
    error: &rhai::EvalAltResult,
) -> ProtocolScriptCompileError {
    use rhai::{EvalAltResult, LexError, ParseErrorType};

    let inner = error.unwrap_inner();
    let code = match inner {
        EvalAltResult::ErrorParsing(
            ParseErrorType::BadInput(LexError::ImproperSymbol(symbol, _)),
            _,
        ) if super::engine::FORBIDDEN_SCRIPT_SYMBOLS.contains(&symbol.as_str()) => {
            ProtocolScriptCompileErrorCode::ForbiddenApi
        }
        EvalAltResult::ErrorParsing(..) => ProtocolScriptCompileErrorCode::ScriptSyntaxInvalid,
        EvalAltResult::ErrorTooManyOperations(..)
        | EvalAltResult::ErrorTooManyModules(..)
        | EvalAltResult::ErrorStackOverflow(..)
        | EvalAltResult::ErrorDataTooLarge(..)
        | EvalAltResult::ErrorTerminated(..) => {
            ProtocolScriptCompileErrorCode::CompilationLimitExceeded
        }
        _ => ProtocolScriptCompileErrorCode::ModuleInitializationFailed,
    };
    ProtocolScriptCompileError::script(code, file, inner.position())
}

/// 协议包声明解析或 Rhai 编译失败。
///
/// 两个阶段保留各自稳定、脱敏的错误类型，调用方无需把所有失败压成字符串，也不会接触底层解析器
/// 或 Rhai 的错误对象。
#[derive(Debug, Error)]
pub enum ProtocolPackageCompilationError {
    /// Manifest/Schema 解析或引用文件验证失败。
    #[error(transparent)]
    Declaration(#[from] ProtocolPackageParseError),
    /// Rhai 脚本、模块或入口验证失败。
    #[error(transparent)]
    Script(#[from] ProtocolScriptCompileError),
}

impl ProtocolPackageCompilationError {
    /// 返回声明阶段错误；若失败发生在脚本阶段则为 `None`。
    #[must_use]
    pub const fn declaration_error(&self) -> Option<&ProtocolPackageParseError> {
        match self {
            Self::Declaration(error) => Some(error),
            Self::Script(_) => None,
        }
    }

    /// 返回脚本阶段错误；若失败发生在声明阶段则为 `None`。
    #[must_use]
    pub const fn script_error(&self) -> Option<&ProtocolScriptCompileError> {
        match self {
            Self::Declaration(_) => None,
            Self::Script(error) => Some(error),
        }
    }
}
