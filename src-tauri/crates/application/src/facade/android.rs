use crate::{
    AndroidAdbViewModel, AndroidCompanionInstallViewModel, AndroidDeviceViewModel,
    AndroidNetworkActivation, AndroidNetworkProfile, AndroidNetworkProfileSummary,
    AndroidNetworkState, AndroidNetworkStatusViewModel, AndroidPackageViewModel,
    AndroidProfileEditIntent, AndroidProxyRouteActivation, AndroidTargetApplication, AppError,
    AppResult, OperationResultViewModel, UiEventPayload,
};
use chrono::Utc;
use std::collections::{BTreeMap, BTreeSet};
use uuid::Uuid;

use super::Application;

impl Application {
    pub async fn android_adb_get(&self) -> AppResult<AndroidAdbViewModel> {
        self.android.adb_get().await
    }

    pub async fn android_adb_select(&self, serial: String) -> AppResult<AndroidAdbViewModel> {
        validate_serial(&serial)?;
        let _gate = self.mutation_gate.lock().await;
        let selected = self.android.adb_select(serial).await?;
        // 即便用户重新选择同一设备，也视为显式刷新包清单。这样安装、卸载或升级
        // 应用后不需要重启桌面端，同时包名筛选仍可复用包清单结果。
        *self.android_package_cache.lock().await = None;
        Ok(selected)
    }

    pub async fn android_device_list(&self) -> AppResult<Vec<AndroidDeviceViewModel>> {
        self.android.device_list().await
    }

    pub async fn android_package_list(&self) -> AppResult<Vec<AndroidPackageViewModel>> {
        let mut cache = self.android_package_cache.lock().await;
        if let Some(packages) = cache.as_ref() {
            return Ok(packages.clone());
        }
        let mut packages = self.android.package_list().await?;
        packages.retain(|package| package.package_name != crate::ANDROID_COMPANION_PACKAGE);
        *cache = Some(packages.clone());
        Ok(packages)
    }

    /// 丢弃当前设备的包清单缓存并重新读取设备。
    ///
    /// APK 安装、卸载或升级不会主动通知桌面进程，因此所有宿主（桌面 UI、未来
    /// CLI/TUI 和无界面测试）都通过该用例获得一致的显式刷新语义。
    pub async fn android_package_refresh(&self) -> AppResult<Vec<AndroidPackageViewModel>> {
        self.refresh_android_package_inventory().await
    }

    /// 包名筛选由 Rust 完成，前端只提交用户输入并渲染返回结果。
    /// 空关键字等价于完整列表；比较时忽略 ASCII 大小写。
    pub async fn android_package_query(
        &self,
        query: String,
    ) -> AppResult<Vec<AndroidPackageViewModel>> {
        let packages = self.android_package_list().await?;
        filter_packages(packages, &query)
    }

    pub async fn android_package_get(
        &self,
        package_name: String,
    ) -> AppResult<AndroidPackageViewModel> {
        validate_package_name(&package_name)?;
        self.android.package_get(package_name).await
    }

    pub async fn android_companion_install(&self) -> AppResult<AndroidCompanionInstallViewModel> {
        self.android.companion_install(false).await
    }

    pub async fn android_companion_update(&self) -> AppResult<AndroidCompanionInstallViewModel> {
        self.android.companion_install(true).await
    }

    pub async fn android_vpn_open_consent(&self) -> AppResult<AndroidNetworkStatusViewModel> {
        let status = self.android.vpn_open_consent().await?;
        self.publish_android_vpn_status(&status);
        Ok(status)
    }

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
    ///
    /// 选择共享 UID 应用时，Rust 自动选择整个 UID 组，并把这次明确点击记录为整组确认；
    /// 取消时整组移除。前端不接触 UID 分组或嵌套默认值。
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

    pub async fn device_network_start(
        &self,
        profile_id: String,
        dangerous_confirmed: bool,
    ) -> AppResult<AndroidNetworkStatusViewModel> {
        // 设备网络运行态、Workspace、监听和方案持久化共享同一组引用关系。
        // 启动期间必须阻止方案删除、Workspace 切换及监听变更，避免校验完成后引用被并发修改。
        let _gate = self.mutation_gate.lock().await;
        let profile = self
            .validate_network_activation(&profile_id, dangerous_confirmed)
            .await?;
        let workspace = self.selected_workspace().await?;
        let activation = Self::android_activation(&workspace, profile)?;
        let status = self.android.network_start(activation).await?;
        self.publish_android_vpn_status(&status);
        Ok(status)
    }

    pub async fn device_network_apply(
        &self,
        profile_id: String,
        dangerous_confirmed: bool,
    ) -> AppResult<AndroidNetworkStatusViewModel> {
        let _gate = self.mutation_gate.lock().await;
        let profile = self
            .validate_network_activation(&profile_id, dangerous_confirmed)
            .await?;
        let workspace = self.selected_workspace().await?;
        let activation = Self::android_activation(&workspace, profile)?;
        let status = self.android.network_apply(activation).await?;
        self.publish_android_vpn_status(&status);
        Ok(status)
    }

    pub async fn device_network_stop(&self) -> AppResult<AndroidNetworkStatusViewModel> {
        let _gate = self.mutation_gate.lock().await;
        let status = self.android.network_stop().await?;
        self.publish_android_vpn_status(&status);
        Ok(status)
    }

    pub async fn device_network_emergency_restore(
        &self,
    ) -> AppResult<AndroidNetworkStatusViewModel> {
        let _gate = self.mutation_gate.lock().await;
        let status = self.android.emergency_restore().await?;
        self.publish_android_vpn_status(&status);
        Ok(status)
    }

    pub async fn device_network_status(&self) -> AppResult<AndroidNetworkStatusViewModel> {
        let status = self.android.network_status().await?;
        if status.state != AndroidNetworkState::Running {
            return Ok(status);
        }
        let Some(profile_id) = status.active_profile_id.as_deref() else {
            return Ok(Self::faulted_runtime_status(
                status,
                "设备报告 VPN 正在运行，但未报告活动方案。请停止后重新启动设备网络接管。",
            ));
        };

        // 切换 Workspace 只改变后续编辑上下文，不会停止设备上已经运行的 VPN。
        // 因此状态恢复必须按 active_profile_id 找到其所属 Workspace，不能误用当前
        // 选中的 Workspace，否则会把仍在运行的方案误报为不存在，或引用错误的入口。
        let Ok((workspace, profile)) = self.running_profile_with_workspace(profile_id).await else {
            return Ok(Self::faulted_runtime_status(
                status,
                "设备正在运行的方案不属于任何现有 Workspace。请显式停止设备网络接管后重新配置。",
            ));
        };
        let activation = Self::android_activation(&workspace, profile)?;
        match self
            .android
            .network_runtime_ready(&activation, &status)
            .await
        {
            Ok(true) => Ok(status),
            Ok(false) => Ok(Self::faulted_runtime_status(
                status,
                "VPN 进程仍在运行，但代理路由运行状态与当前方案不一致。请点击“应用修改”显式恢复。",
            )),
            Err(error) => Ok(Self::faulted_runtime_status(
                status,
                format!(
                    "无法核对 VPN 代理路由运行状态：{}。请点击“应用修改”显式恢复。",
                    error.view_model.message
                ),
            )),
        }
    }

    fn faulted_runtime_status(
        mut status: AndroidNetworkStatusViewModel,
        message: impl Into<String>,
    ) -> AndroidNetworkStatusViewModel {
        status.state = AndroidNetworkState::Faulted;
        status.message = message.into();
        status.with_rust_state_text()
    }

    /// 把由桌面端触发的 VPN 状态变更推入统一有序事件流。
    ///
    /// 设备也可能通过通知栏或系统设置改变 VPN，因此页面仍会定时向 Rust 读取状态；
    /// 查询本身不发布事件，避免“查询 -> 事件 -> 再查询”的反馈循环。
    fn publish_android_vpn_status(&self, status: &AndroidNetworkStatusViewModel) {
        self.events.publish(
            None,
            Utc::now(),
            Some(status.serial.clone()),
            None,
            UiEventPayload::AndroidVpnStatusChanged(status.clone()),
        );
    }

    async fn validate_network_activation(
        &self,
        profile_id: &str,
        confirmed: bool,
    ) -> AppResult<AndroidNetworkProfile> {
        validate_profile_id(profile_id)?;
        let profile = self
            .device_network_profile_get(profile_id.to_owned())
            .await?;
        profile.validate().map_err(AppError::from)?;
        self.validate_profile_against_device(&profile).await?;
        if profile.requires_dangerous_confirmation() && !confirmed {
            return Err(AppError::new(
                "ANDROID_DANGEROUS_CONFIRMATION_REQUIRED",
                "100% 丢包或黑洞窗口需要显式二次确认。",
            ));
        }
        Ok(profile)
    }

    fn android_activation(
        workspace: &crate::ProxyWorkspace,
        profile: AndroidNetworkProfile,
    ) -> AppResult<AndroidNetworkActivation> {
        let mut proxy_routes = Vec::with_capacity(profile.proxy_routes.len());
        for route in &profile.proxy_routes {
            let listener = workspace
                .listeners
                .iter()
                .find(|listener| listener.id == route.listener_id)
                .ok_or_else(|| {
                    AppError::new(
                        "ANDROID_PROXY_LISTENER_NOT_FOUND",
                        "设备网络方案引用的代理入口已不存在。",
                    )
                    .entity(route.listener_id.to_string())
                })?;
            if !listener.enabled {
                return Err(AppError::new(
                    "ANDROID_PROXY_LISTENER_DISABLED",
                    format!("代理入口“{}”尚未启用。", listener.name),
                )
                .entity(listener.id.to_string()));
            }
            proxy_routes.push(AndroidProxyRouteActivation {
                listener_id: listener.id.to_string(),
                original_destination: route.destination.clone(),
                original_ports: route.ports.clone(),
                desktop_listener_port: listener.port,
            });
        }
        Ok(AndroidNetworkActivation {
            profile,
            proxy_routes,
        })
    }

    /// 按运行时返回的方案 ID 解析其所属 Workspace。
    ///
    /// Workspace 复制和导入会重新生成方案 ID，因此正常数据中只能命中一次。这里仍然
    /// 显式拒绝重复 ID，避免损坏数据时静默选择错误的代理入口。
    async fn running_profile_with_workspace(
        &self,
        profile_id: &str,
    ) -> AppResult<(crate::ProxyWorkspace, AndroidNetworkProfile)> {
        validate_profile_id(profile_id)?;
        let mut found = None;
        for summary in self.workspaces.list().await? {
            let workspace = self.workspaces.get(summary.id).await?;
            let Some(profile) = workspace
                .android_network_profiles
                .iter()
                .find(|profile| profile.id == profile_id)
                .cloned()
            else {
                continue;
            };
            if found.is_some() {
                return Err(AppError::new(
                    "ANDROID_ACTIVE_PROFILE_AMBIGUOUS",
                    "运行中的设备网络方案 ID 在多个 Workspace 中重复，无法确定其代理入口。",
                )
                .entity(profile_id));
            }
            found = Some((workspace, profile));
        }
        found.ok_or_else(|| {
            AppError::new(
                "ANDROID_ACTIVE_PROFILE_NOT_FOUND",
                "运行中的设备网络方案所属 Workspace 已不存在；请停止设备网络接管后重新启动。",
            )
            .entity(profile_id)
        })
    }

    async fn selected_workspace(&self) -> AppResult<crate::ProxyWorkspace> {
        let selected = self
            .workspaces
            .list()
            .await?
            .into_iter()
            .find(|summary| summary.selected)
            .ok_or_else(|| AppError::new("WORKSPACE_NOT_SELECTED", "请先选择一个 Workspace。"))?;
        self.workspaces.get(selected.id).await
    }

    async fn validate_profile_against_device(
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

    /// 强制从当前设备读取包身份，并用最新结果替换页面查询缓存。
    async fn refresh_android_package_inventory(&self) -> AppResult<Vec<AndroidPackageViewModel>> {
        let mut packages = self.android.package_list().await?;
        packages.retain(|package| package.package_name != crate::ANDROID_COMPANION_PACKAGE);
        *self.android_package_cache.lock().await = Some(packages.clone());
        Ok(packages)
    }
}

fn filter_packages(
    packages: Vec<AndroidPackageViewModel>,
    query: &str,
) -> AppResult<Vec<AndroidPackageViewModel>> {
    if query.chars().count() > 255 {
        return Err(AppError::new(
            "ANDROID_PACKAGE_QUERY_TOO_LONG",
            "包名筛选关键字不能超过 255 个字符。",
        ));
    }
    let query = query.trim().to_ascii_lowercase();
    if query.is_empty() {
        return Ok(packages);
    }
    Ok(packages
        .into_iter()
        .filter(|package| package.package_name.to_ascii_lowercase().contains(&query))
        .collect())
}

fn apply_package_toggle(
    profile: &mut AndroidNetworkProfile,
    packages: &[AndroidPackageViewModel],
    package_name: &str,
    selected: bool,
) -> AppResult<()> {
    let selected_package = packages
        .iter()
        .find(|package| package.package_name == package_name)
        .ok_or_else(|| {
            AppError::new(
                "ANDROID_TARGET_PACKAGE_CHANGED",
                format!("目标应用 {package_name} 已卸载。"),
            )
        })?;
    if selected_package.package_name == crate::ANDROID_COMPANION_PACKAGE {
        return Err(AppError::new(
            "ANDROID_COMPANION_CANNOT_BE_TARGETED",
            "设备端组件自身不能进入网络接管允许列表。",
        ));
    }

    let group = packages
        .iter()
        .filter(|candidate| match selected_package.shared_uid {
            Some(shared_uid) => candidate.uid == shared_uid,
            None => candidate.package_name == selected_package.package_name,
        })
        .collect::<Vec<_>>();
    let group_names = group
        .iter()
        .map(|package| package.package_name.as_str())
        .collect::<BTreeSet<_>>();
    profile
        .target_applications
        .retain(|target| !group_names.contains(target.package_name.as_str()));

    if let Some(shared_uid) = selected_package.shared_uid {
        profile.confirmed_shared_uids.remove(&shared_uid);
        if selected {
            profile.confirmed_shared_uids.insert(shared_uid);
        }
    }
    if selected {
        let targets = group
            .into_iter()
            .map(target_from_installed_package)
            .collect::<Vec<_>>();
        profile.target_applications.extend(targets);
    }
    profile
        .target_applications
        .sort_by(|left, right| left.package_name.cmp(&right.package_name));
    Ok(())
}

fn target_from_installed_package(package: &AndroidPackageViewModel) -> AndroidTargetApplication {
    AndroidTargetApplication {
        package_name: package.package_name.clone(),
        uid: package.uid,
        display_name: Some(package.package_name.clone()),
    }
}

fn validate_serial(serial: &str) -> AppResult<()> {
    if serial.is_empty()
        || serial.len() > 128
        || !serial
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b':' | b'_' | b'-'))
    {
        return Err(AppError::new(
            "ANDROID_SERIAL_INVALID",
            "安卓设备序列号格式无效。",
        ));
    }
    Ok(())
}

fn validate_profile_id(value: &str) -> AppResult<()> {
    if value.is_empty()
        || value.len() > 128
        || !value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'_' | b'-'))
    {
        return Err(AppError::new(
            "ANDROID_PROFILE_ID_INVALID",
            "设备网络方案 ID 格式无效。",
        ));
    }
    Ok(())
}

fn validate_package_name(value: &str) -> AppResult<()> {
    if value.len() > 255
        || !value.contains('.')
        || !value.split('.').all(|part| {
            !part.is_empty()
                && part
                    .bytes()
                    .all(|byte| byte.is_ascii_alphanumeric() || byte == b'_')
        })
    {
        return Err(AppError::new(
            "ANDROID_PACKAGE_NAME_INVALID",
            "Android 包名格式无效。",
        ));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn package(name: &str) -> AndroidPackageViewModel {
        AndroidPackageViewModel {
            package_name: name.into(),
            uid: 10_001,
            shared_uid: None,
        }
    }

    #[test]
    fn package_query_filters_by_name_case_insensitively() {
        let result = filter_packages(
            vec![
                package("com.example.Payment"),
                package("com.example.launcher"),
            ],
            " payment ",
        )
        .expect("包名筛选应成功");

        assert_eq!(result, vec![package("com.example.Payment")]);
    }

    #[test]
    fn package_query_rejects_unbounded_input() {
        let error = filter_packages(vec![package("com.example.payment")], &"a".repeat(256))
            .expect_err("过长关键字必须由 Rust 拒绝");

        assert_eq!(error.view_model.code, "ANDROID_PACKAGE_QUERY_TOO_LONG");
    }

    #[test]
    fn package_toggle_expands_and_confirms_shared_uid_in_rust() {
        let mut profile = AndroidNetworkProfile {
            id: "shared".into(),
            name: "Shared".into(),
            target_applications: Vec::new(),
            destination_targets: Vec::new(),
            proxy_routes: Vec::new(),
            confirmed_shared_uids: BTreeSet::new(),
            auto_resume_after_reboot: false,
            weak_network: intercept_proxy_domain::WeakNetworkProfile::default(),
        };
        let packages = vec![
            AndroidPackageViewModel {
                package_name: "com.example.one".into(),
                uid: 10_042,
                shared_uid: Some(10_042),
            },
            AndroidPackageViewModel {
                package_name: "com.example.two".into(),
                uid: 10_042,
                shared_uid: Some(10_042),
            },
        ];

        apply_package_toggle(&mut profile, &packages, "com.example.one", true)
            .expect("共享 UID 应整组扩选");

        assert_eq!(profile.target_applications.len(), 2);
        assert!(profile.confirmed_shared_uids.contains(&10_042));
        apply_package_toggle(&mut profile, &packages, "com.example.two", false)
            .expect("取消任一成员应移除整组");
        assert!(profile.target_applications.is_empty());
        assert!(profile.confirmed_shared_uids.is_empty());
    }
}
