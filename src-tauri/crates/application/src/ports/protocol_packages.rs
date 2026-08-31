//! 协议包生命周期的应用端口。

use async_trait::async_trait;

use crate::{
    AppResult, ListenerStatusViewModel, ProtocolPackageImportPreviewViewModel,
    ProtocolPackageImportToken, ProtocolPackageImportViewModel, ProtocolPackageRef,
    ProtocolPackageUsageCount, ProtocolPackageUsageViewModel, ProxyWorkspace,
};

#[async_trait]
/// 使用宿主原生文件选择器导入一个 ZIP，并在成功前完成全部校验。
///
/// `WebView` 不提供路径或字节。`None` 只表示取消；非法包不得留下数据库记录或缓存。
pub trait ProtocolPackageImportPort: Send + Sync + std::fmt::Debug {
    async fn prepare_zip(&self) -> AppResult<Option<ProtocolPackageImportPreviewViewModel>>;
    async fn commit_zip(
        &self,
        token: ProtocolPackageImportToken,
    ) -> AppResult<ProtocolPackageImportViewModel>;
    /// 主动释放尚未提交的冻结包；成功后该 token 永久按无效处理。
    async fn discard_zip(&self, token: ProtocolPackageImportToken) -> AppResult<()>;
}

#[async_trait]
/// 官方内置协议包的显式恢复边界。
///
/// 实现必须把编译期资产重新当作不可信 ZIP，完整执行 Archive、Manifest、
/// Schema、JavaScript 和 Host API 校验后才原子替换官方精确身份。
pub trait BuiltinProtocolPackagePort: Send + Sync + std::fmt::Debug {
    /// 返回编译期内置 ZIP 的独立字节副本，供用户直接导出模板。
    ///
    /// 该操作不读取已安装注册表，也不重新打包协议包。
    async fn builtin_archive(&self) -> AppResult<Vec<u8>>;
    async fn restore_builtin(&self) -> AppResult<ProtocolPackageImportViewModel>;
}

#[async_trait]
/// 查询所有 Workspace 中对精确协议包版本的已保存引用，并合并 Listener 运行态。
pub trait ProtocolPackageUsageQueryPort: Send + Sync + std::fmt::Debug {
    async fn usages(
        &self,
        package: &ProtocolPackageRef,
    ) -> AppResult<Vec<ProtocolPackageUsageViewModel>>;
    async fn usage_counts(&self) -> AppResult<Vec<ProtocolPackageUsageCount>>;
    /// 使用调用者已经持有的一致 Workspace 与运行态快照计算引用数，不再次读取仓储。
    async fn usage_counts_for_snapshot(
        &self,
        _workspaces: &[ProxyWorkspace],
        _listener_statuses: &[ListenerStatusViewModel],
    ) -> AppResult<Vec<ProtocolPackageUsageCount>> {
        self.usage_counts().await
    }
}

/// 协议包生命周期与可移植性用例的具名端口集合，供 Host 和测试替身装配。
#[derive(Debug, Clone)]
pub struct ProtocolPackageApplicationServices {
    pub importer: std::sync::Arc<dyn ProtocolPackageImportPort>,
    pub builtin: std::sync::Arc<dyn BuiltinProtocolPackagePort>,
    pub usage_query: std::sync::Arc<dyn ProtocolPackageUsageQueryPort>,
    /// 外部软件包注册表与活动连接端口。
    pub external: std::sync::Arc<dyn crate::ExternalPackageApplicationPort>,
}
