//! Android device-network activation and runtime reconciliation.

use super::{AndroidNetworkProfile, AppError, AppResult, Application, validate_profile_id};
use crate::{
    AndroidNetworkActivation, AndroidNetworkState, AndroidNetworkStatusViewModel,
    AndroidProxyRouteActivation,
};
use std::collections::{BTreeMap, BTreeSet};

impl Application {
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
            "开始建立设备代理数据通道",
            Some(format!(
                "目标应用 {} 个；透明代理路由 {} 条；同网段且入口允许 LAN 时直连桌面，否则业务走 adb reverse。",
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
            "设备代理数据通道已生效",
            Some("目标应用 VPN 与桌面 Listener 的临时运行端点已装载。".into()),
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
            "开始更新设备代理数据通道",
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
                desktop_listener_bind_address: listener.bind_address.clone(),
                desktop_listener_port: listener.port,
                allowed_client_cidrs: listener.allowed_client_cidrs.clone(),
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
}
