//! 协议包生命周期的应用端口与缺省 Host 实现。

use async_trait::async_trait;

use crate::{
    AppError, AppResult, ProtocolPackageCompilationReceipt, ProtocolPackageDescriptionViewModel,
    ProtocolPackageImportPreviewViewModel, ProtocolPackageImportToken,
    ProtocolPackageImportViewModel, ProtocolPackageRef, ProtocolPackageUsageCount,
    ProtocolPackageUsageViewModel, ProtocolPackageVersionViewModel,
};

#[async_trait]
/// 协议包持久化的应用语义边界。
///
/// 实现只返回无源码摘要。`set_enabled` 和 `delete` 必须各自是原子存储操作；Application
/// 在调用前负责引用与编译校验，端口不能自行修改 Workspace 或选择其他版本。
pub trait ProtocolPackageStorePort: Send + Sync + std::fmt::Debug {
    async fn list(&self) -> AppResult<Vec<ProtocolPackageVersionViewModel>>;
    async fn get(
        &self,
        package: &ProtocolPackageRef,
    ) -> AppResult<Option<ProtocolPackageVersionViewModel>>;
    async fn set_enabled(&self, package: &ProtocolPackageRef, enabled: bool) -> AppResult<()>;
    async fn delete(&self, package: &ProtocolPackageRef) -> AppResult<()>;
}

#[async_trait]
/// 启用协议包前执行完整恢复、编译与 Host API 兼容性校验。
///
/// 每次启用都必须调用该端口，不能只信任导入时或列表中的历史 `Valid` 状态。
pub trait ProtocolPackageCompilerPort: Send + Sync + std::fmt::Debug {
    async fn validate_for_enable(
        &self,
        package: &ProtocolPackageRef,
    ) -> AppResult<ProtocolPackageCompilationReceipt>;
    /// 从编译产物投影 Schema 和能力；不得返回脚本、AST 或包内路径。
    async fn describe(
        &self,
        package: &ProtocolPackageRef,
    ) -> AppResult<ProtocolPackageDescriptionViewModel>;
}

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
/// 查询所有 Workspace 中对精确协议包版本的已保存引用，并合并 Listener 运行态。
pub trait ProtocolPackageUsageQueryPort: Send + Sync + std::fmt::Debug {
    async fn usages(
        &self,
        package: &ProtocolPackageRef,
    ) -> AppResult<Vec<ProtocolPackageUsageViewModel>>;
    async fn usage_counts(&self) -> AppResult<Vec<ProtocolPackageUsageCount>>;
}

/// 协议包生命周期用例的四个独立端口，Host/测试用它具名装配。
#[derive(Debug, Clone)]
pub struct ProtocolPackageApplicationServices {
    pub store: std::sync::Arc<dyn ProtocolPackageStorePort>,
    pub compiler: std::sync::Arc<dyn ProtocolPackageCompilerPort>,
    pub importer: std::sync::Arc<dyn ProtocolPackageImportPort>,
    pub usage_query: std::sync::Arc<dyn ProtocolPackageUsageQueryPort>,
}

impl ProtocolPackageApplicationServices {
    #[must_use]
    pub fn unavailable() -> Self {
        let unavailable = std::sync::Arc::new(UnavailableProtocolPackageServices);
        Self {
            store: unavailable.clone(),
            compiler: unavailable.clone(),
            importer: unavailable.clone(),
            usage_query: unavailable,
        }
    }
}

#[derive(Debug)]
struct UnavailableProtocolPackageServices;

fn unavailable_protocol_packages<T>() -> AppResult<T> {
    Err(AppError::new(
        "PROTOCOL_PACKAGE_SERVICES_UNAVAILABLE",
        "当前 Host 未提供协议包生命周期服务。",
    ))
}

#[async_trait]
impl ProtocolPackageStorePort for UnavailableProtocolPackageServices {
    async fn list(&self) -> AppResult<Vec<ProtocolPackageVersionViewModel>> {
        unavailable_protocol_packages()
    }

    async fn get(
        &self,
        _: &ProtocolPackageRef,
    ) -> AppResult<Option<ProtocolPackageVersionViewModel>> {
        unavailable_protocol_packages()
    }

    async fn set_enabled(&self, _: &ProtocolPackageRef, _: bool) -> AppResult<()> {
        unavailable_protocol_packages()
    }

    async fn delete(&self, _: &ProtocolPackageRef) -> AppResult<()> {
        unavailable_protocol_packages()
    }
}

#[async_trait]
impl ProtocolPackageCompilerPort for UnavailableProtocolPackageServices {
    async fn validate_for_enable(
        &self,
        _: &ProtocolPackageRef,
    ) -> AppResult<ProtocolPackageCompilationReceipt> {
        unavailable_protocol_packages()
    }

    async fn describe(
        &self,
        _: &ProtocolPackageRef,
    ) -> AppResult<ProtocolPackageDescriptionViewModel> {
        unavailable_protocol_packages()
    }
}

#[async_trait]
impl ProtocolPackageImportPort for UnavailableProtocolPackageServices {
    async fn prepare_zip(&self) -> AppResult<Option<ProtocolPackageImportPreviewViewModel>> {
        unavailable_protocol_packages()
    }

    async fn commit_zip(
        &self,
        _: ProtocolPackageImportToken,
    ) -> AppResult<ProtocolPackageImportViewModel> {
        unavailable_protocol_packages()
    }

    async fn discard_zip(&self, _: ProtocolPackageImportToken) -> AppResult<()> {
        unavailable_protocol_packages()
    }
}

#[async_trait]
impl ProtocolPackageUsageQueryPort for UnavailableProtocolPackageServices {
    async fn usages(
        &self,
        _: &ProtocolPackageRef,
    ) -> AppResult<Vec<ProtocolPackageUsageViewModel>> {
        unavailable_protocol_packages()
    }

    async fn usage_counts(&self) -> AppResult<Vec<ProtocolPackageUsageCount>> {
        unavailable_protocol_packages()
    }
}
