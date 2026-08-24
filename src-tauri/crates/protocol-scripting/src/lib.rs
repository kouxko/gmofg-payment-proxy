//! HTTP/Socket 协议包的隔离编译与单阶段执行边界。
//!
//! 本 crate 负责 ZIP/Manifest 校验、Rhai 编译以及受限入口调用，但不会访问真实 HTTP/Socket、
//! 数据库、进程或 UI。未绑定协议包的流量不需要初始化脚本引擎；只有显式选择协议包的 Listener
//! 才会由外层基础设施构造本 crate 的对象。
//!
//! 导入链路会在内存中依次完成 ZIP 限额读取、Manifest/Schema 解析、包内模块解析、Rhai 语法编译
//! 和入口签名校验。只有整条链路全部成功，才会产生 [`CompiledProtocolPackage`]；调用方因而无法把
//! “只解析了一半”的协议包误当成可执行对象。当前运行时已经实现受限 `frame(reader, context)` 与
//! 单方向有界 FIFO，以及 Frame/Decode/Encode/Display 单阶段执行器；HTTP/Socket 数据面由外层
//! runtime 通过 Exchange capability factory 显式接线。

#![deny(missing_docs)]

mod archive;
mod cancellation;
mod compiled;
mod compiler;
mod declaration_name;
mod error;
mod framing;
mod host;
mod limits;
mod manifest;
mod parse_error;
mod rhai_identifier;
mod runtime;
mod schema_parser;
mod toml_parser;

pub use archive::{
    DEFAULT_MAX_ARCHIVE_BYTES, DEFAULT_MAX_ARCHIVE_ENTRIES, DEFAULT_MAX_COMPRESSION_RATIO,
    DEFAULT_MAX_FILE_BYTES, DEFAULT_MAX_PATH_DEPTH, DEFAULT_MAX_TOTAL_BYTES,
    MAX_ARCHIVE_BYTES_LIMIT, MAX_ARCHIVE_ENTRIES_LIMIT, MAX_COMPRESSION_RATIO_LIMIT,
    MAX_FILE_BYTES_LIMIT, MAX_PATH_DEPTH_LIMIT, MAX_TOTAL_BYTES_LIMIT, ProtocolArchiveError,
    ProtocolArchiveErrorCode, ProtocolArchiveLimits, ProtocolPackageFiles,
    read_protocol_package_zip, restore_protocol_package_files,
};
pub use cancellation::ProtocolExecutionCancellation;
pub use compiled::CompiledProtocolPackage;
pub use compiler::{
    ProtocolPackageCompilationError, ProtocolPackageCompiler, ProtocolScriptCompileError,
    ProtocolScriptCompileErrorCode,
};
pub use declaration_name::{
    MAX_PACKAGE_FILE_PATH_BYTES, MAX_PROTOCOL_FUNCTION_NAME_BYTES, PackageFilePath,
    ProtocolFunctionName,
};
pub use error::{
    LocalResponseOwnershipViolation, ProtocolEntryPoint, ProtocolResourceLimit,
    ProtocolRuntimeError, ProtocolRuntimeResult,
};
pub use framing::{
    ProtocolFrameInspection, ProtocolFrameInspector, ProtocolFramingError,
    ProtocolFramingErrorCode, ProtocolFramingLimit, ProtocolFramingLimits,
};
pub use host::context::ProtocolDirection;
pub use limits::{
    DEFAULT_MAX_BLOB_BYTES, DEFAULT_MAX_CALL_DEPTH, DEFAULT_MAX_OPERATIONS,
    DEFAULT_MAX_STRING_BYTES, DEFAULT_MAX_WALL_TIME_MS, MAX_BLOB_BYTES_LIMIT, MAX_CALL_DEPTH_LIMIT,
    MAX_OPERATIONS_LIMIT, MAX_STRING_BYTES_LIMIT, MAX_WALL_TIME_MS_LIMIT, ProtocolRuntimeLimits,
};
pub use manifest::{
    DirectionHooks, DisplayDeclaration, DocumentDeclaration, MAX_MANIFEST_TOML_BYTES,
    ProtocolDocuments, ProtocolHooks, ProtocolManifest, ProtocolPackageKind,
    ProtocolPackageMetadata, SUPPORTED_PROTOCOL_HOST_API, parse_protocol_manifest,
};
pub use parse_error::{
    ProtocolPackageFile, ProtocolPackageParseError, ProtocolPackageParseErrorCode,
};
pub use runtime::{
    DirectionExecutionPlan, DisplayFallbackReason, LocalRequestOutput, LocalResponderCoordinator,
    LocalResponseDisplayHandle, LocalResponseOutput, ProtocolDirectionExecutor,
    ProtocolDisplayResult, ProtocolFrameOutput,
};
pub use schema_parser::{MAX_DOCUMENT_SCHEMA_TOML_BYTES, parse_document_schema};

#[cfg(test)]
mod test_support;
#[cfg(test)]
mod tests;
