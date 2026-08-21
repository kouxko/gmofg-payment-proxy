//! 外部软件包注册表与连接状态的应用端口。

use async_trait::async_trait;

use crate::{
    AppResult, ExternalPackageDetailViewModel, ExternalPackageServiceStatusViewModel,
    ProtocolPackageDescriptionViewModel, ProtocolPackageRef, ProtocolPackageVersionViewModel,
};

#[async_trait]
/// 外部软件包元数据、用户启用位和活动连接的应用边界。
///
/// 实现返回的版本必须使用 `ProtocolPackageSourceViewModel::External`，并将 `online` 作为
/// 当前连接状态快照。注册首次成功时实现负责持久化为“在线 + 停用”；重连只能改变在线
/// 投影，不能自动启用或启动入口。所有方法都按精确 `(package_id, version)` 操作，不得
/// 自动选择同 ID 的其他版本。
pub trait ExternalPackageApplicationPort: Send + Sync + std::fmt::Debug {
    /// 返回启动期监听结果和当前在线连接数；查询不得触发绑定或重连。
    async fn service_status(&self) -> AppResult<ExternalPackageServiceStatusViewModel>;

    /// 列出全部已注册外部精确版本，包括离线和停用版本。
    async fn list(&self) -> AppResult<Vec<ProtocolPackageVersionViewModel>>;

    /// 查询一个外部精确版本；不存在时返回 `None`。
    async fn get(
        &self,
        package: &ProtocolPackageRef,
    ) -> AppResult<Option<ProtocolPackageVersionViewModel>>;

    /// 返回注册时严格校验并持久化的安全描述，不执行 Rhai 编译或网络 RPC。
    async fn describe(
        &self,
        package: &ProtocolPackageRef,
    ) -> AppResult<ProtocolPackageDescriptionViewModel>;

    /// 返回详情页所需的连接历史、指纹、调用期限和完整方法映射。
    async fn detail(
        &self,
        package: &ProtocolPackageRef,
    ) -> AppResult<ExternalPackageDetailViewModel>;

    /// 原子写入用户启用位；不得改变连接在线状态或启动入口。
    async fn set_enabled(&self, package: &ProtocolPackageRef, enabled: bool) -> AppResult<()>;

    /// 主动关闭当前精确版本的 WebSocket 连接；离线时应幂等成功。
    async fn disconnect(&self, package: &ProtocolPackageRef) -> AppResult<()>;

    /// 删除已由 Application 确认无引用的持久化元数据。
    async fn delete(&self, package: &ProtocolPackageRef) -> AppResult<()>;
}
