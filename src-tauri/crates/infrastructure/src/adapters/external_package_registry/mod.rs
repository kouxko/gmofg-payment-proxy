//! 外部协议包持久化注册表与 Application 端口适配。
//!
//! `SQLite` 是注册元数据和用户启用位的事实源；本适配器只在内存中保存活动 WebSocket client。
//! 两者通过同一注册表临界区发布，确保重复连接不能覆盖先注册者，数据库失败也不会产生“幽灵在线”状态。

use std::{collections::HashMap, net::SocketAddr, sync::Arc, time::Duration};

use chrono::Utc;
use intercept_proxy_application::{
    AppError, AppResult, EventHub, ExternalPackageRecentErrorViewModel,
    ExternalPackageServiceStateViewModel, ExternalPackageServiceStatusViewModel, UiEventPayload,
};
use intercept_proxy_domain::{ExternalPackageRegistration, ProtocolPackageRef};
use parking_lot::{Mutex, RwLock};
use uuid::Uuid;

use crate::SqliteStore;

use super::{
    common::app_error,
    external_packages::{ExternalPackageClient, ExternalPackageConnectionError},
    listener_runtime::{ExternalSocketPackageBinding, ExternalSocketPackageProvider},
};
use crate::sqlite::external_packages::{
    StoredExternalPackageRegistrationOutcome, canonical_external_registration_fingerprint,
};

mod application_port;
mod diagnostics;
mod views;

use views::recent_error_view;

/// 一次在线注册的稳定标识，用于忽略被新连接取代后的迟到断线通知。
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub struct ExternalPackageConnectionId(Uuid);

impl ExternalPackageConnectionId {
    /// 返回可用于日志关联的 UUID。
    #[must_use]
    pub const fn as_uuid(self) -> Uuid {
        self.0
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
enum OnlineConnection {
    Active {
        id: ExternalPackageConnectionId,
        client: ExternalPackageClient,
    },
    Closing {
        id: ExternalPackageConnectionId,
        client: Option<ExternalPackageClient>,
    },
}

/// 外部协议包的 `SQLite` + 活动连接组合注册表。
#[derive(Debug)]
pub struct ExternalPackageRegistryAdapter {
    store: Arc<SqliteStore>,
    online: Mutex<HashMap<ProtocolPackageRef, OnlineConnection>>,
    service: Mutex<ExternalPackageServiceSnapshot>,
    events: RwLock<Option<Arc<EventHub>>>,
    connection_details: Mutex<HashMap<ProtocolPackageRef, ConnectionDetailSnapshot>>,
}

#[derive(Clone, Debug)]
pub(super) struct ConnectionDetailSnapshot {
    pub(super) connection_id: ExternalPackageConnectionId,
    pub(super) remote_address: Option<SocketAddr>,
    pub(super) rpc_timeout: Duration,
    pub(super) recent_error: Option<ExternalPackageRecentErrorViewModel>,
}

#[derive(Clone, Debug)]
struct ExternalPackageServiceSnapshot {
    websocket_url: String,
    state: ExternalPackageServiceStateViewModel,
}

impl ExternalPackageRegistryAdapter {
    /// 从持久化仓储创建注册表。
    ///
    /// 构造时不会从 `SQLite` 恢复在线状态；只有当前进程完成注册握手的 client 才能进入内存表。
    #[must_use]
    pub fn new(store: Arc<SqliteStore>) -> Self {
        Self {
            store,
            online: Mutex::new(HashMap::new()),
            service: Mutex::new(ExternalPackageServiceSnapshot {
                websocket_url: "ws://0.0.0.0:8765/packages".to_owned(),
                state: ExternalPackageServiceStateViewModel::Failed {
                    error: "外部软件包服务尚未完成启动。".to_owned(),
                },
            }),
            events: RwLock::new(None),
            connection_details: Mutex::new(HashMap::new()),
        }
    }

    /// 注入应用级事件中心，使注册、断线和服务状态变化能刷新所有展示适配器。
    pub fn set_event_hub(&self, events: Arc<EventHub>) {
        *self.events.write() = Some(events);
    }

    /// 记录 Host 已成功绑定的本次进程实际 WebSocket 地址。
    pub fn mark_service_listening(&self, websocket_url: impl Into<String>) {
        let websocket_url = websocket_url.into();
        *self.service.lock() = ExternalPackageServiceSnapshot {
            websocket_url: websocket_url.clone(),
            state: ExternalPackageServiceStateViewModel::Listening,
        };
        self.publish_service_status();
        self.publish_service_listening(&websocket_url);
    }

    /// 记录 Host 非致命的监听失败；内置协议包不受此状态影响。
    pub fn mark_service_failed(&self, websocket_url: impl Into<String>, error: impl Into<String>) {
        let websocket_url = websocket_url.into();
        let error = error.into();
        *self.service.lock() = ExternalPackageServiceSnapshot {
            websocket_url: websocket_url.clone(),
            state: ExternalPackageServiceStateViewModel::Failed {
                error: error.clone(),
            },
        };
        self.publish_service_status();
        self.publish_service_failed(&websocket_url);
    }

    /// 原子接纳完成握手的注册结果，并发布活动 client。
    ///
    /// `fingerprint` 必须由 [`external_package_registration_fingerprint`] 计算；本方法会再次计算并
    /// 比较，避免接线层传错摘要。相同精确身份已有 Active 或 Closing 连接时，后注册者被拒绝。
    pub fn accept_registration(
        &self,
        registration: &ExternalPackageRegistration,
        fingerprint: [u8; 32],
        client: ExternalPackageClient,
    ) -> AppResult<AcceptedExternalPackageConnection> {
        let computed = external_package_registration_fingerprint(registration)?;
        let package = registration.package().identity().clone();
        if computed != fingerprint {
            return Err(package_error(
                "EXTERNAL_PACKAGE_FINGERPRINT_INVALID",
                "外部协议包注册指纹与规范化内容不一致。",
                &package,
            ));
        }

        let mut online = self.online.lock();
        if online.contains_key(&package) {
            return Err(package_error(
                "EXTERNAL_PACKAGE_ALREADY_ONLINE",
                "相同外部协议包精确版本已有在线连接。",
                &package,
            ));
        }
        let enabled = match self
            .store
            .accept_external_package_registration(registration, fingerprint, Utc::now())
            .map_err(app_error)?
        {
            StoredExternalPackageRegistrationOutcome::IdentityConflict => {
                return Err(package_error(
                    "PROTOCOL_PACKAGE_IDENTITY_CONFLICT",
                    "相同协议包 ID 与版本已经注册，但 Schema、方法或元数据不同。",
                    &package,
                ));
            }
            StoredExternalPackageRegistrationOutcome::Inserted => false,
            StoredExternalPackageRegistrationOutcome::Reconnected { enabled } => enabled,
        };
        let connection_id = ExternalPackageConnectionId(Uuid::new_v4());
        let rpc_timeout = client.rpc_timeout();
        // 始终按 online -> connection_details 的顺序持锁。详情先安装、在线代次后发布；
        // 因此观察到 Active 新代次的调用必然也能观察到同一代次的详情快照。
        self.connection_details.lock().insert(
            package.clone(),
            ConnectionDetailSnapshot {
                connection_id,
                remote_address: None,
                rpc_timeout,
                recent_error: None,
            },
        );
        online.insert(
            package.clone(),
            OnlineConnection::Active {
                id: connection_id,
                client,
            },
        );
        drop(online);
        self.publish_catalog_changed(&package);
        self.publish_service_status();
        Ok(AcceptedExternalPackageConnection {
            package,
            connection_id,
            enabled,
        })
    }

    /// 将 TCP accept 得到的远端地址关联到已接纳的连接代次；迟到写入不会覆盖后续连接。
    pub fn record_remote_address(
        &self,
        package: &ProtocolPackageRef,
        connection_id: ExternalPackageConnectionId,
        remote_address: SocketAddr,
    ) -> AppResult<bool> {
        let online = self.online.lock();
        if !matches!(
            online.get(package),
            Some(OnlineConnection::Active { id, .. }) if *id == connection_id
        ) {
            return Ok(false);
        }
        let mut details = self.connection_details.lock();
        let Some(detail) = details
            .get_mut(package)
            .filter(|detail| detail.connection_id == connection_id)
        else {
            return Ok(false);
        };
        if !self
            .store
            .record_external_package_remote_address(package, remote_address)
            .map_err(app_error)?
        {
            return Err(not_found(package));
        }
        detail.remote_address = Some(remote_address);
        detail.recent_error = None;
        drop(details);
        drop(online);
        self.publish_connection_online(package, connection_id, remote_address);
        Ok(true)
    }

    /// 记录连接终止的安全摘要，供详情页和后续 MCP 投影复用。
    pub fn record_connection_error(
        &self,
        package: &ProtocolPackageRef,
        connection_id: ExternalPackageConnectionId,
        reason: &ExternalPackageConnectionError,
    ) -> AppResult<bool> {
        let online = self.online.lock();
        if !matches!(
            online.get(package),
            Some(OnlineConnection::Active { id, .. }) if *id == connection_id
        ) {
            return Ok(false);
        }
        let mut details = self.connection_details.lock();
        let Some(detail) = details
            .get_mut(package)
            .filter(|detail| detail.connection_id == connection_id)
        else {
            return Ok(false);
        };
        let recent_error = recent_error_view(reason);
        if !self
            .store
            .record_external_package_recent_error(
                package,
                &recent_error.code,
                &recent_error.message,
                recent_error.occurred_at,
            )
            .map_err(app_error)?
        {
            return Err(not_found(package));
        }
        detail.recent_error = Some(recent_error);
        drop(details);
        drop(online);
        self.publish_connection_offline(package, connection_id, reason);
        Ok(true)
    }

    /// 返回在线精确版本的 client 快照，供 Socket 运行时创建单业务连接适配器。
    #[must_use]
    pub fn client(&self, package: &ProtocolPackageRef) -> Option<ExternalPackageClient> {
        match self.online.lock().get(package) {
            Some(OnlineConnection::Active { client, .. }) => Some(client.clone()),
            Some(OnlineConnection::Closing { .. }) | None => None,
        }
    }

    /// 处理 actor 的断线通知；只有连接 ID 仍匹配当前所有者时才移除在线状态。
    ///
    /// 该比较使迟到通知无法把同一精确版本的后续连接错误标记为离线。
    pub fn mark_disconnected(
        &self,
        package: &ProtocolPackageRef,
        connection_id: ExternalPackageConnectionId,
    ) -> bool {
        let mut online = self.online.lock();
        // Closing 由 disconnect/delete 操作持有门禁；actor 的 wait_closed 通知不能提前移除它，
        // 否则新注册可能在 SQLite 删除完成前穿过临界区。
        let matches = matches!(
            online.get(package),
            Some(OnlineConnection::Active { id, .. }) if *id == connection_id
        );
        if matches {
            online.remove(package);
        }
        drop(online);
        if matches {
            self.publish_catalog_changed(package);
            self.publish_service_status();
        }
        matches
    }

    /// 核验指定连接仍是最近一次、且尚未被重连取代的离线代次。
    /// 按既定锁顺序取得一致快照，避免旧连接的迟到清理作用于新连接。
    pub(crate) fn is_still_offline_after(
        &self,
        package: &ProtocolPackageRef,
        connection_id: ExternalPackageConnectionId,
    ) -> bool {
        let online = self.online.lock();
        if online.contains_key(package) {
            return false;
        }
        self.connection_details
            .lock()
            .get(package)
            .is_some_and(|detail| detail.connection_id == connection_id)
    }

    async fn disconnect_active(&self, package: &ProtocolPackageRef, keep_gate: bool) {
        let (connection_id, client) = {
            let mut online = self.online.lock();
            match online.get(package) {
                Some(OnlineConnection::Active { id, client }) => {
                    let id = *id;
                    let client = client.clone();
                    online.insert(
                        package.clone(),
                        OnlineConnection::Closing {
                            id,
                            client: Some(client.clone()),
                        },
                    );
                    (Some(id), Some(client))
                }
                Some(OnlineConnection::Closing { id, client }) => (Some(*id), client.clone()),
                None => (None, None),
            }
        };
        if let Some(client) = client {
            client.disconnect().await;
        }
        if !keep_gate && let Some(connection_id) = connection_id {
            let mut online = self.online.lock();
            let owns_closing_gate = matches!(
                online.get(package),
                Some(OnlineConnection::Closing { id, .. }) if *id == connection_id
            );
            if owns_closing_gate {
                online.remove(package);
            }
            drop(online);
            if owns_closing_gate {
                self.publish_catalog_changed(package);
                self.publish_service_status();
            }
        }
    }

    fn is_online(&self, package: &ProtocolPackageRef) -> bool {
        matches!(
            self.online.lock().get(package),
            Some(OnlineConnection::Active { .. })
        )
    }

    fn service_status_snapshot(&self) -> ExternalPackageServiceStatusViewModel {
        let service = self.service.lock().clone();
        let online_connection_count = self
            .online
            .lock()
            .values()
            .filter(|connection| matches!(connection, OnlineConnection::Active { .. }))
            .count();
        ExternalPackageServiceStatusViewModel {
            websocket_url: service.websocket_url,
            fixed_path: "/packages".to_owned(),
            online_connection_count,
            state: service.state,
            authentication_enabled: false,
        }
    }

    fn publish_service_status(&self) {
        let Some(events) = self.events.read().clone() else {
            return;
        };
        events.publish(
            None,
            Utc::now(),
            Some("external-package-service".into()),
            None,
            UiEventPayload::ExternalPackageServiceStatusChanged(self.service_status_snapshot()),
        );
    }

    fn publish_catalog_changed(&self, package: &ProtocolPackageRef) {
        let Some(events) = self.events.read().clone() else {
            return;
        };
        events.publish(
            None,
            Utc::now(),
            Some(format!("{}@{}", package.id, package.version)),
            None,
            UiEventPayload::ProtocolPackageCatalogChanged {
                package: package.clone(),
            },
        );
    }
}

impl ExternalSocketPackageProvider for ExternalPackageRegistryAdapter {
    fn resolve(
        &self,
        package: &ProtocolPackageRef,
    ) -> AppResult<Option<ExternalSocketPackageBinding>> {
        let Some(stored) = self
            .store
            .get_external_package(package)
            .map_err(app_error)?
        else {
            return Ok(None);
        };
        if !stored.enabled {
            return Err(package_error(
                "EXTERNAL_PACKAGE_DISABLED",
                "外部协议包已停用，无法启动绑定入口。",
                package,
            ));
        }
        let client = self.client(package).ok_or_else(|| {
            package_error(
                "EXTERNAL_PACKAGE_OFFLINE",
                "外部协议包当前离线，无法启动绑定入口。",
                package,
            )
        })?;
        let max_frame_bytes = client.max_logical_frame_bytes();
        let rpc_timeout = client.rpc_timeout();
        Ok(Some(ExternalSocketPackageBinding::with_limits(
            stored.registration,
            Arc::new(client),
            max_frame_bytes,
            rpc_timeout,
        )))
    }
}

/// 对规范化注册合同计算稳定 SHA-256 指纹。
pub fn external_package_registration_fingerprint(
    registration: &ExternalPackageRegistration,
) -> AppResult<[u8; 32]> {
    canonical_external_registration_fingerprint(registration).map_err(|error| {
        tracing::error!(error = ?error, "external package registration serialization failed");
        AppError::new("INTERNAL_ERROR", "外部协议包注册内容无法规范化。")
    })
}

fn not_found(package: &ProtocolPackageRef) -> AppError {
    package_error(
        "PROTOCOL_PACKAGE_NOT_FOUND",
        "外部协议包精确版本不存在。",
        package,
    )
}

fn package_error(code: &str, message: &str, package: &ProtocolPackageRef) -> AppError {
    AppError::new(code, message).entity(format!("{}@{}", package.id, package.version))
}

#[cfg(test)]
mod tests;
