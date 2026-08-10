use super::Application;
use crate::{
    AndroidAdbViewModel, AndroidCompanionInstallViewModel, AndroidDeviceViewModel,
    AndroidNetworkActivation, AndroidNetworkProfile, AndroidNetworkState,
    AndroidNetworkStatusViewModel, AndroidPackageViewModel, AndroidProxyRouteActivation,
    AndroidTargetApplication, AppError, AppResult, OperationResultViewModel,
};
use std::collections::{BTreeMap, BTreeSet};
mod packages;
mod profiles;
mod runtime;
#[cfg(test)]
use packages::filter_packages;
use packages::{apply_package_toggle, validate_package_name, validate_profile_id};

impl Application {
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
        self.ensure_android_proxy_listeners_running(&workspace, &activation.profile)
            .await?;
        let profile_id = activation.profile.id.clone();
        self.publish_device_network_step(
            crate::DiagnosticLogLevel::Info,
            crate::DiagnosticLogStage::RouteActivation,
            "开始建立 USB/ADB 代理通道",
            Some(format!(
                "目标应用 {} 个；透明代理路由 {} 条；控制走 adb forward，业务走 adb reverse。",
                activation.profile.target_applications.len(),
                activation.proxy_routes.len()
            )),
            None,
            Some(profile_id.clone()),
        );
        let status = match self.android.network_start(activation).await {
            Ok(status) => status,
            Err(error) => {
                self.publish_device_network_error(&error, Some(profile_id));
                return Err(error);
            }
        };
        self.publish_device_network_step(
            crate::DiagnosticLogLevel::Info,
            crate::DiagnosticLogStage::AdbReverseBusiness,
            "USB/ADB 业务映射已生效",
            Some("设备连接本机临时端口，ADB 将业务连接反向映射到桌面代理入口。".into()),
            Some(status.serial.clone()),
            status.active_profile_id.clone(),
        );
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
        self.ensure_android_proxy_listeners_running(&workspace, &activation.profile)
            .await?;
        let profile_id = activation.profile.id.clone();
        self.publish_device_network_step(
            crate::DiagnosticLogLevel::Info,
            crate::DiagnosticLogStage::RouteActivation,
            "开始更新 USB/ADB 代理通道",
            Some(format!(
                "透明代理路由 {} 条。",
                activation.proxy_routes.len()
            )),
            None,
            Some(profile_id.clone()),
        );
        let status = match self.android.network_apply(activation).await {
            Ok(status) => status,
            Err(error) => {
                self.publish_device_network_error(&error, Some(profile_id));
                return Err(error);
            }
        };
        self.publish_android_vpn_status(&status);
        Ok(status)
    }

    pub async fn device_network_stop(&self) -> AppResult<AndroidNetworkStatusViewModel> {
        let _gate = self.mutation_gate.lock().await;
        let status = match self.android.network_stop().await {
            Ok(status) => status,
            Err(error) => {
                self.publish_device_network_error(&error, None);
                return Err(error);
            }
        };
        let (level, stage, summary) =
            if status.transport == crate::AndroidControlTransport::AdbForceStop {
                (
                    crate::DiagnosticLogLevel::Warning,
                    crate::DiagnosticLogStage::StopFallback,
                    "控制通道不可用，已通过 ADB 强制停止设备端组件",
                )
            } else {
                (
                    crate::DiagnosticLogLevel::Info,
                    crate::DiagnosticLogStage::Cleanup,
                    "设备网络接管已停止并清理 ADB 映射",
                )
            };
        self.publish_device_network_step(
            level,
            stage,
            summary,
            (!status.message.is_empty()).then(|| status.message.clone()),
            Some(status.serial.clone()),
            status.active_profile_id.clone(),
        );
        self.publish_android_vpn_status(&status);
        Ok(status)
    }

    pub async fn device_network_emergency_restore(
        &self,
    ) -> AppResult<AndroidNetworkStatusViewModel> {
        let _gate = self.mutation_gate.lock().await;
        let status = match self.android.emergency_restore().await {
            Ok(status) => status,
            Err(error) => {
                self.publish_device_network_error(&error, None);
                return Err(error);
            }
        };
        self.publish_device_network_step(
            crate::DiagnosticLogLevel::Warning,
            crate::DiagnosticLogStage::StopFallback,
            "已执行紧急恢复并清理 USB/ADB 映射",
            (!status.message.is_empty()).then(|| status.message.clone()),
            Some(status.serial.clone()),
            status.active_profile_id.clone(),
        );
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
            let bind_address = listener
                .bind_address
                .parse::<std::net::IpAddr>()
                .map_err(|_| {
                    AppError::new(
                        "ANDROID_PROXY_LISTENER_BIND_INVALID",
                        format!("代理入口“{}”的绑定地址无效。", listener.name),
                    )
                    .entity(listener.id.to_string())
                })?;
            if !bind_address.is_loopback() && !bind_address.is_unspecified() {
                return Err(AppError::new(
                    "ANDROID_PROXY_LISTENER_BIND_UNREACHABLE",
                    format!(
                        "代理入口“{}”不能用于 USB 透明代理路由；入口必须绑定回环地址或未指定地址。",
                        listener.name
                    ),
                )
                .entity(listener.id.to_string())
                .retryable("请将入口绑定地址改为 127.0.0.1、::1、0.0.0.0 或 ::。"));
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

    async fn ensure_android_proxy_listeners_running(
        &self,
        workspace: &crate::ProxyWorkspace,
        profile: &AndroidNetworkProfile,
    ) -> AppResult<()> {
        let referenced_listener_ids = profile
            .proxy_routes
            .iter()
            .map(|route| route.listener_id)
            .collect::<BTreeSet<_>>();
        if referenced_listener_ids.is_empty() {
            return Ok(());
        }
        let statuses = self
            .listener_runtime
            .statuses()
            .await?
            .into_iter()
            .map(|status| (status.listener_id, status.state))
            .collect::<BTreeMap<_, _>>();

        for listener_id in referenced_listener_ids {
            if statuses.get(&listener_id) == Some(&crate::ListenerRuntimeState::Running) {
                continue;
            }
            let listener_name = workspace
                .listeners
                .iter()
                .find(|listener| listener.id == listener_id)
                .map_or("未知代理入口", |listener| listener.name.as_str());
            return Err(AppError::new(
                "ANDROID_PROXY_LISTENER_NOT_RUNNING",
                format!("代理入口“{listener_name}”当前未运行，无法启动或应用设备网络方案。"),
            )
            .retryable("请先启动对应代理入口，确认状态为“运行中”后重试。")
            .entity(listener_id.to_string()));
        }
        Ok(())
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
}
#[cfg(test)]
#[path = "android_tests.rs"]
mod tests;
