use std::{collections::BTreeMap, fmt, sync::Arc};

use rhai::{AST, FnAccess};

use intercept_proxy_domain::DocumentSchemaNode;

use crate::{
    DirectionHooks, DocumentDeclaration, PackageFilePath, ProtocolEntryPoint, ProtocolFunctionName,
    ProtocolManifest,
};

use super::{ProtocolScriptCompileError, ProtocolScriptCompileErrorCode};

const FRAME_ARITY: usize = 2;
const DECODE_ARITY: usize = 2;
const DISPLAY_ARITY: usize = 2;
const ENCODE_ARITY: usize = 3;

/// 已验证名称、参数数量和可见性的单个 Rhai 入口。
#[derive(Clone)]
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

impl CompiledEntry {
    #[cfg(test)]
    pub(crate) const fn entry(&self) -> ProtocolEntryPoint {
        self.entry
    }

    #[cfg(test)]
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

/// 单方向已经编译的 Schema、Frame、Decode、Encode 与 Display 入口。
#[derive(Clone, Debug)]
pub(crate) struct CompiledDirection {
    schema: Arc<DocumentSchemaNode>,
    frame: Option<CompiledEntry>,
    decode: CompiledEntry,
    encode: CompiledEntry,
    display: CompiledEntry,
}

impl CompiledDirection {
    pub(crate) fn schema(&self) -> &DocumentSchemaNode {
        &self.schema
    }

    pub(crate) fn schema_arc(&self) -> Arc<DocumentSchemaNode> {
        Arc::clone(&self.schema)
    }

    pub(crate) const fn frame(&self) -> Option<&CompiledEntry> {
        self.frame.as_ref()
    }

    pub(crate) const fn decode(&self) -> &CompiledEntry {
        &self.decode
    }

    pub(crate) const fn encode(&self) -> &CompiledEntry {
        &self.encode
    }

    pub(crate) const fn display(&self) -> &CompiledEntry {
        &self.display
    }
}

pub(crate) fn validate_manifest_entries(
    manifest: &ProtocolManifest,
    scripts: &BTreeMap<PackageFilePath, Arc<AST>>,
    upstream_schema: Arc<DocumentSchemaNode>,
    downstream_schema: Arc<DocumentSchemaNode>,
) -> Result<(CompiledDirection, CompiledDirection), ProtocolScriptCompileError> {
    let upstream = validate_direction(
        manifest.hooks().upstream(),
        manifest.document().upstream(),
        upstream_schema,
        scripts,
    )?;
    let downstream = validate_direction(
        manifest.hooks().downstream(),
        manifest.document().downstream(),
        downstream_schema,
        scripts,
    )?;
    Ok((upstream, downstream))
}

fn validate_direction(
    hooks: &DirectionHooks,
    document: &DocumentDeclaration,
    schema: Arc<DocumentSchemaNode>,
    scripts: &BTreeMap<PackageFilePath, Arc<AST>>,
) -> Result<CompiledDirection, ProtocolScriptCompileError> {
    let frame = hooks
        .frame()
        .map(|function| {
            validate_entry(
                ProtocolEntryPoint::Frame,
                hooks.script(),
                function,
                FRAME_ARITY,
                scripts,
            )
        })
        .transpose()?;
    let decode = validate_entry(
        ProtocolEntryPoint::Decode,
        hooks.script(),
        hooks.decode(),
        DECODE_ARITY,
        scripts,
    )?;
    let encode = validate_entry(
        ProtocolEntryPoint::Encode,
        hooks.script(),
        hooks.encode(),
        ENCODE_ARITY,
        scripts,
    )?;
    let display = validate_entry(
        ProtocolEntryPoint::Display,
        document.display().script(),
        document.display().function(),
        DISPLAY_ARITY,
        scripts,
    )?;
    Ok(CompiledDirection {
        schema,
        frame,
        decode,
        encode,
        display,
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
