//! Android 设备网络方案的创建、读取与保存。
//!
//! 这里仅处理可持久化配置；VPN 的启动、应用和停止仍由父模块编排。

use super::{
    AndroidNetworkProfile, AppError, AppResult, Application, apply_package_toggle,
    validate_package_name, validate_profile_id,
};
use crate::{AndroidNetworkProfileSummary, AndroidProfileEditIntent};
use std::collections::BTreeSet;
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
}
