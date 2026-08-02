//! 已退役的单实例代理生命周期兼容端口。
//!
//! 通用版本只允许 [`ListenerRuntimePort`](intercept_proxy_application::ListenerRuntimePort)
//! 管理 Workspace 中的动态代理入口。旧的 `ProxySupervisorPort` 仍被部分历史应用用例和
//! 测试 DTO 引用，因此生产 Host 注入这个永远停止、不能启动网络监听的适配器，防止
//! 第二套 supervisor 与动态入口争抢端口或产生相互矛盾的状态。

use async_trait::async_trait;
use intercept_proxy_application::{
    AppError, AppResult, ConnectionHealthState, ConnectionHealthViewModel, DisabledReason,
    ProxyState, ProxyStatusViewModel, ProxySupervisorPort, SettingsDraft, UiTone,
};

#[derive(Debug)]
pub struct RetiredProxyAdapter {
    settings: SettingsDraft,
}

impl RetiredProxyAdapter {
    #[must_use]
    pub fn new(settings: SettingsDraft) -> Self {
        Self { settings }
    }

    fn status_view(&self) -> ProxyStatusViewModel {
        let unavailable = ConnectionHealthViewModel {
            state: ConnectionHealthState::Unavailable,
            state_text: "已由 Workspace 代理入口取代".to_owned(),
            detail: "请在“代理入口”页面启动或停止具体入口。".to_owned(),
            ui_tone: UiTone::Neutral,
        };
        let retired = DisabledReason {
            code: "LEGACY_PROXY_RETIRED".to_owned(),
            message: "单实例代理已退役，请使用 Workspace 代理入口。".to_owned(),
        };
        ProxyStatusViewModel {
            state: ProxyState::Stopped,
            state_text: "已由 Workspace 代理入口取代".to_owned(),
            ui_tone: UiTone::Neutral,
            runtime_epoch: None,
            revision: 0,
            channels: Vec::new(),
            app_to_proxy_health: unavailable.clone(),
            proxy_to_server_health: unavailable,
            active_sessions: 0,
            pending_breakpoints: 0,
            logical_memory_bytes: 0,
            logical_memory_text: "0 B".to_owned(),
            memory_capacity_bytes: self.settings.max_memory_bytes,
            memory_capacity_text: format!("{} B", self.settings.max_memory_bytes),
            memory_usage_percent: 0,
            session_capacity: self.settings.max_sessions,
            default_timeout_seconds: self.settings.read_timeout_seconds,
            can_start: false,
            start_disabled_reason: Some(retired.clone()),
            can_stop: false,
            stop_disabled_reason: Some(retired.clone()),
            can_restart: false,
            restart_disabled_reason: Some(retired),
            fault_reason: None,
        }
    }
}

#[async_trait]
impl ProxySupervisorPort for RetiredProxyAdapter {
    async fn status(&self) -> AppResult<ProxyStatusViewModel> {
        Ok(self.status_view())
    }

    async fn start(&self, _effective_settings: SettingsDraft) -> AppResult<ProxyStatusViewModel> {
        Err(AppError::new(
            "LEGACY_PROXY_RETIRED",
            "单实例代理已退役，请在“代理入口”页面启动 Workspace 入口。",
        ))
    }

    async fn stop(&self) -> AppResult<ProxyStatusViewModel> {
        Ok(self.status_view())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn retired_adapter_cannot_open_a_second_listener_lifecycle() {
        let adapter = RetiredProxyAdapter::new(SettingsDraft::default());
        let status = adapter.status().await.expect("status");
        assert_eq!(status.state, ProxyState::Stopped);
        assert!(!status.can_start);
        let error = adapter
            .start(SettingsDraft::default())
            .await
            .expect_err("retired lifecycle must fail closed");
        assert_eq!(error.view_model.code, "LEGACY_PROXY_RETIRED");
    }
}
