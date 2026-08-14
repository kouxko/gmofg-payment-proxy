use std::{collections::BTreeMap, fmt, sync::Arc};

use rhai::{AST, FnAccess};

use crate::{
    DirectionHooks, PackageFilePath, ProtocolEntryPoint, ProtocolFunctionName, ProtocolManifest,
};

use super::{ProtocolScriptCompileError, ProtocolScriptCompileErrorCode};

const FRAME_ARITY: usize = 2;
const DECODE_ARITY: usize = 2;
const DISPLAY_ARITY: usize = 2;
const ENCODE_ARITY: usize = 3;

/// 已验证名称、参数数量和可见性的单个 Rhai 入口。
#[derive(Clone)]
#[allow(dead_code)] // AST 与访问器由 T10/T11 的 Framing/Executor 消费；T08 只负责安全冻结。
pub(crate) struct CompiledEntry {
    entry: ProtocolEntryPoint,
    script: PackageFilePath,
    function: ProtocolFunctionName,
    ast: Arc<AST>,
}

impl fmt::Debug for CompiledEntry {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        // AST 可能间接保留作者源码信息，因此 Debug 只输出安全的 Manifest 声明。
        formatter
            .debug_struct("CompiledEntry")
            .field("entry", &self.entry)
            .field("script", &self.script)
            .field("function", &self.function)
            .finish_non_exhaustive()
    }
}

#[allow(dead_code)]
impl CompiledEntry {
    pub(crate) const fn entry(&self) -> ProtocolEntryPoint {
        self.entry
    }

    pub(crate) const fn script(&self) -> &PackageFilePath {
        &self.script
    }

    pub(crate) const fn function(&self) -> &ProtocolFunctionName {
        &self.function
    }

    pub(crate) fn ast(&self) -> &AST {
        &self.ast
    }
}

/// 单方向已经编译的 Frame、Decode 与可选 Encode 入口。
#[derive(Clone, Debug)]
#[allow(dead_code)] // Frame/Decode AST 已在 T08 冻结，运行调用属于后续任务。
pub(crate) struct CompiledDirection {
    frame: CompiledEntry,
    decode: CompiledEntry,
    encode: Option<CompiledEntry>,
}

#[allow(dead_code)]
impl CompiledDirection {
    pub(crate) const fn frame(&self) -> &CompiledEntry {
        &self.frame
    }

    pub(crate) const fn decode(&self) -> &CompiledEntry {
        &self.decode
    }

    pub(crate) const fn encode(&self) -> Option<&CompiledEntry> {
        self.encode.as_ref()
    }
}

pub(crate) fn validate_manifest_entries(
    manifest: &ProtocolManifest,
    scripts: &BTreeMap<PackageFilePath, Arc<AST>>,
) -> Result<(CompiledDirection, CompiledDirection, Option<CompiledEntry>), ProtocolScriptCompileError>
{
    let upstream = validate_direction(manifest.hooks().upstream(), scripts)?;
    let downstream = validate_direction(manifest.hooks().downstream(), scripts)?;
    let display = manifest
        .document()
        .display()
        .map(|declaration| {
            validate_entry(
                ProtocolEntryPoint::Display,
                declaration.script(),
                declaration.function(),
                DISPLAY_ARITY,
                scripts,
            )
        })
        .transpose()?;
    Ok((upstream, downstream, display))
}

fn validate_direction(
    hooks: &DirectionHooks,
    scripts: &BTreeMap<PackageFilePath, Arc<AST>>,
) -> Result<CompiledDirection, ProtocolScriptCompileError> {
    let receive = hooks.receive();
    let frame = validate_entry(
        ProtocolEntryPoint::Frame,
        receive.script(),
        receive.frame(),
        FRAME_ARITY,
        scripts,
    )?;
    let decode = validate_entry(
        ProtocolEntryPoint::Decode,
        receive.script(),
        receive.decode(),
        DECODE_ARITY,
        scripts,
    )?;
    let encode = hooks
        .send()
        .map(|send| {
            validate_entry(
                ProtocolEntryPoint::Encode,
                send.script(),
                send.encode(),
                ENCODE_ARITY,
                scripts,
            )
        })
        .transpose()?;
    Ok(CompiledDirection {
        frame,
        decode,
        encode,
    })
}

fn validate_entry(
    entry: ProtocolEntryPoint,
    script: &PackageFilePath,
    function: &ProtocolFunctionName,
    expected_arity: usize,
    scripts: &BTreeMap<PackageFilePath, Arc<AST>>,
) -> Result<CompiledEntry, ProtocolScriptCompileError> {
    let ast = scripts.get(script).expect(
        "Manifest references are validated and every referenced script is compiled before entries",
    );
    let named = ast
        .iter_functions()
        .filter(|metadata| metadata.name == function.as_str())
        .collect::<Vec<_>>();

    if named.is_empty() {
        return Err(ProtocolScriptCompileError::entry_failure(
            ProtocolScriptCompileErrorCode::EntryPointMissing,
            script.clone(),
            function.clone(),
            expected_arity,
            Vec::new(),
        ));
    }

    let is_callable = named.iter().any(|metadata| {
        metadata.access == FnAccess::Public
            && metadata.this_type.is_none()
            && metadata.params.len() == expected_arity
    });
    if is_callable {
        return Ok(CompiledEntry {
            entry,
            script: script.clone(),
            function: function.clone(),
            ast: Arc::clone(ast),
        });
    }

    let available_arities = named
        .iter()
        .filter(|metadata| metadata.access == FnAccess::Public && metadata.this_type.is_none())
        .map(|metadata| metadata.params.len())
        .collect::<Vec<_>>();
    let code = if available_arities.is_empty() {
        ProtocolScriptCompileErrorCode::EntryPointNotPublic
    } else {
        ProtocolScriptCompileErrorCode::EntryPointArityMismatch
    };
    Err(ProtocolScriptCompileError::entry_failure(
        code,
        script.clone(),
        function.clone(),
        expected_arity,
        available_arities,
    ))
}
