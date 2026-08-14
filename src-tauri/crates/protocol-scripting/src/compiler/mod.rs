mod engine;
mod entry_validation;
mod error;
mod module_resolver;
mod package_compiler;

pub use error::{
    ProtocolPackageCompilationError, ProtocolScriptCompileError, ProtocolScriptCompileErrorCode,
};
pub use package_compiler::ProtocolPackageCompiler;

pub(crate) use engine::build_engine;
pub(crate) use entry_validation::validate_manifest_entries;
pub(crate) use entry_validation::{CompiledDirection, CompiledEntry};
