//! 两阶段导入在 prepare 与 commit 之间持有的冻结协议包。

use intercept_proxy_protocol_scripting::{CompiledProtocolPackage, ProtocolPackageFiles};

/// 已完整通过 ZIP、Manifest、Schema 与 Rhai 校验，但尚未写入注册表的协议包。
///
/// 字段和构造器保持在注册表模块内可见，只有
/// [`super::ProtocolPackageRepositoryAdapter::prepare_zip`] 能从不可信 ZIP 创建它。调用方
/// 只可读取无源码编译元数据与资源计数，再把对象放入有界 pending import。
pub(crate) struct PreparedProtocolPackage {
    pub(super) files: ProtocolPackageFiles,
    pub(super) compiled: CompiledProtocolPackage,
}

impl PreparedProtocolPackage {
    /// 返回生成安全预览所需的编译结果；不暴露 AST 或脚本内容。
    #[must_use]
    pub(crate) const fn compiled(&self) -> &CompiledProtocolPackage {
        &self.compiled
    }

    /// 返回 pending import 资源记账使用的解压后累计字节数。
    #[must_use]
    pub(crate) const fn total_bytes(&self) -> u64 {
        self.files.total_bytes()
    }
}

impl std::fmt::Debug for PreparedProtocolPackage {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        // ProtocolPackageFiles 的派生 Debug 会打印文件字节，因此这里只输出安全计数和身份。
        formatter
            .debug_struct("PreparedProtocolPackage")
            .field("package", self.compiled.package())
            .field("file_count", &self.files.len())
            .field("total_bytes", &self.files.total_bytes())
            .finish_non_exhaustive()
    }
}
