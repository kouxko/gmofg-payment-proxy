//! Socket 协议包的隔离编译与执行边界。
//!
//! 本 crate 最终负责 ZIP/Manifest 校验、Rhai 编译以及受限入口调用，但不会访问真实 Socket、
//! 数据库、进程或 UI。这样现有 Direct relay 不需要依赖或初始化脚本引擎；只有选择 Scripted 的
//! Listener 才会由外层基础设施显式构造本 crate 的对象。
//!
//! 当前导入链路会在内存中依次完成 ZIP 限额读取、Manifest/Schema 解析、包内模块解析、Rhai
//! 语法编译和入口签名校验。只有整条链路全部成功，才会产生 [`CompiledProtocolPackage`]；调用方
//! 因而无法把“只解析了一半”的协议包误当成可执行对象。脚本执行和 Host Document/Context API
//! 仍由后续运行时任务负责，本 crate 在这一阶段不会执行协议入口函数。

#![deny(missing_docs)]

mod archive;
mod compiled;
mod compiler;
mod declaration_name;
mod error;
mod host;
mod limits;
mod manifest;
mod parse_error;
mod schema_parser;
mod toml_parser;

pub use archive::{
    DEFAULT_MAX_ARCHIVE_BYTES, DEFAULT_MAX_ARCHIVE_ENTRIES, DEFAULT_MAX_COMPRESSION_RATIO,
    DEFAULT_MAX_FILE_BYTES, DEFAULT_MAX_PATH_DEPTH, DEFAULT_MAX_TOTAL_BYTES,
    MAX_ARCHIVE_BYTES_LIMIT, MAX_ARCHIVE_ENTRIES_LIMIT, MAX_COMPRESSION_RATIO_LIMIT,
    MAX_FILE_BYTES_LIMIT, MAX_PATH_DEPTH_LIMIT, MAX_TOTAL_BYTES_LIMIT, ProtocolArchiveError,
    ProtocolArchiveErrorCode, ProtocolArchiveLimits, ProtocolPackageFiles,
    read_protocol_package_zip,
};
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
    ProtocolEntryPoint, ProtocolResourceLimit, ProtocolRuntimeError, ProtocolRuntimeResult,
};
pub use limits::{
    DEFAULT_MAX_BLOB_BYTES, DEFAULT_MAX_CALL_DEPTH, DEFAULT_MAX_OPERATIONS,
    DEFAULT_MAX_STRING_BYTES, DEFAULT_MAX_WALL_TIME_MS, MAX_BLOB_BYTES_LIMIT, MAX_CALL_DEPTH_LIMIT,
    MAX_OPERATIONS_LIMIT, MAX_STRING_BYTES_LIMIT, MAX_WALL_TIME_MS_LIMIT, ProtocolRuntimeLimits,
};
pub use manifest::{
    DirectionHooks, DisplayDeclaration, DocumentDeclaration, MAX_MANIFEST_TOML_BYTES,
    ProtocolHooks, ProtocolManifest, ProtocolPackageMetadata, ReceiveHookDeclaration,
    SUPPORTED_PROTOCOL_HOST_API, SendHookDeclaration, parse_protocol_manifest,
};
pub use parse_error::{
    ProtocolPackageFile, ProtocolPackageParseError, ProtocolPackageParseErrorCode,
};
pub use schema_parser::{MAX_DOCUMENT_SCHEMA_TOML_BYTES, parse_document_schema};

#[cfg(test)]
mod test_support;
#[cfg(test)]
mod tests;
