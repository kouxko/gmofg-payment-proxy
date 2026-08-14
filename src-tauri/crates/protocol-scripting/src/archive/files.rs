use std::collections::BTreeMap;

use crate::PackageFilePath;

/// 完整通过安全 ZIP 边界后得到的确定性包内文件集合。
///
/// 文件按 [`PackageFilePath`] 排序，与 ZIP 条目顺序、时间戳和压缩方式无关。目录、原始 ZIP、CRC、
/// 绝对路径和压缩元数据不会进入该模型；任何错误都发生在本类型构造之前。
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ProtocolPackageFiles {
    files: BTreeMap<PackageFilePath, Vec<u8>>,
    total_bytes: u64,
}

impl ProtocolPackageFiles {
    pub(crate) const fn new(files: BTreeMap<PackageFilePath, Vec<u8>>, total_bytes: u64) -> Self {
        Self { files, total_bytes }
    }

    /// 返回普通文件数量；目录条目不进入结果。
    #[must_use]
    pub fn len(&self) -> usize {
        self.files.len()
    }

    /// 返回结果是否没有普通文件。成功读取的协议包因必含 Manifest，恒为 `false`。
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.files.is_empty()
    }

    /// 返回全部文件实际解压后的累计字节数。
    #[must_use]
    pub const fn total_bytes(&self) -> u64 {
        self.total_bytes
    }

    /// 按已校验包内路径读取文件内容。
    #[must_use]
    pub fn get(&self, path: &PackageFilePath) -> Option<&[u8]> {
        self.files.get(path).map(Vec::as_slice)
    }

    /// 返回根目录 `manifest.toml` 字节。
    #[must_use]
    pub fn manifest(&self) -> &[u8] {
        // 该键由 reader 在构造本类型前强制检查；私有构造器保证此处是不变量而非用户输入假设。
        self.files
            .iter()
            .find(|(path, _)| path.as_str() == "manifest.toml")
            .map(|(_, bytes)| bytes.as_slice())
            .expect("ProtocolPackageFiles invariant requires root manifest.toml")
    }

    /// 以稳定路径顺序遍历全部文件。
    pub fn iter(&self) -> impl ExactSizeIterator<Item = (&PackageFilePath, &[u8])> {
        self.files
            .iter()
            .map(|(path, bytes)| (path, bytes.as_slice()))
    }
}
