//! 外部协议包持久化注册表与 Application 端口适配。
//!
//! `SQLite` 是注册元数据和用户启用位的事实源；本适配器只在内存中保存活动 WebSocket client。
//! 两者通过同一注册表临界区发布，确保重复连接不能覆盖先注册者，数据库失败也不会产生“幽灵在线”状态。

use std::{collections::HashMap, net::SocketAddr, sync::Arc};

use chrono::Utc;
use intercept_proxy_application::{
    AppResult, EventHub, ExternalPackageServiceStateViewModel,
    ExternalPackageServiceStatusViewModel, UiEventPayload,
};
use intercept_proxy_domain::ProtocolPackageRef;
use intercept_proxy_package_contract::PackageManifest;
use parking_lot::{Mutex, RwLock};

#[cfg(test)]
use crate::SqliteStore;
use crate::{IntoSqlitePersistence, SqliteExecutor};

use super::{
    InProcessWasmRuntime, PackageTransportClient, PackageTransportError,
    common::app_error,
    listener_runtime::{ExternalSocketPackageBinding, ExternalSocketPackageProvider},
};
use crate::sqlite::external_packages::StoredExternalPackageRegistrationOutcome;

mod application_port;
mod cleanup;
mod connection;
mod diagnostics;
mod identity;
mod local_archives;
mod service;
mod views;

#[cfg(test)]
use cleanup::DisconnectBarrier;
pub use connection::{AcceptedExternalPackageConnection, ExternalPackageConnectionId};
use connection::{ConnectionDetailSnapshot, ExternalPackageServiceSnapshot, OnlineConnection};
pub use identity::external_package_registration_fingerprint;
use identity::{not_found, package_error};
pub(crate) use views::application_description;
use views::recent_error_view;

/// 外部协议包的 `SQLite` + 活动连接组合注册表。
#[derive(Clone, Debug)]
pub struct ExternalPackageRegistryAdapter {
    environment_apply_resource_gates: Arc<super::EnvironmentApplyResourceGateRegistry>,
    #[cfg(test)]
    store: Arc<SqliteStore>,
    executor: SqliteExecutor,
    connection_mutations: Arc<Mutex<HashMap<ProtocolPackageRef, Arc<tokio::sync::Mutex<()>>>>>,
    application_bundle_mutation: Arc<tokio::sync::Mutex<()>>,
    online: Arc<Mutex<HashMap<ProtocolPackageRef, OnlineConnection>>>,
    service: Arc<Mutex<ExternalPackageServiceSnapshot>>,
    events: Arc<RwLock<Option<Arc<EventHub>>>>,
    connection_details: Arc<Mutex<HashMap<ProtocolPackageRef, ConnectionDetailSnapshot>>>,
    online_changed: Arc<tokio::sync::Notify>,
    local_runtimes: Arc<Mutex<HashMap<ProtocolPackageRef, Arc<InProcessWasmRuntime>>>>,
    #[cfg(test)]
    disconnect_barriers: Arc<Mutex<HashMap<ProtocolPackageRef, DisconnectBarrier>>>,
    #[cfg(test)]
    cleanup_complete: Arc<tokio::sync::Notify>,
    #[cfg(test)]
    deletion_complete: Arc<tokio::sync::Notify>,
}

impl ExternalPackageRegistryAdapter {
    pub(crate) async fn environment_apply_projection(
        &self,
        package: &ProtocolPackageRef,
    ) -> AppResult<Option<intercept_proxy_application::ProtocolPackageVersionViewModel>> {
        intercept_proxy_application::ExternalPackageApplicationPort::get(self, package).await
    }

    pub(crate) async fn environment_apply_projections(
        &self,
    ) -> AppResult<Vec<intercept_proxy_application::ProtocolPackageVersionViewModel>> {
        intercept_proxy_application::ExternalPackageApplicationPort::list(self).await
    }

    /// 从持久化仓储创建注册表。
    ///
    /// 构造时不会从 `SQLite` 恢复在线状态；只有当前进程完成注册握手的 client 才能进入内存表。
    #[must_use]
    pub fn new(persistence: impl IntoSqlitePersistence) -> Self {
        let (executor, store) = persistence.into_sqlite_persistence();
        #[cfg(not(test))]
        drop(store);
        Self {
            environment_apply_resource_gates: Arc::new(
                super::EnvironmentApplyResourceGateRegistry::default(),
            ),
            #[cfg(test)]
            store,
            executor,
            connection_mutations: Arc::new(Mutex::new(HashMap::new())),
            application_bundle_mutation: Arc::new(tokio::sync::Mutex::new(())),
            online: Arc::new(Mutex::new(HashMap::new())),
            service: Arc::new(Mutex::new(ExternalPackageServiceSnapshot {
                websocket_url: "ws://0.0.0.0:8765/packages".to_owned(),
                state: ExternalPackageServiceStateViewModel::Failed {
                    error: "外部软件包服务尚未完成启动。".to_owned(),
                },
            })),
            events: Arc::new(RwLock::new(None)),
            connection_details: Arc::new(Mutex::new(HashMap::new())),
            online_changed: Arc::new(tokio::sync::Notify::new()),
            local_runtimes: Arc::new(Mutex::new(HashMap::new())),
            #[cfg(test)]
            disconnect_barriers: Arc::new(Mutex::new(HashMap::new())),
            #[cfg(test)]
            cleanup_complete: Arc::new(tokio::sync::Notify::new()),
            #[cfg(test)]
            deletion_complete: Arc::new(tokio::sync::Notify::new()),
        }
    }

    pub(crate) fn with_environment_apply_resource_gates(
        mut self,
        gates: Arc<super::EnvironmentApplyResourceGateRegistry>,
    ) -> Self {
        self.environment_apply_resource_gates = gates;
        self
    }

    pub(super) async fn acquire_environment_apply_package_gate(
        &self,
        package: &ProtocolPackageRef,
    ) -> tokio::sync::OwnedMutexGuard<()> {
        self.environment_apply_resource_gates
            .acquire(
                super::environment_configuration_lease::EnvironmentApplyLeaseResourceKey::ExactPackage(
                    package.clone(),
                ),
            )
            .await
    }

    /// 注入应用级事件中心，使注册、断线和服务状态变化能刷新所有展示适配器。
    pub fn set_event_hub(&self, events: Arc<EventHub>) {
        *self.events.write() = Some(events);
    }

    /// 原子接纳完成握手的注册结果，并发布活动 client。
    ///
    /// `fingerprint` 必须由 [`external_package_registration_fingerprint`] 计算；本方法会再次计算并
    /// 比较，避免接线层传错摘要。相同精确身份已有 Active 或 Closing 连接时，后注册者被拒绝。
    pub async fn accept_registration(
        &self,
        registration: &PackageManifest,
        fingerprint: [u8; 32],
        client: PackageTransportClient,
    ) -> AppResult<AcceptedExternalPackageConnection> {
        let _bundle_mutation = self.application_bundle_mutation.lock().await;
        let computed = external_package_registration_fingerprint(registration)?;
        let package = registration.package().identity().clone();
        let _environment_apply_gate = self.acquire_environment_apply_package_gate(&package).await;
        if computed != fingerprint {
            return Err(package_error(
                "EXTERNAL_PACKAGE_FINGERPRINT_INVALID",
                "外部协议包注册指纹与规范化内容不一致。",
                &package,
            ));
        }

        let gate = self.connection_mutation(&package);
        let mutation = gate.lock().await;
        let selected = package.clone();
        let local_archive_exists = self
            .executor
            .execute(move |store| store.get_external_package(&selected))
            .await
            .map_err(app_error)?
            .is_some_and(|stored| stored.local_archive.is_some());
        if self.online.lock().contains_key(&package) {
            return Err(package_error(
                "EXTERNAL_PACKAGE_ALREADY_ONLINE",
                "相同协议包精确版本已经有远端调试连接在线。",
                &package,
            ));
        }
        if self.has_active_local_runtime(&package) || local_archive_exists {
            return Err(package_error(
                "PROTOCOL_PACKAGE_SOURCE_CONFLICT",
                "相同协议包精确版本已由本地 Component 或远端调试连接占有。",
                &package,
            ));
        }
        let stored_registration = registration.clone();
        let enabled = match self
            .executor
            .execute(move |store| {
                store.accept_external_package_registration(
                    &stored_registration,
                    fingerprint,
                    Utc::now(),
                )
            })
            .await
            .map_err(app_error)?
        {
            StoredExternalPackageRegistrationOutcome::IdentityConflict => {
                return Err(package_error(
                    "PROTOCOL_PACKAGE_IDENTITY_CONFLICT",
                    "相同协议包 ID 与版本已经注册，但 Schema、方法或元数据不同。",
                    &package,
                ));
            }
            StoredExternalPackageRegistrationOutcome::Inserted => true,
            StoredExternalPackageRegistrationOutcome::Reconnected { enabled } => enabled,
        };
        let connection_id = ExternalPackageConnectionId::new();
        {
            // SQLite 完成后才发布内存代次；所有 parking_lot guard 都局限在无 await 的作用域内。
            // 始终按 online -> connection_details 的顺序持锁，使 Active 与详情快照一起可见。
            let mut online = self.online.lock();
            self.connection_details.lock().insert(
                package.clone(),
                ConnectionDetailSnapshot {
                    connection_id,
                    remote_address: None,
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
        }
        drop(mutation);
        self.online_changed.notify_waiters();
        self.publish_catalog_changed(&package);
        self.publish_service_status();
        Ok(AcceptedExternalPackageConnection {
            package,
            connection_id,
            enabled,
        })
    }

    /// 将 TCP accept 得到的远端地址关联到已接纳的连接代次；迟到写入不会覆盖后续连接。
    pub async fn record_remote_address(
        &self,
        package: &ProtocolPackageRef,
        connection_id: ExternalPackageConnectionId,
        remote_address: SocketAddr,
    ) -> AppResult<bool> {
        let _environment_apply_gate = self.acquire_environment_apply_package_gate(package).await;
        let gate = self.connection_mutation(package);
        let mutation = gate.lock().await;
        if !self.is_active_connection(package, connection_id) {
            return Ok(false);
        }
        let selected = package.clone();
        if !self
            .executor
            .execute(move |store| {
                store.record_external_package_remote_address(&selected, remote_address)
            })
            .await
            .map_err(app_error)?
        {
            return Err(not_found(package));
        }
        let mut details = self.connection_details.lock();
        let Some(detail) = details
            .get_mut(package)
            .filter(|detail| detail.connection_id == connection_id)
        else {
            return Ok(false);
        };
        detail.remote_address = Some(remote_address);
        detail.recent_error = None;
        drop(details);
        drop(mutation);
        self.publish_connection_online(package, connection_id, remote_address);
        Ok(true)
    }

    /// 记录连接终止的安全摘要，供详情页和后续 MCP 投影复用。
    pub async fn record_connection_error(
        &self,
        package: &ProtocolPackageRef,
        connection_id: ExternalPackageConnectionId,
        reason: &PackageTransportError,
    ) -> AppResult<bool> {
        let _environment_apply_gate = self.acquire_environment_apply_package_gate(package).await;
        let gate = self.connection_mutation(package);
        let mutation = gate.lock().await;
        if !self.is_active_connection(package, connection_id) {
            return Ok(false);
        }
        let recent_error = recent_error_view(reason);
        let selected = package.clone();
        let stored_error = recent_error.clone();
        if !self
            .executor
            .execute(move |store| {
                store.record_external_package_recent_error(
                    &selected,
                    &stored_error.code,
                    &stored_error.message,
                    stored_error.occurred_at,
                )
            })
            .await
            .map_err(app_error)?
        {
            return Err(not_found(package));
        }
        let mut details = self.connection_details.lock();
        let Some(detail) = details
            .get_mut(package)
            .filter(|detail| detail.connection_id == connection_id)
        else {
            return Ok(false);
        };
        detail.recent_error = Some(recent_error);
        drop(details);
        drop(mutation);
        self.publish_connection_offline(package, connection_id, reason);
        Ok(true)
    }

    /// 返回在线精确版本的 client 快照，供 Socket 运行时创建单业务连接适配器。
    #[must_use]
    pub fn client(&self, package: &ProtocolPackageRef) -> Option<PackageTransportClient> {
        match self.online.lock().get(package) {
            Some(OnlineConnection::Active { client, .. }) => Some(client.clone()),
            Some(OnlineConnection::Closing { .. }) | None => None,
        }
    }

    /// 处理 actor 的断线通知；只有连接 ID 仍匹配当前所有者时才移除在线状态。
    ///
    /// 该比较使迟到通知无法把同一精确版本的后续连接错误标记为离线。
    pub async fn mark_disconnected(
        &self,
        package: &ProtocolPackageRef,
        connection_id: ExternalPackageConnectionId,
    ) -> bool {
        let _environment_apply_gate = self.acquire_environment_apply_package_gate(package).await;
        let gate = self.connection_mutation(package);
        let mutation = gate.lock().await;
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
        drop(mutation);
        if matches {
            self.publish_catalog_changed(package);
            self.publish_service_status();
        }
        matches
    }

    fn is_active_connection(
        &self,
        package: &ProtocolPackageRef,
        connection_id: ExternalPackageConnectionId,
    ) -> bool {
        matches!(
            self.online.lock().get(package),
            Some(OnlineConnection::Active { id, .. }) if *id == connection_id
        )
    }

    /// 核验指定连接仍是最近一次、且尚未被重连取代的离线代次。
    /// 按既定锁顺序取得一致快照，避免旧连接的迟到清理作用于新连接。
    pub(crate) async fn is_still_offline_after(
        &self,
        package: &ProtocolPackageRef,
        connection_id: ExternalPackageConnectionId,
    ) -> bool {
        let gate = self.connection_mutation(package);
        let _mutation = gate.lock().await;
        let online = self.online.lock();
        if online.contains_key(package) {
            return false;
        }
        self.connection_details
            .lock()
            .get(package)
            .is_some_and(|detail| detail.connection_id == connection_id)
    }

    pub(crate) fn is_online(&self, package: &ProtocolPackageRef) -> bool {
        self.has_active_local_runtime(package)
            || matches!(
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

#[async_trait::async_trait]
impl ExternalSocketPackageProvider for ExternalPackageRegistryAdapter {
    async fn resolve(
        &self,
        package: &ProtocolPackageRef,
    ) -> AppResult<Option<ExternalSocketPackageBinding>> {
        let selected = package.clone();
        let Some(stored) = self
            .executor
            .execute(move |store| store.get_external_package(&selected))
            .await
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
        if let Some(runtime) = self.active_local_runtime(package) {
            return Ok(Some(ExternalSocketPackageBinding::with_limits(
                stored.registration,
                runtime,
                usize::MAX,
            )));
        }
        if stored.local_archive.is_some() {
            return Err(package_error(
                "PROTOCOL_PACKAGE_RUNTIME_OFFLINE",
                "本地 Wasm 协议包当前未成功实例化，无法启动绑定入口。",
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
        Ok(Some(ExternalSocketPackageBinding::with_limits(
            stored.registration,
            Arc::new(client),
            max_frame_bytes,
        )))
    }
}

#[cfg(test)]
mod tests;
