//! 动态 Listener 的 Workspace 配置与网络生命周期用例。

use std::collections::BTreeMap;

use chrono::Utc;
use intercept_proxy_domain::Revision as DomainRevision;

use super::Application;
use crate::{
    AppError, AppResult, ListenerId, ListenerMonitorRowViewModel, ListenerOverviewViewModel,
    ListenerRuntimeState, ListenerStatusViewModel, OperationResultViewModel, ProxyListener,
    ProxyWorkspace, UiEventPayload, UiTone, WorkspaceChangeKind, WorkspaceId,
};

impl Application {
    /// 由 Rust 创建带稳定 ID 的未保存代理监听草稿。
    ///
    /// 草稿默认按请求目标动态转发；用户可在同一配置中开启 `fixed_server`，不需要先
    /// 选择“正向/反向”类型，也不会因为切换路由方式而丢失监听器 ID 与公共配置。
    pub fn listener_new(&self) -> AppResult<ProxyListener> {
        Ok(ProxyListener::default())
    }

    /// 复制一条尚未保存或已经保存的 Listener 草稿。
    ///
    /// 复制动作必须经过 Rust：新 ID 由领域类型生成，运行状态强制关闭，避免前端复制
    /// 旧 ID 或把一个正在运行的监听器误认为第二个独立运行实例。端口和上游配置保留，
    /// 便于用户以现有映射为模板，再修改为另一条本地端口 -> 上游 origin 映射。
    pub fn listener_copy(&self, source: ProxyListener) -> AppResult<ProxyListener> {
        Ok(copy_listener_draft(source))
    }

    pub async fn listener_list(&self, workspace_id: WorkspaceId) -> AppResult<Vec<ProxyListener>> {
        Ok(self.workspaces.get(workspace_id).await?.listeners)
    }

    pub async fn listener_get(
        &self,
        workspace_id: WorkspaceId,
        listener_id: ListenerId,
    ) -> AppResult<ProxyListener> {
        find_listener(
            &self.workspaces.get(workspace_id).await?.listeners,
            listener_id,
        )
    }

    pub async fn listener_save(
        &self,
        workspace_id: WorkspaceId,
        expected_workspace_revision: u64,
        listener: ProxyListener,
    ) -> AppResult<ProxyListener> {
        let _gate = self.mutation_gate.lock().await;
        let mut workspace = self.workspaces.get(workspace_id).await?;
        ensure_listener_not_running(&*self.listener_runtime, listener.id).await?;
        workspace.revision = DomainRevision::new(expected_workspace_revision);
        if let Some(current) = workspace
            .listeners
            .iter_mut()
            .find(|current| current.id == listener.id)
        {
            *current = listener.clone();
        } else {
            workspace.listeners.push(listener.clone());
        }
        let saved = self.workspaces.save(workspace).await?;
        self.publish_workspace(&saved, true, WorkspaceChangeKind::Updated);
        Ok(listener)
    }

    pub async fn listener_delete(
        &self,
        workspace_id: WorkspaceId,
        expected_workspace_revision: u64,
        listener_id: ListenerId,
    ) -> AppResult<OperationResultViewModel> {
        let _gate = self.mutation_gate.lock().await;
        let mut workspace = self.workspaces.get(workspace_id).await?;
        ensure_listener_not_running(&*self.listener_runtime, listener_id).await?;
        workspace.revision = DomainRevision::new(expected_workspace_revision);
        let before = workspace.listeners.len();
        workspace
            .listeners
            .retain(|listener| listener.id != listener_id);
        if before == workspace.listeners.len() {
            return Err(listener_not_found(listener_id));
        }
        let saved = self.workspaces.save(workspace).await?;
        self.publish_workspace(&saved, true, WorkspaceChangeKind::Updated);
        Ok(OperationResultViewModel {
            success: true,
            cancelled: false,
            message: "Listener 已删除。".into(),
            ui_tone: UiTone::Positive,
            entity_id: Some(listener_id.to_string()),
            revision: Some(saved.revision.get()),
            requires_restart: false,
        })
    }

    pub async fn listener_statuses(&self) -> AppResult<Vec<ListenerStatusViewModel>> {
        self.listener_runtime.statuses().await
    }

    /// 使用已经持久化的入口配置测试真实上游 TLS 握手。
    ///
    /// 只允许已配置 HTTPS 固定 Server 的监听器调用。证书材料由 infrastructure 根据安全引用读取，应用层与
    /// 前端都不会接触客户端私钥或 PKCS#12 密码。
    pub async fn listener_test_upstream_tls(
        &self,
        workspace_id: WorkspaceId,
        listener_id: ListenerId,
    ) -> AppResult<crate::ListenerUpstreamTlsTestViewModel> {
        let workspace = self.workspaces.get(workspace_id).await?;
        let listener = find_listener(&workspace.listeners, listener_id)?;
        let Some(fixed_server) = &listener.fixed_server else {
            return Err(AppError::new(
                "LISTENER_TLS_TEST_UNSUPPORTED",
                "该监听器未开启固定 Server，无法测试单一 Server TLS。",
            )
            .entity(listener_id.to_string()));
        };
        if !fixed_server
            .upstream_url
            .to_ascii_lowercase()
            .starts_with("https://")
        {
            return Err(AppError::new(
                "UPSTREAM_TLS_NOT_ENABLED",
                "固定 Server 使用 HTTP，没有 TLS 握手可测试。",
            )
            .entity(listener_id.to_string()));
        }
        self.listener_runtime
            .test_upstream_tls(workspace, listener)
            .await
    }

    /// 返回当前 Workspace 的入口配置与实际运行状态合并快照。
    ///
    /// 停止状态同样由 Rust 补齐，前端只渲染结果，不根据“运行状态列表里没有该 ID”
    /// 自行推断业务状态。
    pub async fn listener_overview(
        &self,
        workspace_id: WorkspaceId,
    ) -> AppResult<ListenerOverviewViewModel> {
        let workspace = self.workspaces.get(workspace_id).await?;
        let statuses = self.listener_runtime.statuses().await?;
        Ok(build_listener_overview(workspace, statuses))
    }

    pub async fn listener_start(
        &self,
        workspace_id: WorkspaceId,
        expected_workspace_revision: u64,
        listener_id: ListenerId,
    ) -> AppResult<ListenerStatusViewModel> {
        let _gate = self.mutation_gate.lock().await;
        let mut workspace = self.workspaces.get(workspace_id).await?;
        workspace.revision = DomainRevision::new(expected_workspace_revision);
        let listener = find_listener(&workspace.listeners, listener_id)?;
        let status = self
            .listener_runtime
            .start(workspace.clone(), listener)
            .await?;
        set_listener_enabled(&mut workspace.listeners, listener_id, true)?;
        if let Err(error) = self.workspaces.save(workspace).await {
            let _ = self.listener_runtime.stop(listener_id).await;
            return Err(error);
        }
        self.publish_listener_status(status.clone());
        Ok(status)
    }

    pub async fn listener_stop(
        &self,
        workspace_id: WorkspaceId,
        expected_workspace_revision: u64,
        listener_id: ListenerId,
    ) -> AppResult<ListenerStatusViewModel> {
        let _gate = self.mutation_gate.lock().await;
        let workspace = self.workspaces.get(workspace_id).await?;
        workspace
            .revision
            .verify(DomainRevision::new(expected_workspace_revision))?;
        let original = find_listener(&workspace.listeners, listener_id)?;
        let status = self.listener_runtime.stop(listener_id).await?;

        // 停止最后一个 Listener 时，运行时会重置一次性规则和命中计数；该操作会推进
        // Workspace revision。因此必须在网络运行时完全停止后重新读取最新 Workspace，
        // 再持久化 Listener 的 disabled 状态。继续保存停止前的快照会把合法的运行时更新
        // 误判为 REVISION_CONFLICT，也可能在错误恢复路径中重新启动已经停止的 Listener。
        let mut workspace = self.workspaces.get(workspace_id).await?;
        set_listener_enabled(&mut workspace.listeners, listener_id, false)?;
        let restart_snapshot = workspace.clone();
        if let Err(error) = self.workspaces.save(workspace).await {
            let _ = self
                .listener_runtime
                .start(restart_snapshot, original)
                .await;
            return Err(error);
        }
        self.publish_listener_status(status.clone());
        Ok(status)
    }

    fn publish_listener_status(&self, status: ListenerStatusViewModel) {
        self.events.publish(
            None,
            Utc::now(),
            Some(status.listener_id.to_string()),
            None,
            UiEventPayload::ListenerStatusChanged(status),
        );
    }
}

async fn ensure_listener_not_running(
    runtime: &dyn crate::ListenerRuntimePort,
    listener_id: ListenerId,
) -> AppResult<()> {
    if runtime
        .statuses()
        .await?
        .iter()
        .any(|status| status.listener_id == listener_id)
    {
        return Err(AppError::new(
            "LISTENER_RUNTIME_ACTIVE",
            "Listener 正在运行；请停止后再保存或删除配置。",
        )
        .entity(listener_id.to_string()));
    }
    Ok(())
}

fn build_listener_overview(
    workspace: ProxyWorkspace,
    statuses: Vec<ListenerStatusViewModel>,
) -> ListenerOverviewViewModel {
    let mut statuses = statuses
        .into_iter()
        .map(|status| (status.listener_id, status))
        .collect::<BTreeMap<_, _>>();
    let rows = workspace
        .listeners
        .iter()
        .map(|listener| {
            let id = listener.id;
            let (address, port) = listener.bind_endpoint();
            let status = statuses.remove(&id).unwrap_or(ListenerStatusViewModel {
                listener_id: id,
                state: ListenerRuntimeState::Stopped,
                state_text: "已停止".into(),
                ui_tone: UiTone::Neutral,
                listen_address: format!("{address}:{port}"),
                fault_reason: None,
                can_start: true,
                can_stop: false,
            });
            let (kind_text, request_destination) = listener.fixed_server.as_ref().map_or_else(
                || ("动态目标".to_owned(), "请求中的目标地址".to_owned()),
                |fixed| ("固定 Server".to_owned(), fixed.upstream_url.clone()),
            );
            ListenerMonitorRowViewModel {
                listener_id: id,
                name: listener.name.clone(),
                kind_text,
                listen_address: status.listen_address,
                request_destination,
                state: status.state,
                state_text: status.state_text,
                ui_tone: status.ui_tone,
                fault_reason: status.fault_reason,
            }
        })
        .collect::<Vec<_>>();
    let active_count = rows
        .iter()
        .filter(|row| {
            matches!(
                row.state,
                ListenerRuntimeState::Starting
                    | ListenerRuntimeState::Running
                    | ListenerRuntimeState::Stopping
            )
        })
        .count();
    let faulted_count = rows
        .iter()
        .filter(|row| row.state == ListenerRuntimeState::Faulted)
        .count();
    let total_count = rows.len();
    let (state_text, ui_tone) = if faulted_count > 0 {
        ("部分入口故障".into(), UiTone::Danger)
    } else if total_count == 0 {
        ("未配置入口".into(), UiTone::Neutral)
    } else if active_count == total_count {
        ("全部入口运行中".into(), UiTone::Positive)
    } else if active_count > 0 {
        ("部分入口运行中".into(), UiTone::Warning)
    } else {
        ("全部入口已停止".into(), UiTone::Neutral)
    };
    ListenerOverviewViewModel {
        workspace_id: workspace.id,
        workspace_name: workspace.name,
        state_text,
        ui_tone,
        total_count,
        active_count,
        faulted_count,
        rows,
    }
}

fn copy_listener_draft(mut source: ProxyListener) -> ProxyListener {
    source.id = ListenerId::new();
    source.name = format!("{} 副本", source.name.trim());
    source.enabled = false;
    source
}

fn find_listener(listeners: &[ProxyListener], listener_id: ListenerId) -> AppResult<ProxyListener> {
    listeners
        .iter()
        .find(|listener| listener.id == listener_id)
        .cloned()
        .ok_or_else(|| listener_not_found(listener_id))
}

fn set_listener_enabled(
    listeners: &mut [ProxyListener],
    listener_id: ListenerId,
    enabled: bool,
) -> AppResult<()> {
    let listener = listeners
        .iter_mut()
        .find(|listener| listener.id == listener_id)
        .ok_or_else(|| listener_not_found(listener_id))?;
    listener.enabled = enabled;
    Ok(())
}

fn listener_not_found(listener_id: ListenerId) -> AppError {
    AppError::new("LISTENER_NOT_FOUND", "Listener 不存在或已被删除。")
        .entity(listener_id.to_string())
}

#[cfg(test)]
mod tests {
    use intercept_proxy_domain::{FixedServerSettings, UpstreamTlsSettings};

    use super::*;

    #[test]
    fn copied_listener_preserves_fixed_server_and_stops_by_default() {
        let original_id = ListenerId::new();
        let source = ProxyListener {
            id: original_id,
            name: "Transaction".into(),
            enabled: true,
            bind_address: "0.0.0.0".into(),
            port: 16_627,
            fixed_server: Some(FixedServerSettings {
                upstream_url: "https://transaction.example.test:16627".into(),
                upstream_tls: UpstreamTlsSettings::default(),
            }),
            ..ProxyListener::default()
        };

        let copy = copy_listener_draft(source);
        assert_ne!(copy.id, original_id);
        assert_eq!(copy.name, "Transaction 副本");
        assert!(!copy.enabled);
        assert_eq!(copy.port, 16_627);
        assert_eq!(
            copy.fixed_server.unwrap().upstream_url,
            "https://transaction.example.test:16627"
        );
    }

    #[test]
    fn overview_uses_workspace_as_the_only_listener_catalog() {
        let mut workspace = ProxyWorkspace::default();
        let forward_id = workspace.listeners[0].id;
        workspace.listeners.push(ProxyListener {
            id: ListenerId::new(),
            name: "API 固定上游".into(),
            enabled: false,
            bind_address: "127.0.0.1".into(),
            port: 9_001,
            fixed_server: Some(FixedServerSettings {
                upstream_url: "https://api.example.test:9443".into(),
                upstream_tls: UpstreamTlsSettings::default(),
            }),
            ..ProxyListener::default()
        });
        let overview = build_listener_overview(
            workspace,
            vec![ListenerStatusViewModel {
                listener_id: forward_id,
                state: ListenerRuntimeState::Running,
                state_text: "运行中".into(),
                ui_tone: UiTone::Positive,
                listen_address: "127.0.0.1:8080".into(),
                fault_reason: None,
                can_start: false,
                can_stop: true,
            }],
        );

        assert_eq!(overview.total_count, 2);
        assert_eq!(overview.active_count, 1);
        assert_eq!(overview.state_text, "部分入口运行中");
        assert_eq!(overview.rows[0].request_destination, "请求中的目标地址");
        assert_eq!(overview.rows[1].state_text, "已停止");
        assert_eq!(
            overview.rows[1].request_destination,
            "https://api.example.test:9443"
        );
    }
}
