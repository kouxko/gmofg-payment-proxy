//! Android 设备网络方案的创建、读取与保存。
//!
//! 这里仅处理可持久化配置；VPN 的启动、应用和停止仍由父模块编排。

use super::{
    AndroidNetworkProfile, AppError, AppResult, Application, apply_package_toggle,
    validate_package_name, validate_profile_id,
};
use crate::{AndroidNetworkProfileSummary, AndroidProfileEditIntent};
use crate::{AndroidNetworkState, OperationResultViewModel};
use std::collections::{BTreeMap, BTreeSet};
use uuid::Uuid;

impl Application {
    pub async fn device_network_profile_list(
        &self,
    ) -> AppResult<Vec<AndroidNetworkProfileSummary>> {
        let workspace = self.selected_workspace().await?;
        Ok(workspace
            .android_network_profiles
            .iter()
            .map(AndroidNetworkProfileSummary::from)
            .collect())
    }

    /// 由 Rust 生成稳定方案 ID 和完整网络默认值；展示层不得自行构造领域对象。
    pub fn device_network_profile_new(&self) -> AndroidNetworkProfile {
        AndroidNetworkProfile {
            id: Uuid::new_v4().to_string(),
            name: "新建设备网络方案".into(),
            target_applications: Vec::new(),
            destination_targets: Vec::new(),
            proxy_routes: Vec::new(),
            confirmed_shared_uids: BTreeSet::default(),
            auto_resume_after_reboot: false,
            weak_network: intercept_proxy_domain::WeakNetworkProfile::default(),
        }
    }

    pub async fn device_network_profile_get(
        &self,
        profile_id: String,
    ) -> AppResult<AndroidNetworkProfile> {
        validate_profile_id(&profile_id)?;
        self.selected_workspace()
            .await?
            .android_network_profiles
            .into_iter()
            .find(|profile| profile.id == profile_id)
            .ok_or_else(|| {
                AppError::new(
                    "ANDROID_PROFILE_NOT_FOUND",
                    "当前 Workspace 中不存在该设备网络方案。",
                )
                .entity(profile_id)
            })
    }

    /// 将页面编辑意图规范化为完整 Profile。
    pub async fn device_network_profile_apply_intent(
        &self,
        mut profile: AndroidNetworkProfile,
        intent: AndroidProfileEditIntent,
    ) -> AppResult<AndroidNetworkProfile> {
        if let AndroidProfileEditIntent::TogglePackage {
            package_name,
            selected,
        } = &intent
        {
            validate_package_name(package_name)?;
            let packages = self.android_package_list().await?;
            apply_package_toggle(&mut profile, &packages, package_name, *selected)?;
        } else {
            intent.apply_defaults(&mut profile);
        }
        Ok(profile)
    }

    pub async fn device_network_profile_save(
        &self,
        profile: AndroidNetworkProfile,
    ) -> AppResult<AndroidNetworkProfile> {
        profile.validate().map_err(AppError::from)?;
        self.validate_profile_against_device(&profile).await?;
        let _gate = self.mutation_gate.lock().await;
        let mut workspace = self.selected_workspace().await?;
        let current = workspace.clone();
        if let Some(stored) = workspace
            .android_network_profiles
            .iter_mut()
            .find(|stored| stored.id == profile.id)
        {
            *stored = profile.clone();
        } else {
            workspace.android_network_profiles.push(profile.clone());
        }
        workspace.validate().map_err(AppError::from)?;
        self.ensure_workspace_update_allowed(&current, &workspace)
            .await?;
        let workspace = self.workspaces.save(workspace).await?;
        self.publish_workspace(&workspace, true, crate::WorkspaceChangeKind::Updated);
        Ok(profile)
    }

    pub async fn device_network_profile_delete(
        &self,
        profile_id: String,
    ) -> AppResult<OperationResultViewModel> {
        validate_profile_id(&profile_id)?;
        // 状态检查与持久化删除必须属于同一个原子边界；否则 start 可在两者之间完成，
        // 导致刚进入运行态的方案仍被删除。
        let _gate = self.mutation_gate.lock().await;
        let status = self.android.network_status().await.map_err(|error| {
            AppError::new(
                "ANDROID_PROFILE_DELETE_STATUS_UNAVAILABLE",
                format!(
                    "删除前无法确认设备网络运行状态：{}",
                    error.view_model.message
                ),
            )
            .retryable("请连接目标设备并刷新运行状态，或先执行紧急恢复网络。")
        })?;
        if matches!(
            status.state,
            AndroidNetworkState::StartRequested
                | AndroidNetworkState::Running
                | AndroidNetworkState::StopRequested
        ) && status.active_profile_id.as_deref() == Some(profile_id.as_str())
        {
            return Err(AppError::new(
                "ANDROID_PROFILE_ACTIVE",
                "设备网络方案仍在运行，不能删除。",
            )
            .retryable("请先停止设备网络接管，再删除方案。")
            .entity(profile_id));
        }
        let mut workspace = self.selected_workspace().await?;
        let current = workspace.clone();
        let before = workspace.android_network_profiles.len();
        workspace
            .android_network_profiles
            .retain(|profile| profile.id != profile_id);
        if workspace.android_network_profiles.len() == before {
            return Err(AppError::new(
                "ANDROID_PROFILE_NOT_FOUND",
                "当前 Workspace 中不存在该设备网络方案。",
            )
            .entity(profile_id));
        }
        self.ensure_workspace_update_allowed(&current, &workspace)
            .await?;
        let workspace = self.workspaces.save(workspace).await?;
        self.publish_workspace(&workspace, true, crate::WorkspaceChangeKind::Updated);
        Ok(OperationResultViewModel {
            success: true,
            cancelled: false,
            message: "设备网络方案已从当前 Workspace 删除。".into(),
            ui_tone: crate::UiTone::Positive,
            entity_id: Some(profile_id),
            revision: Some(workspace.revision.get()),
            requires_restart: false,
        })
    }

    pub(super) async fn selected_workspace(&self) -> AppResult<crate::ProxyWorkspace> {
        let selected = self
            .workspaces
            .list()
            .await?
            .into_iter()
            .find(|summary| summary.selected)
            .ok_or_else(|| AppError::new("WORKSPACE_NOT_SELECTED", "请先选择一个 Workspace。"))?;
        self.workspaces.get(selected.id).await
    }

    pub(super) async fn validate_profile_against_device(
        &self,
        profile: &AndroidNetworkProfile,
    ) -> AppResult<()> {
        // 启动和应用方案不能复用页面浏览时的包清单缓存。应用可能在页面打开后被
        // ADB 安装、升级或卸载；这里必须重新读取包名、UID 与 shared UID 分组。
        let packages = self.refresh_android_package_inventory().await?;
        let inventory = packages
            .iter()
            .map(|package| (package.package_name.as_str(), package))
            .collect::<BTreeMap<_, _>>();
        let selected = profile
            .target_applications
            .iter()
            .map(|target| target.package_name.as_str())
            .collect::<BTreeSet<_>>();

        for target in &profile.target_applications {
            let installed = inventory.get(target.package_name.as_str()).ok_or_else(|| {
                AppError::new(
                    "ANDROID_TARGET_PACKAGE_CHANGED",
                    format!("目标应用 {} 已卸载。", target.package_name),
                )
            })?;
            if installed.uid != target.uid {
                return Err(AppError::new(
                    "ANDROID_TARGET_PACKAGE_CHANGED",
                    format!(
                        "目标应用 {} 的 UID 已变化。请在“目标应用”中取消后重新选择该应用，再保存方案。",
                        target.package_name
                    ),
                )
                .retryable("重新确认目标应用身份后保存方案"));
            }
            if let Some(shared_uid) = installed.shared_uid {
                let complete_group = packages
                    .iter()
                    .filter(|package| package.uid == shared_uid)
                    .map(|package| package.package_name.as_str())
                    .collect::<BTreeSet<_>>();
                if !complete_group.is_subset(&selected) {
                    return Err(AppError::new(
                        "ANDROID_SHARED_UID_PARTIAL_SELECTION",
                        format!("UID {shared_uid} 的共享应用组必须完整选择。"),
                    ));
                }
                if !profile.confirmed_shared_uids.contains(&shared_uid) {
                    return Err(AppError::new(
                        "ANDROID_SHARED_UID_CONFIRMATION_REQUIRED",
                        format!("共享 UID {shared_uid} 需要显式确认。"),
                    ));
                }
            }
        }
        Ok(())
    }
}
