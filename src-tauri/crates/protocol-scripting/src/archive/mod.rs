//! 普通 ZIP 协议包与持久化文件集合的有界、确定性内存读取边界。
//!
//! 本模块不调用 `zip::ZipArchive::extract`，不创建临时目录，也不信任中央目录声明的文件大小。
//! 每个条目先验证原始 UTF-8 名称、跨平台相对路径、类型和元数据，再通过额外一字节的读取上限流式
//! 解压。只有全部条目和根目录 `manifest.toml` 都通过后，调用方才会得到 [`ProtocolPackageFiles`]。
//! 从持久化层恢复时同样不信任数据库行，会按实际 `(path, bytes)` 重新执行适用的安全门禁。

mod error;
mod files;
mod limits;
mod reader;
mod restore;

pub use error::{ProtocolArchiveError, ProtocolArchiveErrorCode};
pub use files::ProtocolPackageFiles;
pub use limits::*;
pub use reader::read_protocol_package_zip;
pub use restore::restore_protocol_package_files;
