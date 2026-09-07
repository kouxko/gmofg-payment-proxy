use chrono::Utc;

use super::Application;
use crate::{AppError, AppResult, ApplicationSnapshotViewModel, DiagnosticLogQuery};

impl Application {
    /// 在应用写入门内采集一份完整快照。
    ///
    /// Workspace 摘要和详情来自仓储的一次聚合读取；其余依赖各调用一次。配置写用例在
    /// 此方法返回前无法进入 mutation gate，因此不会混合两个 Application 配置代。
    pub async fn application_snapshot(&self) -> AppResult<ApplicationSnapshotViewModel> {
        let _gate = self.mutation_gate.lock().await;
        let settings = self.settings.get().await?;
        let workspace_snapshot = self.workspaces.snapshot().await?;
        let entry_statuses = self.listener_runtime.statuses().await?;
        let protocol_packages = self
            .protocol_package_list_for_snapshot(&workspace_snapshot.details, &entry_statuses)
            .await?;
        let external_package_service = self.external_packages.service_status().await?;
        let diagnostics = self.diagnostic_log_query(&DiagnosticLogQuery::default());

        let generation_bytes = serde_json::to_vec(&(
            &settings,
            &workspace_snapshot.summaries,
            &workspace_snapshot.details,
            &entry_statuses,
            &protocol_packages,
            &external_package_service,
            &diagnostics,
        ))
        .map_err(|error| {
            AppError::new(
                "INTERNAL_ERROR",
                format!("应用快照无法生成观察指纹：{error}"),
            )
        })?;
        let generation = generation_bytes
            .iter()
            .fold(0xcbf2_9ce4_8422_2325_u64, |hash, byte| {
                (hash ^ u64::from(*byte)).wrapping_mul(0x0000_0100_0000_01b3)
            });

        Ok(ApplicationSnapshotViewModel {
            generation: format!("{generation:016x}"),
            observed_at: Utc::now(),
            settings,
            workspaces: workspace_snapshot.summaries,
            workspace_details: workspace_snapshot.details,
            entry_statuses,
            protocol_packages,
            external_package_service,
            diagnostics,
        })
    }
}
