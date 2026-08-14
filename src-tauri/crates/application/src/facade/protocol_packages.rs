//! 协议包查询、启用、停用与删除的应用用例。
//!
//! 引用检查与写操作共享 [`Application::mutation_gate`]。因此前端此前看到的详情只能作为
//! 展示快照，真正执行停用或删除时一定会在同一临界区重新查询，不能利用查询和写入之间
//! 的时间窗口绕过 Rust 约束。

use std::collections::BTreeMap;

use super::Application;
use crate::{
    AppError, AppResult, OperationResultViewModel, ProtocolPackageDetailViewModel,
    ProtocolPackageGroupViewModel, ProtocolPackageRef, ProtocolPackageValidationViewModel,
    ProtocolPackageVersionViewModel, SocketPayloadProcessing, UiTone,
};

impl Application {
    /// 按稳定 ID 分组列出所有精确版本，不隐式编译或改变启用状态。
    pub async fn protocol_package_list(&self) -> AppResult<Vec<ProtocolPackageGroupViewModel>> {
        let mut groups = BTreeMap::new();
        for version in self.protocol_package_store.list().await? {
            groups
                .entry(version.package.id.clone())
                .or_insert_with(Vec::new)
                .push(version);
        }
        Ok(groups
            .into_iter()
            .map(|(id, mut versions)| {
                versions.sort_by(|left, right| {
                    left.package
                        .version
                        .cmp(&right.package.version)
                        .then_with(|| left.name.cmp(&right.name))
                });
                ProtocolPackageGroupViewModel { id, versions }
            })
            .collect())
    }

    /// 查询精确版本和当前全部已保存引用；任何依赖失败都使整个详情查询失败。
    pub async fn protocol_package_detail(
        &self,
        package: ProtocolPackageRef,
    ) -> AppResult<ProtocolPackageDetailViewModel> {
        let version = self.require_protocol_package(&package).await?;
        let usages = self.protocol_package_usage.usages(&package).await?;
        Ok(ProtocolPackageDetailViewModel { version, usages })
    }

    /// 完整重新编译并确认 Host API 后才原子写入启用位。
    pub async fn protocol_package_enable(
        &self,
        package: ProtocolPackageRef,
    ) -> AppResult<ProtocolPackageVersionViewModel> {
        let _gate = self.mutation_gate.lock().await;
        let stored = self.require_protocol_package(&package).await?;
        let receipt = self
            .protocol_package_compiler
            .validate_for_enable(&package)
            .await?;
        if receipt.package != package || receipt.host_api != stored.host_api || !receipt.compatible
        {
            return Err(AppError::new(
                "PROTOCOL_PACKAGE_API_INCOMPATIBLE",
                "协议包无法由当前版本的脚本 Host 安全加载。",
            )
            .entity(package_entity(&package)));
        }
        self.protocol_package_store
            .set_enabled(&package, true)
            .await?;
        Ok(ProtocolPackageVersionViewModel {
            enabled: true,
            ..stored
        })
    }

    /// 仅当精确版本没有任何活动或故障运行态引用时停用。
    ///
    /// 已停止 Listener 的保存引用会原样保留；本用例绝不会自动改写 Workspace 或选择
    /// 另一个协议包版本。
    pub async fn protocol_package_disable(
        &self,
        package: ProtocolPackageRef,
    ) -> AppResult<ProtocolPackageVersionViewModel> {
        let _gate = self.mutation_gate.lock().await;
        let stored = self.require_protocol_package(&package).await?;
        let usages = self.protocol_package_usage.usages(&package).await?;
        if usages
            .iter()
            .any(crate::ProtocolPackageUsageViewModel::blocks_disable)
        {
            return Err(AppError::new(
                "PROTOCOL_PACKAGE_RUNTIME_IN_USE",
                "仍有 Listener 正在使用该协议包版本，请先停止对应入口。",
            )
            .entity(package_entity(&package)));
        }
        self.protocol_package_store
            .set_enabled(&package, false)
            .await?;
        Ok(ProtocolPackageVersionViewModel {
            enabled: false,
            ..stored
        })
    }

    /// 删除没有任何保存引用的精确版本。
    pub async fn protocol_package_delete(
        &self,
        package: ProtocolPackageRef,
    ) -> AppResult<OperationResultViewModel> {
        let _gate = self.mutation_gate.lock().await;
        self.require_protocol_package(&package).await?;
        if !self
            .protocol_package_usage
            .usages(&package)
            .await?
            .is_empty()
        {
            return Err(AppError::new(
                "PROTOCOL_PACKAGE_REFERENCE_IN_USE",
                "仍有已保存 Listener 引用该协议包版本，请先修改或删除这些入口。",
            )
            .entity(package_entity(&package)));
        }
        self.protocol_package_store.delete(&package).await?;
        Ok(OperationResultViewModel {
            success: true,
            cancelled: false,
            message: "协议包版本已删除。".into(),
            ui_tone: UiTone::Positive,
            entity_id: Some(package_entity(&package)),
            revision: None,
            requires_restart: false,
        })
    }

    /// Listener 启动前确认其精确脚本包仍存在、启用且最近一次校验有效。
    pub(super) async fn ensure_listener_protocol_package_available(
        &self,
        listener: &crate::ProxyListener,
    ) -> AppResult<()> {
        let crate::ListenerDataPlane::Socket(socket) = &listener.data_plane else {
            return Ok(());
        };
        let SocketPayloadProcessing::Scripted(scripted) = &socket.processing else {
            return Ok(());
        };
        let version = self.require_protocol_package(&scripted.package).await?;
        if !version.enabled {
            return Err(AppError::new(
                "PROTOCOL_PACKAGE_DISABLED",
                "Listener 引用的协议包版本已停用，请先在协议包页面启用。",
            )
            .entity(package_entity(&scripted.package)));
        }
        if !matches!(
            version.validation,
            ProtocolPackageValidationViewModel::Valid
        ) {
            return Err(AppError::new(
                "PROTOCOL_PACKAGE_INVALID",
                "Listener 引用的协议包版本未通过校验，请重新导入有效版本。",
            )
            .entity(package_entity(&scripted.package)));
        }
        Ok(())
    }

    async fn require_protocol_package(
        &self,
        package: &ProtocolPackageRef,
    ) -> AppResult<ProtocolPackageVersionViewModel> {
        self.protocol_package_store
            .get(package)
            .await?
            .ok_or_else(|| protocol_package_not_found(package))
    }
}

fn protocol_package_not_found(package: &ProtocolPackageRef) -> AppError {
    AppError::new(
        "PROTOCOL_PACKAGE_NOT_FOUND",
        "指定的协议包精确版本尚未安装。",
    )
    .entity(package_entity(package))
}

fn package_entity(package: &ProtocolPackageRef) -> String {
    format!("{}@{}", package.id, package.version)
}
