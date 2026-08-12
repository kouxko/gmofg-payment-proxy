use super::{AndroidNetworkState, AndroidNetworkStatusViewModel, Application};
use crate::{
    AppError, DiagnosticLogEntryViewModel, DiagnosticLogLevel, DiagnosticLogStage, UiEventPayload,
    events::stage_for_error_code,
};
use chrono::Utc;

impl Application {
    pub(super) fn faulted_runtime_status(
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
    pub(super) fn publish_android_vpn_status(&self, status: &AndroidNetworkStatusViewModel) {
        self.events.publish(
            None,
            Utc::now(),
            Some(status.serial.clone()),
            None,
            UiEventPayload::AndroidVpnStatusChanged(status.clone()),
        );
    }

    /// 记录设备网络操作的非敏感步骤。详细命令参数、报文正文、证书和密码均不得进入日志。
    pub(super) fn publish_device_network_step(
        &self,
        level: DiagnosticLogLevel,
        stage: DiagnosticLogStage,
        summary: impl Into<String>,
        detail: Option<String>,
        device_serial: Option<String>,
        profile_id: Option<String>,
    ) {
        self.diagnostic_log_record(DiagnosticLogEntryViewModel {
            level,
            stage,
            summary: summary.into(),
            detail,
            device_serial,
            listener_id: None,
            profile_id,
            socket_context: None,
        });
    }

    /// 将稳定错误码归到用户可理解的链路阶段，避免所有失败都显示成“系统错误”。
    pub(super) fn publish_device_network_error(
        &self,
        error: &AppError,
        profile_id: Option<String>,
    ) {
        let stage = stage_for_error_code(&error.view_model.code);
        self.publish_device_network_step(
            DiagnosticLogLevel::Error,
            stage,
            error.view_model.message.clone(),
            Some(format!("错误码：{}", error.view_model.code)),
            None,
            profile_id,
        );
    }
}
