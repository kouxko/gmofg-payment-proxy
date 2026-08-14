use std::{collections::BTreeMap, sync::Arc};

use rhai::{AST, Position};

use crate::{
    CompiledProtocolPackage, ProtocolPackageFile, ProtocolPackageFiles, ProtocolPackageParseError,
    ProtocolPackageParseErrorCode, ProtocolRuntimeLimits, parse_document_schema,
    parse_protocol_manifest,
};

use super::{
    ProtocolPackageCompilationError, ProtocolScriptCompileError, ProtocolScriptCompileErrorCode,
    build_engine, module_resolver::PackageModuleResolver, validate_manifest_entries,
};

/// 把 T07 的安全文件集合编译成不可伪造协议包句柄。
///
/// Compiler 自己重新解析 Manifest/Schema、验证全部引用、安装包内 resolver、编译每个直接入口脚本，
/// 最后才构造 [`CompiledProtocolPackage`]。调用者无法跳过其中一个阶段创建半成品。
#[derive(Clone, Copy, Debug)]
pub struct ProtocolPackageCompiler {
    limits: ProtocolRuntimeLimits,
}

impl ProtocolPackageCompiler {
    /// 使用已验证运行时限制创建编译器。
    #[must_use]
    pub const fn new(limits: ProtocolRuntimeLimits) -> Self {
        Self { limits }
    }

    /// 完整验证并编译一个协议包文件集合。
    ///
    /// 本方法不访问磁盘、不持久化文件，也不执行任何协议入口。导入模块的顶层只允许静态 import、
    /// export 和直接标量 const；其他初始化计算会在 Rhai 求值前被策略校验拒绝。
    pub fn compile(
        &self,
        files: &ProtocolPackageFiles,
    ) -> Result<CompiledProtocolPackage, ProtocolPackageCompilationError> {
        let manifest_source = std::str::from_utf8(files.manifest()).map_err(|_| {
            ProtocolPackageParseError::new(
                ProtocolPackageParseErrorCode::TomlInvalid,
                ProtocolPackageFile::Manifest,
                "$",
            )
        })?;
        let manifest = parse_protocol_manifest(manifest_source)?;

        let schema_bytes = files.get(manifest.document().schema()).ok_or_else(|| {
            ProtocolPackageParseError::new(
                ProtocolPackageParseErrorCode::ReferencedFileMissing,
                ProtocolPackageFile::Manifest,
                "document.schema",
            )
        })?;
        let schema_source = std::str::from_utf8(schema_bytes).map_err(|_| {
            ProtocolPackageParseError::new(
                ProtocolPackageParseErrorCode::TomlInvalid,
                ProtocolPackageFile::DocumentSchema,
                "$",
            )
        })?;
        let schema = Arc::new(parse_document_schema(schema_source)?);

        let available = files.iter().map(|(path, _)| path.clone()).collect();
        manifest.validate_referenced_files(&available)?;

        let resolver = PackageModuleResolver::new(Arc::new(files.clone()));
        let mut engine = build_engine(self.limits);
        // Rhai 的标准 Engine 在原生平台可配置文件 resolver；这里无条件覆盖为包内内存 resolver。
        engine.set_module_resolver(resolver.clone());

        let mut script_paths = manifest.referenced_files();
        script_paths.remove(manifest.document().schema());
        let mut scripts: BTreeMap<_, Arc<AST>> = BTreeMap::new();
        for path in script_paths {
            // `validate_referenced_files` 已在同一不可变集合上验证全部 Manifest 引用；这里是内部不变量。
            let bytes = files
                .get(path)
                .expect("validated Manifest script reference must remain available");
            let source = std::str::from_utf8(bytes).map_err(|_| {
                ProtocolScriptCompileError::script(
                    ProtocolScriptCompileErrorCode::ScriptNotUtf8,
                    path.clone(),
                    Position::NONE,
                )
            })?;
            let ast = resolver.compile_root(&engine, path, source)?;
            scripts.insert(path.clone(), Arc::new(ast));
        }

        let (upstream, downstream, display) = validate_manifest_entries(&manifest, &scripts)?;
        Ok(CompiledProtocolPackage::from_compilation(
            manifest, schema, upstream, downstream, display,
        ))
    }
}

impl Default for ProtocolPackageCompiler {
    fn default() -> Self {
        Self::new(ProtocolRuntimeLimits::default())
    }
}
