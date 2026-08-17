//! 应用启动、事件订阅与退出清理用例。

use chrono::Utc;

use super::Application;
use crate::{
    AppBootstrapViewModel, AppError, AppResult, CaptureQuery, EventSubscription,
    ListenerRuntimeState, OperationResultViewModel, PageRequest, ProxyState, ProxyStatusViewModel,
    UiEventEnvelope, UiEventPayload,
};

impl Application {
    pub async fn app_bootstrap(&self) -> AppResult<AppBootstrapViewModel> {
        let proxy = self.proxy.status().await?;
        let recent_capture = self
            .capture_query(CaptureQuery {
                keyword: None,
                terminal_ip: None,
                channel: None,
                stage: None,
                result: None,
                rule_id: None,
                after_event_id: None,
                sort: crate::CaptureSort::OccurredAt,
                direction: crate::SortDirection::Desc,
                page: PageRequest {
                    page: 1,
                    page_size: 5,
                },
            })
            .await?;
        // 动态入口各自拥有运行 epoch；启动快照必须聚合全部待处理断点，不能再以已退役
        // 的单实例代理 epoch 过滤，否则界面会漏掉真实入口产生的断点。
        let pending_breakpoints = self.breakpoints.query(None).into_iter().collect();
        // 启动快照只读取证书的非敏感元数据。不能为了画状态栏就解密私钥并触发
        // Keychain/DPAPI 授权，否则用户取消系统提示会让整个展示层无法启动。
        let certificate = self.certificates.status().await?;
        let settings = self.settings.get().await?;
        // 规则和故障动作的通道必须引用当前 Workspace 的 Listener UUID。
        // 旧产品设置中的静态通道只服务于兼容状态展示，不能再作为可提交的配置来源；
        // 否则 UI 会生成领域层必然拒绝、且永远无法命中动态 Listener 的规则。
        let channel_catalog = self.selected_workspace_channel_catalog().await?;
        Ok(AppBootstrapViewModel {
            product_name: self.product_name.clone(),
            proxy,
            channel_catalog,
            recent_capture,
            pending_breakpoints,
            certificate,
            settings,
            event_cursor: self.events.current_cursor(),
        })
    }

    pub fn app_subscribe_events(&self, after_event_id: u64) -> AppResult<EventSubscription> {
        self.events.subscribe_default(after_event_id)
    }

    pub fn app_take_subscription_failure(&self, subscription_id: u64) -> Option<UiEventEnvelope> {
        self.events.take_subscription_failure(subscription_id)
    }

    pub fn app_unsubscribe_events(&self, subscription_id: u64) -> OperationResultViewModel {
        self.events.unsubscribe(subscription_id);
        OperationResultViewModel::success("实时事件订阅已取消。")
    }

    /// Stops the runtime on exit after waiting for any in-flight mutation.
    pub async fn app_shutdown(&self) -> AppResult<ProxyStatusViewModel> {
        let _gate = self.mutation_gate.lock().await;
        self.app_shutdown_inner().await
    }

    pub(super) async fn app_shutdown_inner(&self) -> AppResult<ProxyStatusViewModel> {
        let mut listener_cleanup_errors = Vec::new();
        match self.android.runtime_owner().await {
            Ok(Some(owner)) => {
                if let Err(error) = self.android.network_stop().await {
                    listener_cleanup_errors.push(format!(
                        "Android 运行设备 {} 停止失败 [{}] {}",
                        owner.serial, error.view_model.code, error.view_model.message
                    ));
                }
            }
            Ok(None) => {}
            Err(error) => listener_cleanup_errors.push(format!(
                "Android 运行设备读取失败 [{}] {}",
                error.view_model.code, error.view_model.message
            )),
        }
        match self.listener_runtime.statuses().await {
            Ok(statuses) => {
                for status in statuses {
                    if status.state == ListenerRuntimeState::Stopped {
                        continue;
                    }
                    if let Err(error) = self.listener_runtime.stop(status.listener_id).await {
                        listener_cleanup_errors.push(format!(
                            "入口 {} 停止失败 [{}] {}",
                            status.listener_id, error.view_model.code, error.view_model.message
                        ));
                    }
                }
            }
            Err(error) => listener_cleanup_errors.push(format!(
                "入口状态读取失败 [{}] {}",
                error.view_model.code, error.view_model.message
            )),
        }
        let before = self.proxy.status().await?;
        let stop_result = if before.state == ProxyState::Stopped {
            Ok(before.clone())
        } else {
            self.proxy.stop().await
        };
        if let Some(epoch) = before.runtime_epoch {
            for summary in self.breakpoints.proxy_stopped(epoch) {
                self.events.publish(
                    Some(epoch),
                    Utc::now(),
                    Some(summary.breakpoint_id.to_string()),
                    Some(summary.revision),
                    UiEventPayload::BreakpointResolved(summary),
                );
            }
        }
        let clear_result = self.settings.clear_effective().await;
        let legacy_result = match (stop_result, clear_result) {
            (Ok(status), Ok(_)) => {
                self.publish_runtime(&status);
                Ok(status)
            }
            (Err(stop_error), Ok(_)) => Err(stop_error),
            (Ok(status), Err(clear_error)) => {
                self.publish_runtime(&status);
                Err(clear_error)
            }
            (Err(stop_error), Err(clear_error)) => Err(AppError::new(
                "APP_SHUTDOWN_FAILED",
                format!(
                    "Proxy 停止失败 [{}] {}；生效设置清理失败 [{}] {}。",
                    stop_error.view_model.code,
                    stop_error.view_model.message,
                    clear_error.view_model.code,
                    clear_error.view_model.message
                ),
            )),
        };
        if listener_cleanup_errors.is_empty() {
            legacy_result
        } else {
            let listener_detail = listener_cleanup_errors.join("；");
            match legacy_result {
                Ok(_) => Err(AppError::new(
                    "APP_SHUTDOWN_FAILED",
                    format!("退出资源清理失败：{listener_detail}。"),
                )),
                Err(error) => Err(AppError::new(
                    "APP_SHUTDOWN_FAILED",
                    format!(
                        "退出资源清理失败：{listener_detail}；其他退出清理失败 [{}] {}。",
                        error.view_model.code, error.view_model.message
                    ),
                )),
            }
        }
    }
}
