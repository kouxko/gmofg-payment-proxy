//! 协议包查询、启用、停用与删除的应用用例。
//!
//! 引用检查与写操作共享 [`Application::mutation_gate`]。因此前端此前看到的详情只能作为
//! 展示快照，真正执行停用或删除时一定会在同一临界区重新查询，不能利用查询和写入之间
//! 的时间窗口绕过 Rust 约束。

use std::collections::{BTreeMap, HashMap};

mod catalog;
mod imports;
mod lookup;

use super::Application;
use crate::{
    AppError, AppResult, ExternalPackageServiceStatusViewModel, HttpBodyProcessing,
    ListenerDataPlane, OperationResultViewModel, ProtocolPackageDetailViewModel,
    ProtocolPackageGroupViewModel, ProtocolPackageRef, ProtocolPackageSourceViewModel,
    ProtocolPackageUsageCount, ProtocolPackageUsageViewModel, ProtocolPackageVersionViewModel,
    SocketPayloadProcessing, UiTone,
};

impl Application {
    /// 查询外部软件包服务状态；绑定失败不会影响内置协议包目录。
    pub async fn external_package_service_status(
        &self,
    ) -> AppResult<ExternalPackageServiceStatusViewModel> {
        self.external_packages.service_status().await
    }

    /// 按稳定 ID 分组列出所有精确版本，不隐式编译或改变启用状态。
    pub async fn protocol_package_list(&self) -> AppResult<Vec<ProtocolPackageGroupViewModel>> {
        let versions = self.protocol_package_versions().await?;
        let usage_counts = self.protocol_package_usage.usage_counts().await?;
        Self::group_protocol_package_versions(versions, usage_counts)
    }

    pub(super) async fn protocol_package_list_for_snapshot(
        &self,
        workspaces: &[crate::ProxyWorkspace],
        listener_statuses: &[crate::ListenerStatusViewModel],
    ) -> AppResult<Vec<ProtocolPackageGroupViewModel>> {
        let versions = self.protocol_package_versions().await?;
        let usage_counts = self
            .protocol_package_usage
            .usage_counts_for_snapshot(workspaces, listener_statuses)
            .await?;
        Self::group_protocol_package_versions(versions, usage_counts)
    }

    fn group_protocol_package_versions(
        versions: Vec<ProtocolPackageVersionViewModel>,
        counts: Vec<ProtocolPackageUsageCount>,
    ) -> AppResult<Vec<ProtocolPackageGroupViewModel>> {
        if versions.is_empty() {
            return Ok(Vec::new());
        }
        let mut usage_counts = HashMap::new();
        for count in counts {
            let totals = usage_counts
                .entry(count.package)
                .or_insert((0_usize, 0_usize));
            totals.0 = totals.0.checked_add(count.reference_count).ok_or_else(|| {
                AppError::new(
                    "PROTOCOL_PACKAGE_USAGE_COUNT_INVALID",
                    "协议包引用计数超过应用可表示范围。",
                )
            })?;
            totals.1 = totals
                .1
                .checked_add(count.active_reference_count)
                .ok_or_else(|| {
                    AppError::new(
                        "PROTOCOL_PACKAGE_USAGE_COUNT_INVALID",
                        "协议包活动引用计数超过应用可表示范围。",
                    )
                })?;
        }
        let mut groups = BTreeMap::new();
        for version in versions {
            groups
                .entry(version.package.id.clone())
                .or_insert_with(Vec::new)
                .push(version);
        }
        groups
            .into_iter()
            .map(|(id, mut versions)| -> AppResult<_> {
                versions.sort_by(|left, right| {
                    left.package
                        .version
                        .semantic_cmp(&right.package.version)
                        .then_with(|| left.name.cmp(&right.name))
                });
                let name = versions
                    .last()
                    .map_or_else(|| id.as_str().to_owned(), |version| version.name.clone());
                let kind = versions
                    .first()
                    .map(|version| version.kind)
                    .ok_or_else(|| {
                        AppError::new(
                            "PROTOCOL_PACKAGE_CATALOG_INVALID",
                            "协议包分组不包含任何精确版本。",
                        )
                    })?;
                if versions.iter().any(|version| version.kind != kind) {
                    return Err(AppError::new(
                        "PROTOCOL_PACKAGE_CATALOG_INVALID",
                        "同一协议包 ID 不能同时包含 HTTP 与 Socket 版本。",
                    ));
                }
                let (reference_count, active_reference_count) = versions.iter().try_fold(
                    (0_usize, 0_usize),
                    |totals, version| -> AppResult<_> {
                        let Some(counts) = usage_counts.get(&version.package) else {
                            return Ok(totals);
                        };
                        Ok((
                            totals.0.checked_add(counts.0).ok_or_else(|| {
                                AppError::new(
                                    "PROTOCOL_PACKAGE_USAGE_COUNT_INVALID",
                                    "协议包引用计数超过应用可表示范围。",
                                )
                            })?,
                            totals.1.checked_add(counts.1).ok_or_else(|| {
                                AppError::new(
                                    "PROTOCOL_PACKAGE_USAGE_COUNT_INVALID",
                                    "协议包活动引用计数超过应用可表示范围。",
                                )
                            })?,
                        ))
                    },
                )?;
                Ok(ProtocolPackageGroupViewModel {
                    id,
                    name,
                    kind,
                    versions,
                    reference_count,
                    active_reference_count,
                })
            })
            .collect()
    }

    /// 查询精确版本和当前全部已保存引用；任何依赖失败都使整个详情查询失败。
    pub async fn protocol_package_detail(
        &self,
        package: ProtocolPackageRef,
    ) -> AppResult<ProtocolPackageDetailViewModel> {
        let version = self.require_protocol_package(&package).await?;
        let (description, external) = match version.source {
            ProtocolPackageSourceViewModel::Internal { .. } => (
                self.protocol_package_compiler.describe(&package).await?,
                None,
            ),
            ProtocolPackageSourceViewModel::External { .. } => {
                let description = self.external_packages.describe(&package).await?;
                ensure_external_description(&package, &description)?;
                let external = self.external_packages.detail(&package).await?;
                (description, Some(external))
            }
        };
        ensure_description_identity(&package, &description)?;
        let usages = self.protocol_package_usage.usages(&package).await?;
        Ok(ProtocolPackageDetailViewModel {
            version,
            kind: description.kind,
            capabilities: description.capabilities,
            upstream_schema: description.upstream_schema,
            downstream_schema: description.downstream_schema,
            usages,
            external,
        })
    }

    /// 单独查询精确版本的全部使用者，供详情刷新和删除确认 Dialog 复用。
    pub async fn protocol_package_usage(
        &self,
        package: ProtocolPackageRef,
    ) -> AppResult<Vec<ProtocolPackageUsageViewModel>> {
        self.require_protocol_package(&package).await?;
        self.protocol_package_usage.usages(&package).await
    }

    /// 校验当前执行来源的严格描述后，原子写入启用位。
    ///
    /// 内置来源每次 fresh 编译；外部来源必须在线，并只使用注册边界已严格校验的描述，
    /// 不进入 Rhai 编译器，也不发送业务 JSON-RPC。
    pub async fn protocol_package_enable(
        &self,
        package: ProtocolPackageRef,
    ) -> AppResult<ProtocolPackageVersionViewModel> {
        let _gate = self.mutation_gate.lock().await;
        let stored = self.require_protocol_package(&package).await?;
        match stored.source {
            ProtocolPackageSourceViewModel::Internal { .. } => {
                let receipt = self
                    .protocol_package_compiler
                    .compile_fresh(&package)
                    .await?;
                ensure_compilation_receipt(&package, stored.host_api, &receipt)?;
                self.protocol_package_store
                    .set_enabled(&package, true)
                    .await?;
            }
            ProtocolPackageSourceViewModel::External { online: false } => {
                return Err(AppError::new(
                    "EXTERNAL_PACKAGE_OFFLINE",
                    "外部软件包当前离线，无法启用。",
                )
                .entity(package_entity(&package)));
            }
            ProtocolPackageSourceViewModel::External { online: true } => {
                let description = self.external_packages.describe(&package).await?;
                ensure_description_identity(&package, &description)?;
                ensure_external_description(&package, &description)?;
                self.external_packages.set_enabled(&package, true).await?;
            }
        }
        Ok(ProtocolPackageVersionViewModel {
            enabled: true,
            ..stored
        })
    }

    /// 停用精确版本；外部来源会先停止所有活动引用，并始终保留 WebSocket 连接。
    ///
    /// 内置来源保持既有约束：调用方必须先停止活动入口。两类来源都保留已保存引用，且
    /// 不会选择同 ID 的其他版本。
    pub async fn protocol_package_disable(
        &self,
        package: ProtocolPackageRef,
    ) -> AppResult<ProtocolPackageVersionViewModel> {
        let _gate = self.mutation_gate.lock().await;
        let stored = self.require_protocol_package(&package).await?;
        let usages = self.protocol_package_usage.usages(&package).await?;
        match stored.source {
            ProtocolPackageSourceViewModel::Internal { .. } => {
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
            }
            ProtocolPackageSourceViewModel::External { .. } => {
                for usage in usages.iter().filter(|usage| usage.blocks_disable()) {
                    self.listener_runtime.stop(usage.listener_id).await?;
                }
                self.external_packages.set_enabled(&package, false).await?;
            }
        }
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
        let stored = self.require_protocol_package(&package).await?;
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
        match stored.source {
            ProtocolPackageSourceViewModel::Internal { .. } => {
                self.protocol_package_store.delete(&package).await?;
            }
            ProtocolPackageSourceViewModel::External { online } => {
                if online {
                    self.external_packages.disconnect(&package).await?;
                }
                self.external_packages.delete(&package).await?;
            }
        }
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

    /// 对目标协议处理入口执行 fresh 包恢复、编译描述与绑定规则校验。
    ///
    /// 保存允许引用停用包，以便用户先完成配置再启用；启动则额外要求精确版本当前启用。
    /// HTTP Plain 与 Socket Direct 不引用协议包，绝不访问包 Store、Compiler 或 Usage 端口。
    pub(super) async fn validate_listener_protocol_package(
        &self,
        workspace: &crate::ProxyWorkspace,
        listener_id: crate::ListenerId,
        require_enabled: bool,
    ) -> AppResult<()> {
        let listener = workspace
            .listeners
            .iter()
            .find(|listener| listener.id == listener_id)
            .ok_or_else(|| {
                AppError::new("LISTENER_NOT_FOUND", "未找到指定的 Listener。")
                    .entity(listener_id.to_string())
            })?;
        let (package, package_field, processing_field) = match &listener.data_plane {
            ListenerDataPlane::Http(http) => match &http.body_processing {
                HttpBodyProcessing::Plain => return Ok(()),
                HttpBodyProcessing::Protocol { package } => (
                    package,
                    "listener.data_plane.http.body_processing.package",
                    "listener.data_plane.http.body_processing",
                ),
            },
            ListenerDataPlane::Socket(socket) => match &socket.processing {
                SocketPayloadProcessing::Direct => return Ok(()),
                SocketPayloadProcessing::Scripted(scripted) => (
                    &scripted.package,
                    "listener.data_plane.socket.processing.package",
                    "listener.data_plane.socket.processing",
                ),
            },
        };
        let version = self
            .require_protocol_package(package)
            .await
            .map_err(|error| listener_error_field(error, package_field))?;
        if require_enabled && !version.enabled {
            return Err(listener_error_field(
                AppError::new(
                    "PROTOCOL_PACKAGE_DISABLED",
                    "入口引用的协议包版本已停用，请先在协议包页面启用。",
                )
                .entity(package_entity(package)),
                package_field,
            ));
        }
        let description = match version.source {
            ProtocolPackageSourceViewModel::Internal { .. } => {
                // validation 列是导入/上次编译的历史快照，不能代替当前 Host API
                // 下的 fresh 恢复与编译。真实的可加载性只由 compiler receipt 决定。
                let receipt = self
                    .protocol_package_compiler
                    .compile_fresh(package)
                    .await
                    .map_err(|error| listener_error_field(error, package_field))?;
                ensure_compilation_receipt(package, version.host_api, &receipt)
                    .map_err(|error| listener_error_field(error, package_field))?;
                self.protocol_package_compiler
                    .describe(package)
                    .await
                    .map_err(|error| listener_error_field(error, package_field))?
            }
            ProtocolPackageSourceViewModel::External { online } => {
                if require_enabled && !online {
                    return Err(listener_error_field(
                        AppError::new(
                            "EXTERNAL_PACKAGE_OFFLINE",
                            "入口引用的外部软件包当前离线，不能启动。",
                        )
                        .entity(package_entity(package)),
                        package_field,
                    ));
                }
                let description = self
                    .external_packages
                    .describe(package)
                    .await
                    .map_err(|error| listener_error_field(error, package_field))?;
                ensure_external_description(package, &description)
                    .map_err(|error| listener_error_field(error, package_field))?;
                description
            }
        };
        super::protocol_package_portability::validate_listener_protocol_binding(
            workspace,
            listener_id,
            &description,
        )
        .map_err(|error| listener_error_field(error, processing_field))?;
        Ok(())
    }
}

pub(super) fn ensure_external_description(
    package: &ProtocolPackageRef,
    description: &crate::ProtocolPackageDescriptionViewModel,
) -> AppResult<()> {
    ensure_description_identity(package, description)?;
    let capabilities = description.capabilities;
    if description.kind == crate::ProtocolPackageKindViewModel::Socket
        && capabilities.upstream.frame
        && capabilities.upstream.decode
        && capabilities.upstream.encode
        && capabilities.downstream.frame
        && capabilities.downstream.decode
        && capabilities.downstream.encode
        && capabilities.display
        && matches!(
            description.upstream_schema.root,
            intercept_proxy_domain::DocumentSchemaNode::Object { .. }
        )
        && matches!(
            description.downstream_schema.root,
            intercept_proxy_domain::DocumentSchemaNode::Object { .. }
        )
    {
        return Ok(());
    }
    Err(AppError::new(
        "EXTERNAL_PACKAGE_DESCRIPTION_INVALID",
        "外部软件包缺少第一版 Socket 处理所需的严格描述或完整能力。",
    )
    .entity(package_entity(package)))
}

fn ensure_compilation_receipt(
    package: &ProtocolPackageRef,
    stored_host_api: u32,
    receipt: &crate::ProtocolPackageCompilationReceipt,
) -> AppResult<()> {
    if receipt.package == *package && receipt.host_api == stored_host_api && receipt.compatible {
        return Ok(());
    }
    Err(AppError::new(
        "PROTOCOL_PACKAGE_API_INCOMPATIBLE",
        "协议包无法由当前版本的脚本 Host 安全加载。",
    )
    .entity(package_entity(package)))
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

fn listener_error_field(mut error: AppError, field: &str) -> AppError {
    if error.view_model.field_errors.is_empty() {
        error
            .view_model
            .field_errors
            .insert(field.into(), vec![error.view_model.message.clone()]);
    }
    error
}

pub(super) fn ensure_description_identity(
    requested: &ProtocolPackageRef,
    description: &crate::ProtocolPackageDescriptionViewModel,
) -> AppResult<()> {
    if description.package == *requested {
        return Ok(());
    }
    Err(AppError::new(
        "PROTOCOL_PACKAGE_DESCRIPTION_IDENTITY_MISMATCH",
        "协议包编译描述与请求的精确版本不一致，已拒绝使用该结果。",
    )
    .entity(package_entity(requested)))
}
