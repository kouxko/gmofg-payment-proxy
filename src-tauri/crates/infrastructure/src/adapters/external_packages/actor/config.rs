//! 外部软件包单连接资源与期限配置。

use std::time::Duration;

const DEFAULT_REGISTRATION_TIMEOUT: Duration = Duration::from_secs(30);
const DEFAULT_RPC_TIMEOUT: Duration = Duration::from_secs(5);
const DEFAULT_HEARTBEAT_INTERVAL: Duration = Duration::from_secs(10);
const DEFAULT_HEARTBEAT_TIMEOUT: Duration = Duration::from_secs(30);
const DEFAULT_MAX_IN_FLIGHT: usize = 256;
const DEFAULT_MAX_REGISTRATION_MESSAGE_BYTES: usize = 1024 * 1024;
const DEFAULT_MAX_RPC_MESSAGE_BYTES: usize = 8 * 1024 * 1024;
const DEFAULT_MAX_DISPLAY_MESSAGE_BYTES: usize = 128 * 1024;

/// 单个外部软件包 WebSocket 连接的资源和期限配置。
#[derive(Clone, Debug)]
pub struct ExternalPackageConnectionConfig {
    pub(super) registration_timeout: Duration,
    pub(super) rpc_timeout: Duration,
    pub(super) heartbeat_interval: Duration,
    pub(super) heartbeat_timeout: Duration,
    pub(super) max_in_flight: usize,
    pub(super) max_logical_frame_bytes: usize,
    pub(super) max_registration_message_bytes: usize,
    pub(super) max_rpc_message_bytes: usize,
    pub(super) max_display_message_bytes: usize,
}

impl Default for ExternalPackageConnectionConfig {
    fn default() -> Self {
        Self {
            registration_timeout: DEFAULT_REGISTRATION_TIMEOUT,
            rpc_timeout: DEFAULT_RPC_TIMEOUT,
            heartbeat_interval: DEFAULT_HEARTBEAT_INTERVAL,
            heartbeat_timeout: DEFAULT_HEARTBEAT_TIMEOUT,
            max_in_flight: DEFAULT_MAX_IN_FLIGHT,
            max_logical_frame_bytes: DEFAULT_MAX_RPC_MESSAGE_BYTES,
            max_registration_message_bytes: DEFAULT_MAX_REGISTRATION_MESSAGE_BYTES,
            max_rpc_message_bytes: DEFAULT_MAX_RPC_MESSAGE_BYTES,
            max_display_message_bytes: DEFAULT_MAX_DISPLAY_MESSAGE_BYTES,
        }
    }
}

impl ExternalPackageConnectionConfig {
    /// 使用明确的运行时限制构造配置。
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        registration_timeout: Duration,
        rpc_timeout: Duration,
        heartbeat_interval: Duration,
        heartbeat_timeout: Duration,
        max_in_flight: usize,
        max_logical_frame_bytes: usize,
        max_registration_message_bytes: usize,
        max_rpc_message_bytes: usize,
        max_display_message_bytes: usize,
    ) -> Self {
        assert!(max_in_flight > 0, "max_in_flight must be positive");
        assert!(
            max_logical_frame_bytes > 0,
            "logical frame limit must be positive"
        );
        assert!(
            max_registration_message_bytes > 0,
            "registration limit must be positive"
        );
        assert!(max_rpc_message_bytes > 0, "RPC limit must be positive");
        assert!(
            max_display_message_bytes > 0,
            "display limit must be positive"
        );
        assert!(
            !registration_timeout.is_zero(),
            "registration timeout must be positive"
        );
        assert!(!rpc_timeout.is_zero(), "RPC timeout must be positive");
        assert!(
            !heartbeat_interval.is_zero(),
            "heartbeat interval must be positive"
        );
        assert!(
            heartbeat_timeout >= heartbeat_interval,
            "heartbeat timeout must cover one interval"
        );
        Self {
            registration_timeout,
            rpc_timeout,
            heartbeat_interval,
            heartbeat_timeout,
            max_in_flight,
            max_logical_frame_bytes,
            max_registration_message_bytes,
            max_rpc_message_bytes,
            max_display_message_bytes,
        }
    }

    #[must_use]
    /// 返回单软件包最大在途调用数。
    pub const fn max_in_flight(&self) -> usize {
        self.max_in_flight
    }
    #[must_use]
    /// 返回 Frame Pump 使用的解码前/编码后逻辑报文字节上限。
    pub const fn max_logical_frame_bytes(&self) -> usize {
        self.max_logical_frame_bytes
    }
    #[must_use]
    /// 返回注册响应最大 JSON 字节数。
    pub const fn max_registration_message_bytes(&self) -> usize {
        self.max_registration_message_bytes
    }
    #[must_use]
    /// 返回普通处理响应的逻辑 JSON 字节上限。
    ///
    /// 第一版 WebSocket wire 上限固定为注册上限；该逻辑上限仍用于发送前和已成功组装响应后的校验。
    pub const fn max_rpc_message_bytes(&self) -> usize {
        self.max_rpc_message_bytes
    }
    #[must_use]
    /// 返回 display 响应最大 JSON 字节数。
    pub const fn max_display_message_bytes(&self) -> usize {
        self.max_display_message_bytes
    }

    #[must_use]
    /// 返回单次业务 RPC 的期限；数据面外层期限必须至少覆盖该值。
    pub const fn rpc_timeout(&self) -> Duration {
        self.rpc_timeout
    }

    /// 返回单次数据面写操作的期限，避免长 RPC 期限阻塞读循环和心跳。
    pub(in crate::adapters::external_packages) fn write_timeout(&self) -> Duration {
        self.rpc_timeout.min(self.heartbeat_timeout)
    }

    #[must_use]
    /// 返回整个连接生命周期内 WebSocket 层允许组装的最大单条 wire 消息。
    ///
    /// `tokio-tungstenite 0.30` 未公开保留预读缓冲区的运行时配置修改 API。为同时避免注册后重建
    /// 状态机丢帧，以及未认证连接提前获得更大内存额度，第一版在注册成功后仍保持该上限。
    pub const fn registration_websocket_message_bytes(&self) -> usize {
        self.max_registration_message_bytes
    }
}
