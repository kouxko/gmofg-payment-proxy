//! 协议包生命周期的应用端口与缺省 Host 实现。

use async_trait::async_trait;

use crate::{
    AppError, AppResult, ApplicationBackupProtocolPackageBaseline,
    ApplicationConfigurationDocument, PortableApplicationProtocolPackage, PortableProtocolPackage,
    ProtocolPackageCompilationReceipt, ProtocolPackageDescriptionViewModel,
    ProtocolPackageImportPreviewViewModel, ProtocolPackageImportToken,
    ProtocolPackageImportViewModel, ProtocolPackageRef, ProtocolPackageUsageCount,
    ProtocolPackageUsageViewModel, ProtocolPackageVersionViewModel, ProxyWorkspace,
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
/// 协议包进入启用态、Listener 配置或运行快照前，执行完整恢复、
/// 编译与 Host API 兼容性校验。
///
/// 每次调用都必须从已安装的规范文件 fresh 构建结果，不能只信任
/// 导入时或列表中的历史 `Valid` 状态。方法名保留 `validate_for_enable`
/// 以维持已有 Host 端口兼容，但其语义是可安全加载的 fresh receipt。
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
/// 官方内置协议包的显式恢复边界。
///
/// 实现必须把编译期资产重新当作不可信 ZIP，完整执行 Archive、Manifest、
/// Schema、Rhai 和 Host API 校验后才原子替换官方精确身份。
pub trait BuiltinProtocolPackagePort: Send + Sync + std::fmt::Debug {
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
}

#[async_trait]
/// 可移植文档与协议包注册表之间的原子边界。
///
/// `preflight_*` 只恢复并编译内存中的规范文件，不写数据库或缓存；`commit_*` 必须在
/// 事务开始前重新执行相同恢复/编译链，再把协议包和 Workspace/完整配置作为一个
/// `SQLite` 事务提交。相同身份但内容不同必须整体失败，不能覆盖本机已安装版本。
pub trait ProtocolPackagePortabilityPort: Send + Sync + std::fmt::Debug {
    /// Stable persisted generations used to detect registry changes after import preview.
    async fn application_backup_baseline(
        &self,
    ) -> AppResult<Vec<ApplicationBackupProtocolPackageBaseline>>;
    /// 导出 Workspace 精确引用的包，不携带本机启用状态。
    async fn export_workspace_packages(
        &self,
        packages: &[ProtocolPackageRef],
    ) -> AppResult<Vec<PortableProtocolPackage>>;
    /// 导出整个应用注册表，并保留每个精确版本的启用状态。
    async fn export_application_packages(
        &self,
    ) -> AppResult<Vec<PortableApplicationProtocolPackage>>;
    /// 对 Workspace 文档内嵌包执行无副作用的完整恢复与编译。
    async fn preflight_workspace_packages(
        &self,
        packages: &[PortableProtocolPackage],
    ) -> AppResult<Vec<ProtocolPackageDescriptionViewModel>>;
    /// 对完整配置内嵌包执行无副作用的完整恢复与编译。
    async fn preflight_application_packages(
        &self,
        packages: &[PortableApplicationProtocolPackage],
    ) -> AppResult<Vec<ProtocolPackageDescriptionViewModel>>;
    /// 只读重验本机已安装精确包；不得更新 validation 状态或编译缓存。
    async fn preflight_installed_packages(
        &self,
        packages: &[ProtocolPackageRef],
    ) -> AppResult<Vec<ProtocolPackageDescriptionViewModel>>;
    /// 原子安装 Workspace 缺少的包（默认停用）并插入已重映射的 Workspace。
    async fn commit_workspace_bundle(
        &self,
        packages: Vec<PortableProtocolPackage>,
        workspace: ProxyWorkspace,
    ) -> AppResult<()>;
    /// 导入无协议包载荷的历史 Workspace；只验证其本机精确引用并插入 Workspace。
    async fn commit_legacy_workspace(&self, workspace: ProxyWorkspace) -> AppResult<()>;
    /// 原子替换协议包注册表、全部 Workspace、当前选择和 Settings。
    async fn replace_application_bundle(
        &self,
        packages: Vec<PortableApplicationProtocolPackage>,
        document: ApplicationConfigurationDocument,
    ) -> AppResult<()>;
    /// 恢复历史完整配置；只验证实际引用，保留整个本机注册表及全部启用状态。
    async fn replace_legacy_application_configuration(
        &self,
        document: ApplicationConfigurationDocument,
    ) -> AppResult<()>;
    /// 原子清除协议包和其他用户数据并写入干净默认配置；成功后清空编译缓存。
    async fn reset_application_bundle(
        &self,
        document: ApplicationConfigurationDocument,
    ) -> AppResult<()>;
}

/// 协议包生命周期与可移植性用例的具名端口集合，供 Host 和测试替身装配。
#[derive(Debug, Clone)]
pub struct ProtocolPackageApplicationServices {
    pub store: std::sync::Arc<dyn ProtocolPackageStorePort>,
    pub compiler: std::sync::Arc<dyn ProtocolPackageCompilerPort>,
    pub importer: std::sync::Arc<dyn ProtocolPackageImportPort>,
    pub builtin: std::sync::Arc<dyn BuiltinProtocolPackagePort>,
    pub usage_query: std::sync::Arc<dyn ProtocolPackageUsageQueryPort>,
    pub portability: std::sync::Arc<dyn ProtocolPackagePortabilityPort>,
}

impl ProtocolPackageApplicationServices {
    #[must_use]
    pub fn unavailable() -> Self {
        let unavailable = std::sync::Arc::new(UnavailableProtocolPackageServices);
        Self {
            store: unavailable.clone(),
            compiler: unavailable.clone(),
            importer: unavailable.clone(),
            builtin: unavailable.clone(),
            usage_query: unavailable,
            portability: std::sync::Arc::new(UnavailableProtocolPackageServices),
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
impl BuiltinProtocolPackagePort for UnavailableProtocolPackageServices {
    async fn restore_builtin(&self) -> AppResult<ProtocolPackageImportViewModel> {
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

#[async_trait]
impl ProtocolPackagePortabilityPort for UnavailableProtocolPackageServices {
    async fn application_backup_baseline(
        &self,
    ) -> AppResult<Vec<ApplicationBackupProtocolPackageBaseline>> {
        unavailable_protocol_packages()
    }

    async fn export_workspace_packages(
        &self,
        _: &[ProtocolPackageRef],
    ) -> AppResult<Vec<PortableProtocolPackage>> {
        unavailable_protocol_packages()
    }

    async fn export_application_packages(
        &self,
    ) -> AppResult<Vec<PortableApplicationProtocolPackage>> {
        unavailable_protocol_packages()
    }

    async fn preflight_workspace_packages(
        &self,
        _: &[PortableProtocolPackage],
    ) -> AppResult<Vec<ProtocolPackageDescriptionViewModel>> {
        unavailable_protocol_packages()
    }

    async fn preflight_application_packages(
        &self,
        _: &[PortableApplicationProtocolPackage],
    ) -> AppResult<Vec<ProtocolPackageDescriptionViewModel>> {
        unavailable_protocol_packages()
    }

    async fn preflight_installed_packages(
        &self,
        _: &[ProtocolPackageRef],
    ) -> AppResult<Vec<ProtocolPackageDescriptionViewModel>> {
        unavailable_protocol_packages()
    }

    async fn commit_workspace_bundle(
        &self,
        _: Vec<PortableProtocolPackage>,
        _: ProxyWorkspace,
    ) -> AppResult<()> {
        unavailable_protocol_packages()
    }

    async fn commit_legacy_workspace(&self, _: ProxyWorkspace) -> AppResult<()> {
        unavailable_protocol_packages()
    }

    async fn replace_application_bundle(
        &self,
        _: Vec<PortableApplicationProtocolPackage>,
        _: ApplicationConfigurationDocument,
    ) -> AppResult<()> {
        unavailable_protocol_packages()
    }

    async fn replace_legacy_application_configuration(
        &self,
        _: ApplicationConfigurationDocument,
    ) -> AppResult<()> {
        unavailable_protocol_packages()
    }

    async fn reset_application_bundle(&self, _: ApplicationConfigurationDocument) -> AppResult<()> {
        unavailable_protocol_packages()
    }
}
