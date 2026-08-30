//! 外部协议包在线连接及展示快照值。

use std::net::SocketAddr;

use intercept_proxy_application::{
    ExternalPackageRecentErrorViewModel, ExternalPackageServiceStateViewModel,
};
use intercept_proxy_domain::ProtocolPackageRef;
use uuid::Uuid;

use crate::package_transport::PackageTransportClient;
use tokio::sync::watch;

/// 一次在线注册的稳定标识，用于忽略被新连接取代后的迟到断线通知。
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub struct ExternalPackageConnectionId(pub(super) Uuid);

impl ExternalPackageConnectionId {
    /// 返回可用于日志关联的 UUID。
    #[must_use]
    pub const fn as_uuid(self) -> Uuid {
        self.0
    }

    /// 为成功接纳的新 WebSocket 连接创建不可复用代次。
    pub(super) fn new() -> Self {
        Self(Uuid::new_v4())
    }
}

/// 注册成功后交给 Host 监视连接生命周期的结果。
#[derive(Clone, Debug)]
pub struct AcceptedExternalPackageConnection {
    /// 软件包声明的精确身份。
    pub package: ProtocolPackageRef,
    /// 此次连接的唯一标识。
    pub connection_id: ExternalPackageConnectionId,
    /// 首次注册时为 `false`；重连保留之前的用户启用位。
    pub enabled: bool,
}

#[derive(Debug)]
pub(super) enum OnlineConnection {
    Active {
        id: ExternalPackageConnectionId,
        client: PackageTransportClient,
    },
    Closing {
        id: ExternalPackageConnectionId,
        completion: watch::Receiver<bool>,
    },
}

#[derive(Clone, Debug)]
pub(super) struct ConnectionDetailSnapshot {
    pub(super) connection_id: ExternalPackageConnectionId,
    pub(super) remote_address: Option<SocketAddr>,
    pub(super) recent_error: Option<ExternalPackageRecentErrorViewModel>,
}

#[derive(Clone, Debug)]
pub(super) struct ExternalPackageServiceSnapshot {
    pub(super) websocket_url: String,
    pub(super) state: ExternalPackageServiceStateViewModel,
}
