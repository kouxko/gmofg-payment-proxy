//! 协议包查询、启用、停用与删除的应用用例。
//!
//! 引用检查与写操作共享 [`Application::mutation_gate`]。因此前端此前看到的详情只能作为
//! 展示快照，真正执行停用或删除时一定会在同一临界区重新查询，不能利用查询和写入之间
//! 的时间窗口绕过 Rust 约束。

use std::collections::{BTreeMap, HashMap};

use super::Application;
use crate::{
    AppError, AppResult, HttpBodyProcessing, ListenerDataPlane,
    ListenerProtocolPackageCatalogViewModel, ListenerProtocolPackageOptionViewModel,
    OperationResultViewModel, ProtocolPackageDetailViewModel, ProtocolPackageGroupViewModel,
    ProtocolPackageImportPreviewViewModel, ProtocolPackageImportToken,
    ProtocolPackageImportViewModel, ProtocolPackageRef, ProtocolPackageUsageViewModel,
    ProtocolPackageValidationViewModel, ProtocolPackageVersionViewModel, SocketPayloadProcessing,
    UiTone, builtin_iso8583_package_ref,
};

impl Application {
    /// 返回 Listener 编辑器当前可以安全选择的精确协议包版本。
    ///
    /// 这是只读目录：不会切换启用状态，也不会把历史 `Valid` 当作当前兼容性证明。
    /// 单个版本恢复或描述失败时只排除该版本，避免一个损坏包让其他健康版本无法配置；
    /// Listener 保存和启动仍会在 mutation 用例中对最终精确绑定重新完整校验。
    pub async fn listener_protocol_package_catalog(
        &self,
    ) -> AppResult<ListenerProtocolPackageCatalogViewModel> {
        // 目录必须与启停、删除和重装串行，否则 list 的名称/状态可能与随后 fresh
        // 编译得到的 Schema/能力来自不同一代持久化内容。
        let _gate = self.mutation_gate.lock().await;
        let mut installed = self.protocol_package_store.list().await?;
        installed.sort_by(|left, right| {
            left.package
                .id
                .cmp(&right.package.id)
                .then_with(|| left.package.version.semantic_cmp(&right.package.version))
                .then_with(|| {
                    left.package
                        .version
                        .as_str()
                        .cmp(right.package.version.as_str())
                })
        });
        if installed
            .windows(2)
            .any(|pair| pair[0].package == pair[1].package)
        {
            return Err(AppError::new(
                "PROTOCOL_PACKAGE_CATALOG_INVALID",
                "协议包目录包含重复的精确版本，已拒绝展示选择器。",
            ));
        }

        let installed_version_count = installed.len();
        let mut options = Vec::with_capacity(installed_version_count);
        for version in installed {
            if !version.enabled
                || !matches!(
                    version.validation,
                    ProtocolPackageValidationViewModel::Valid
                )
            {
                continue;
            }
            // 使用 portability 的纯读 preflight，从规范持久化文件重新恢复和编译；
            // 不能复用 compiler.describe 的暖 AST 缓存，也不能更新 validation/cache。
            let Ok(descriptions) = self
                .protocol_package_portability
                .preflight_installed_packages(std::slice::from_ref(&version.package))
                .await
            else {
                continue;
            };
            let [description] = descriptions.as_slice() else {
                continue;
            };
            if ensure_description_identity(&version.package, description).is_err() {
                continue;
            }
            options.push(ListenerProtocolPackageOptionViewModel {
                package: version.package,
                name: version.name,
                kind: description.kind,
                capabilities: description.capabilities,
                upstream_schema: description.upstream_schema.clone(),
                downstream_schema: description.downstream_schema.clone(),
            });
        }
        let recommended = builtin_iso8583_package_ref();
        let recommended_package = options
            .iter()
            .any(|option| option.package == recommended)
            .then_some(recommended);
        let unavailable_version_count = installed_version_count
            .checked_sub(options.len())
            .ok_or_else(|| {
                AppError::new(
                    "PROTOCOL_PACKAGE_CATALOG_INVALID",
                    "协议包目录计数不一致，已拒绝展示选择器。",
                )
            })?;
        Ok(ListenerProtocolPackageCatalogViewModel {
            options,
            recommended_package,
            installed_version_count,
            unavailable_version_count,
        })
    }

    /// 按稳定 ID 分组列出所有精确版本，不隐式编译或改变启用状态。
    pub async fn protocol_package_list(&self) -> AppResult<Vec<ProtocolPackageGroupViewModel>> {
        let versions = self.protocol_package_store.list().await?;
        if versions.is_empty() {
            return Ok(Vec::new());
        }
        let mut usage_counts = HashMap::new();
        for count in self.protocol_package_usage.usage_counts().await? {
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
        let description = self.protocol_package_compiler.describe(&package).await?;
        ensure_description_identity(&package, &description)?;
        let usages = self.protocol_package_usage.usages(&package).await?;
        Ok(ProtocolPackageDetailViewModel {
            version,
            kind: description.kind,
            capabilities: description.capabilities,
            upstream_schema: description.upstream_schema,
            downstream_schema: description.downstream_schema,
            usages,
        })
    }

    /// 通过宿主原生文件选择器导入 ZIP；WebView 不提交路径或文件内容。
    pub async fn protocol_package_import(
        &self,
    ) -> AppResult<Option<ProtocolPackageImportPreviewViewModel>> {
        // 原生文件 Dialog 可能长时间等待用户选择，不能在这段交互期间占用全局 mutation_gate。
        // 注册表自身会串行化同一身份的 install/delete/cache 写入；导入又不会改写既有启用位或
        // Listener 引用，因此无需阻塞其他独立的 Application 变更。
        self.protocol_package_importer.prepare_zip().await
    }

    /// 使用 prepare 阶段返回的随机 token 原子提交被冻结的已验证包。
    pub async fn protocol_package_import_commit(
        &self,
        token: ProtocolPackageImportToken,
    ) -> AppResult<ProtocolPackageImportViewModel> {
        let _gate = self.mutation_gate.lock().await;
        self.protocol_package_importer.commit_zip(token).await
    }

    /// 关闭已就绪预览时立即释放 pending 容量；无效、过期或已使用 token 均稳定失败。
    pub async fn protocol_package_import_discard(
        &self,
        token: ProtocolPackageImportToken,
    ) -> AppResult<OperationResultViewModel> {
        self.protocol_package_importer.discard_zip(token).await?;
        Ok(OperationResultViewModel {
            success: true,
            cancelled: false,
            message: "待确认的协议包导入已释放。".into(),
            ui_tone: UiTone::Neutral,
            entity_id: None,
            revision: None,
            requires_restart: false,
        })
    }

    /// 从应用内置的不可信 ZIP 重新恢复官方起始示例。
    pub async fn protocol_package_restore_builtin(
        &self,
    ) -> AppResult<ProtocolPackageImportViewModel> {
        let _gate = self.mutation_gate.lock().await;
        self.protocol_package_builtin.restore_builtin().await
    }

    /// 读取编译期内置的官方起始示例 ZIP，不访问或修改安装库。
    pub async fn protocol_package_builtin_archive(&self) -> AppResult<Vec<u8>> {
        self.protocol_package_builtin.builtin_archive().await
    }

    /// 单独查询精确版本的全部使用者，供详情刷新和删除确认 Dialog 复用。
    pub async fn protocol_package_usage(
        &self,
        package: ProtocolPackageRef,
    ) -> AppResult<Vec<ProtocolPackageUsageViewModel>> {
        self.require_protocol_package(&package).await?;
        self.protocol_package_usage.usages(&package).await
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
            .compile_fresh(&package)
            .await?;
        ensure_compilation_receipt(&package, stored.host_api, &receipt)?;
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
        // validation 列是导入/上次编译的历史快照，不能代替当前 Host API
        // 下的 fresh 恢复与编译。真实的可加载性只由下面的 compiler receipt 决定。
        let receipt = self
            .protocol_package_compiler
            .compile_fresh(package)
            .await
            .map_err(|error| listener_error_field(error, package_field))?;
        ensure_compilation_receipt(package, version.host_api, &receipt)
            .map_err(|error| listener_error_field(error, package_field))?;
        let description = self
            .protocol_package_compiler
            .describe(package)
            .await
            .map_err(|error| listener_error_field(error, package_field))?;
        super::protocol_package_portability::validate_listener_protocol_binding(
            workspace,
            listener_id,
            &description,
        )
        .map_err(|error| listener_error_field(error, processing_field))?;
        Ok(())
    }

    pub(super) async fn require_protocol_package(
        &self,
        package: &ProtocolPackageRef,
    ) -> AppResult<ProtocolPackageVersionViewModel> {
        self.protocol_package_store
            .get(package)
            .await?
            .ok_or_else(|| protocol_package_not_found(package))
    }
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
