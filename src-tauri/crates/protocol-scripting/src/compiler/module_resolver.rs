use std::{
    collections::{BTreeMap, BTreeSet},
    fmt,
    sync::{Arc, Mutex, MutexGuard},
};

use rhai::{
    AST, ASTFlags, ASTNode, Engine, EvalAltResult, Expr, Module, ModuleResolver, OptimizationLevel,
    Position, Scope, Stmt,
};

use crate::{PackageFilePath, ProtocolPackageFiles};

use super::error::{ProtocolScriptCompileError, ProtocolScriptCompileErrorCode, error_from_rhai};

/// 只从 T07 已验证文件集合解析脚本模块的 Rhai resolver。
///
/// 本类型没有目录路径、文件句柄或回退 resolver。即使 Rhai 默认支持 `FileModuleResolver`，Compiler
/// 也会用本对象覆盖它，因此 `import` 永远不能读取协议包外的文件系统。
#[derive(Clone)]
pub(crate) struct PackageModuleResolver {
    files: Arc<ProtocolPackageFiles>,
    state: Arc<Mutex<ResolverState>>,
}

#[derive(Default)]
struct ResolverState {
    compiled: BTreeMap<PackageFilePath, AST>,
    seen: BTreeSet<PackageFilePath>,
    counted_functions: BTreeSet<PackageFilePath>,
    function_count: usize,
    stack: Vec<PackageFilePath>,
    failure: Option<ProtocolScriptCompileError>,
}

impl fmt::Debug for PackageModuleResolver {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        let state = self.lock_state();
        formatter
            .debug_struct("PackageModuleResolver")
            .field("file_count", &self.files.len())
            .field("compiled_module_count", &state.compiled.len())
            .field("resolving_depth", &state.stack.len())
            .finish_non_exhaustive()
    }
}

impl PackageModuleResolver {
    pub(crate) fn new(files: Arc<ProtocolPackageFiles>) -> Self {
        Self {
            files,
            state: Arc::new(Mutex::new(ResolverState::default())),
        }
    }

    /// 编译一个 Manifest 直接引用的入口脚本，并把所有静态 import 嵌入 AST。
    ///
    /// 根脚本也进入解析栈，因此模块反向 import 根脚本会和普通模块环一样被确定性拒绝。
    pub(crate) fn compile_root(
        &self,
        engine: &Engine,
        path: &PackageFilePath,
        source: &str,
    ) -> Result<AST, ProtocolScriptCompileError> {
        {
            let mut state = self.lock_state();
            state.failure = None;
            state.stack.clear();
            state.stack.push(path.clone());
        }

        let result = engine.compile_into_self_contained(&Scope::new(), source);
        self.pop_path(path);

        match result {
            Ok(mut ast) => {
                ast.set_source(path.as_str());
                validate_ast_policy(&ast, path)?;
                self.register_function_count(path, &ast)?;
                let mut ast = optimize_preserving_embedded_modules(engine, ast);
                ast.set_source(path.as_str());
                Ok(ast)
            }
            Err(error) => Err(self
                .take_failure()
                .unwrap_or_else(|| error_from_rhai(path.clone(), &error))),
        }
    }

    fn compile_module_ast(
        &self,
        engine: &Engine,
        raw_path: &str,
        position: Position,
    ) -> Result<AST, Box<EvalAltResult>> {
        let Ok(path) = normalize_import_path(raw_path) else {
            let error = ProtocolScriptCompileError::module_without_file(
                ProtocolScriptCompileErrorCode::ModulePathInvalid,
            );
            self.record_failure(error);
            return Err(
                EvalAltResult::ErrorModuleNotFound("invalid module".into(), position).into(),
            );
        };

        {
            let state = self.lock_state();
            if state.stack.contains(&path) {
                drop(state);
                self.record_failure(ProtocolScriptCompileError::script(
                    ProtocolScriptCompileErrorCode::ModuleCycle,
                    path.clone(),
                    position,
                ));
                return Err(EvalAltResult::ErrorInModule(
                    path.as_str().into(),
                    EvalAltResult::ErrorRuntime("module cycle".into(), position).into(),
                    position,
                )
                .into());
            }
            if let Some(ast) = state.compiled.get(&path) {
                return Ok(ast.clone());
            }
        }

        let Some(bytes) = self.files.get(&path) else {
            self.record_failure(ProtocolScriptCompileError::script(
                ProtocolScriptCompileErrorCode::ModuleMissing,
                path.clone(),
                position,
            ));
            return Err(EvalAltResult::ErrorModuleNotFound(path.as_str().into(), position).into());
        };
        let Ok(source) = std::str::from_utf8(bytes) else {
            self.record_failure(ProtocolScriptCompileError::script(
                ProtocolScriptCompileErrorCode::ScriptNotUtf8,
                path.clone(),
                position,
            ));
            return Err(EvalAltResult::ErrorInModule(
                path.as_str().into(),
                EvalAltResult::ErrorRuntime("module is not UTF-8".into(), position).into(),
                position,
            )
            .into());
        };

        {
            let mut state = self.lock_state();
            if !state.seen.contains(&path) && state.seen.len() >= super::engine::MAX_SCRIPT_MODULES
            {
                drop(state);
                self.record_failure(ProtocolScriptCompileError::script(
                    ProtocolScriptCompileErrorCode::CompilationLimitExceeded,
                    path.clone(),
                    position,
                ));
                return Err(EvalAltResult::ErrorTooManyModules(position).into());
            }
            state.seen.insert(path.clone());
        }

        self.lock_state().stack.push(path.clone());
        let result = engine.compile_into_self_contained(&Scope::new(), source);
        self.pop_path(&path);

        match result {
            Ok(ast) => self.finish_module_ast(engine, path, position, ast),
            Err(error) => {
                if self.peek_failure().is_none() {
                    self.record_failure(error_from_rhai(path.clone(), &error));
                }
                Err(error)
            }
        }
    }

    /// 对成功解析的模块执行不会触发 Rhai 求值的策略检查，然后冻结并缓存 AST。
    fn finish_module_ast(
        &self,
        engine: &Engine,
        path: PackageFilePath,
        position: Position,
        mut ast: AST,
    ) -> Result<AST, Box<EvalAltResult>> {
        ast.set_source(path.as_str());
        let policy_result = validate_ast_policy(&ast, &path)
            .and_then(|()| validate_module_top_level(&ast, &path))
            .and_then(|()| self.register_function_count(&path, &ast));
        if let Err(error) = policy_result {
            self.record_failure(error);
            return Err(EvalAltResult::ErrorInModule(
                path.as_str().into(),
                EvalAltResult::ErrorRuntime("module policy rejected the script".into(), position)
                    .into(),
                position,
            )
            .into());
        }

        let mut ast = optimize_preserving_embedded_modules(engine, ast);
        ast.set_source(path.as_str());
        self.lock_state().compiled.insert(path, ast.clone());
        Ok(ast)
    }

    fn record_failure(&self, error: ProtocolScriptCompileError) {
        let mut state = self.lock_state();
        if state.failure.is_none() {
            state.failure = Some(error);
        }
    }

    /// 统计整个协议包的唯一脚本函数数，而不是依赖 Rhai 只针对单个 AST 的限制。
    ///
    /// 同一文件既可能被 Manifest 直接引用，也可能被另一个脚本 import；用规范包路径去重可避免
    /// 重复计数，同时保证把函数分散到多个模块也不能绕过包级上限。
    fn register_function_count(
        &self,
        path: &PackageFilePath,
        ast: &AST,
    ) -> Result<(), ProtocolScriptCompileError> {
        let mut state = self.lock_state();
        if state.counted_functions.contains(path) {
            return Ok(());
        }

        let count = ast.iter_functions().count();
        let Some(total) = state.function_count.checked_add(count) else {
            return Err(ProtocolScriptCompileError::script(
                ProtocolScriptCompileErrorCode::CompilationLimitExceeded,
                path.clone(),
                Position::NONE,
            ));
        };
        if total > super::engine::MAX_SCRIPT_FUNCTIONS {
            return Err(ProtocolScriptCompileError::script(
                ProtocolScriptCompileErrorCode::CompilationLimitExceeded,
                path.clone(),
                Position::NONE,
            ));
        }

        state.function_count = total;
        state.counted_functions.insert(path.clone());
        Ok(())
    }

    fn peek_failure(&self) -> Option<ProtocolScriptCompileError> {
        self.lock_state().failure.clone()
    }

    fn take_failure(&self) -> Option<ProtocolScriptCompileError> {
        self.lock_state().failure.take()
    }

    fn pop_path(&self, path: &PackageFilePath) {
        let mut state = self.lock_state();
        if state.stack.last() == Some(path) {
            state.stack.pop();
        } else {
            // 此分支只用于在第三方 resolver 回调顺序变化时恢复一致状态，不能因为诊断栈异常而 panic。
            state.stack.retain(|candidate| candidate != path);
        }
    }

    fn lock_state(&self) -> MutexGuard<'_, ResolverState> {
        self.state
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
    }
}

/// Rhai 1.25 的 `Engine::optimize_ast` 会重建 AST，但不会复制 self-contained AST 内嵌的
/// `StaticModuleResolver`。如果直接返回优化结果，导入阶段明明已经安全解析并冻结的 `import` 会在
/// 真实入口调用时退化为 `ModuleNotFound`。这里先克隆一个“无语句、无函数、仅保留 resolver”的
/// AST，优化完成后再只合并 resolver；绝不把文件 resolver 或新的脚本内容带回运行时。
fn optimize_preserving_embedded_modules(engine: &Engine, ast: AST) -> AST {
    let embedded_modules = ast.clone_functions_only_filtered(|_, _, _, _, _| false);
    let mut optimized = engine.optimize_ast(&Scope::new(), ast, OptimizationLevel::Simple);
    optimized.combine_filtered(embedded_modules, |_, _, _, _, _| false);
    optimized
}

impl ModuleResolver for PackageModuleResolver {
    fn resolve(
        &self,
        engine: &Engine,
        _source: Option<&str>,
        raw_path: &str,
        position: Position,
    ) -> Result<rhai::Shared<Module>, Box<EvalAltResult>> {
        let path = normalize_import_path(raw_path)
            .map_err(|()| EvalAltResult::ErrorModuleNotFound("invalid module".into(), position))?;
        let ast = self.compile_module_ast(engine, raw_path, position)?;

        self.lock_state().stack.push(path.clone());
        let result = Module::eval_ast_as_new(Scope::new(), &ast, engine);
        self.pop_path(&path);

        match result {
            Ok(mut module) => {
                module.build_index();
                Ok(module.into())
            }
            Err(error) => {
                if self.peek_failure().is_none() {
                    self.record_failure(error_from_rhai(path.clone(), &error));
                }
                Err(EvalAltResult::ErrorInModule(path.as_str().into(), error, position).into())
            }
        }
    }

    fn resolve_ast(
        &self,
        engine: &Engine,
        _source: Option<&str>,
        path: &str,
        position: Position,
    ) -> Option<Result<AST, Box<EvalAltResult>>> {
        Some(self.compile_module_ast(engine, path, position))
    }
}

/// 把 Rhai 的模块名转换为唯一的包根相对 `.rhai` 文件。
///
/// 与文件系统 resolver 不同，这里没有“当前目录”概念。所有模块名都从协议包根解析，路径规则因此在
/// macOS/Windows 上完全一致；扩展名可以省略，但如果作者显式提供扩展名就只能是 `.rhai`。
fn normalize_import_path(raw_path: &str) -> Result<PackageFilePath, ()> {
    let file_name = raw_path.rsplit('/').next().ok_or(())?;
    if file_name.is_empty() || file_name == ".rhai" {
        return Err(());
    }

    let normalized = match file_name.rsplit_once('.') {
        None => format!("{raw_path}.rhai"),
        Some((_, "rhai")) => raw_path.to_owned(),
        Some(_) => return Err(()),
    };
    PackageFilePath::new(normalized).map_err(|_| ())
}

/// 只允许编译期可以完整枚举的静态模块名。
///
/// `compile_into_self_contained` 只会嵌入字符串常量 import。若允许变量形式，模块是否存在就会推迟到
/// 收到真实 Socket 数据之后才暴露，而且可能绕过导入图环检测。固定 Rhai 版本的 `internals` feature
/// 仅在这里用于读取 Import AST，不读取/修改其他内部节点。
fn validate_ast_policy(
    ast: &AST,
    file: &PackageFilePath,
) -> Result<(), ProtocolScriptCompileError> {
    let mut violation = None;
    ast.walk(&mut |path| match path.last() {
        Some(node @ ASTNode::Stmt(Stmt::Import(import, _)))
            if !matches!(import.0, Expr::StringConstant(..)) =>
        {
            violation = Some((
                ProtocolScriptCompileErrorCode::ModulePathInvalid,
                node.position(),
            ));
            false
        }
        Some(node @ ASTNode::Expr(Expr::FnCall(call, _)))
            if super::engine::FORBIDDEN_SCRIPT_SYMBOLS.contains(&call.name.as_str())
                || (!call.namespace.is_empty()
                    && super::engine::FORBIDDEN_SCRIPT_SYMBOLS
                        .contains(&call.namespace.root())) =>
        {
            violation = Some((
                ProtocolScriptCompileErrorCode::ForbiddenApi,
                node.position(),
            ));
            false
        }
        _ => true,
    });

    violation.map_or(Ok(()), |(code, position)| {
        Err(ProtocolScriptCompileError::script(
            code,
            file.clone(),
            position,
        ))
    })
}

/// 拒绝会在导入阶段执行任意计算的模块顶层语句。
///
/// Rhai 必须求值模块 AST 才能建立 import 别名和模块常量。如果允许循环、函数调用、可变变量或
/// 容器常量，攻击者可以在入口尚未调用前构造深层容器，绕开“每个 Rhai 操作”的直觉成本并消耗
/// 宿主栈/内存。模块顶层因此只允许：静态 import、export、空语句，以及直接写出的标量 const。
/// 函数体不在 AST 顶层 statements 中，仍可正常包含协议解析逻辑，并在后续执行阶段受运行时门禁。
fn validate_module_top_level(
    ast: &AST,
    file: &PackageFilePath,
) -> Result<(), ProtocolScriptCompileError> {
    let violation = ast.statements().iter().find(|statement| match statement {
        Stmt::Noop(..) | Stmt::Import(..) | Stmt::Export(..) => false,
        Stmt::Var(variable, flags, ..) => {
            !flags.contains(ASTFlags::CONSTANT) || !is_safe_scalar_constant(&variable.1)
        }
        _ => true,
    });

    violation.map_or(Ok(()), |statement| {
        Err(ProtocolScriptCompileError::script(
            ProtocolScriptCompileErrorCode::ForbiddenApi,
            file.clone(),
            statement.position(),
        ))
    })
}

/// 只接受无需递归求值、克隆或调用宿主函数的标量字面量。
fn is_safe_scalar_constant(expression: &Expr) -> bool {
    matches!(
        expression,
        Expr::BoolConstant(..)
            | Expr::IntegerConstant(..)
            | Expr::CharConstant(..)
            | Expr::StringConstant(..)
            | Expr::Unit(..)
    )
}
