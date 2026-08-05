use super::{AndroidNetworkState, AndroidNetworkStatusViewModel, Application};
use crate::UiEventPayload;
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
}
