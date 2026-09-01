//! Listener 协议包目录的严格组装。

use super::{Application, ensure_external_description};
use crate::{
    AppError, AppResult, ListenerProtocolPackageCatalogViewModel,
    ListenerProtocolPackageOptionViewModel, ProtocolPackageSourceViewModel,
    ProtocolPackageValidationViewModel, builtin_iso8583_package_ref,
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
        let mut installed = self.protocol_package_versions().await?;
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
            let description = match version.source {
                ProtocolPackageSourceViewModel::Managed { online: true }
                | ProtocolPackageSourceViewModel::External { online: true } => {
                    let Ok(description) = self.external_packages.describe(&version.package).await
                    else {
                        continue;
                    };
                    description
                }
                ProtocolPackageSourceViewModel::Managed { online: false }
                | ProtocolPackageSourceViewModel::External { online: false } => continue,
            };
            let description_valid = ensure_external_description(&version.package, &description);
            if description_valid.is_err() {
                continue;
            }
            options.push(ListenerProtocolPackageOptionViewModel {
                package: version.package,
                name: version.name,
                source: version.source,
                kind: description.kind,
                capabilities: description.capabilities,
                upstream_schema: description.upstream_schema,
                downstream_schema: description.downstream_schema,
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
}
