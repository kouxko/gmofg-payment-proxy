//! 动态 Listener 的 Workspace 配置与网络生命周期用例。

use chrono::Utc;
use intercept_proxy_domain::Revision as DomainRevision;

use super::Application;
use crate::{
    AppError, AppResult, CertificateReference, ListenerDataPlane, ListenerId,
    ListenerOverviewViewModel, ListenerStatusViewModel, OperationResultViewModel, ProxyListener,
    ProxyWorkspace, SocketRelaySecurity, SocketTopology, UiEventPayload, UiTone,
    WorkspaceChangeKind, WorkspaceId, WorkspaceValidationViewModel,
};

use model::{
    build_listener_overview, copy_listener_draft, ensure_listener_not_running, find_listener,
    listener_not_found, merge_new_certificate_references, set_listener_enabled,
    validate_new_certificate_references,
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
        reject_unavailable_local_responder(&source)?;
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
        certificate_references: Vec<CertificateReference>,
    ) -> AppResult<ProxyWorkspace> {
        let _gate = self.mutation_gate.lock().await;
        let workspace = self.workspaces.get(workspace_id).await?;
        workspace
            .revision
            .verify(DomainRevision::new(expected_workspace_revision))
            .map_err(AppError::from)?;
        ensure_listener_not_running(&*self.listener_runtime, listener.id).await?;
        let listener_id = listener.id;
        let workspace = self
            .listener_draft_workspace(
                workspace,
                expected_workspace_revision,
                listener,
                certificate_references,
            )
            .await?;
        workspace.validate().map_err(AppError::from)?;
        self.validate_listener_protocol_package(&workspace, listener_id, false)
            .await?;
        let saved = self.workspaces.save(workspace).await?;
        self.publish_workspace(&saved, true, WorkspaceChangeKind::Updated);
        Ok(saved)
    }

    /// 仅校验当前监听草稿及已持久化的其他监听。
    ///
    /// 前端可能同时保留多个未保存草稿；保存、启动或测试某一监听时，不应被另一个
    /// 未保存草稿阻断。Rust 会从仓储读取当前 Workspace，只替换目标监听和本次原生
    /// 导入的托管证书引用，再执行完整领域校验。
    pub async fn listener_validate(
        &self,
        workspace_id: WorkspaceId,
        expected_workspace_revision: u64,
        listener: ProxyListener,
        certificate_references: Vec<CertificateReference>,
    ) -> AppResult<WorkspaceValidationViewModel> {
        let _gate = self.mutation_gate.lock().await;
        let workspace = self.workspaces.get(workspace_id).await?;
        workspace
            .revision
            .verify(DomainRevision::new(expected_workspace_revision))
            .map_err(AppError::from)?;
        let listener_id = listener.id;
        let candidate = self
            .listener_draft_workspace(
                workspace,
                expected_workspace_revision,
                listener,
                certificate_references,
            )
            .await?;
        let validation = WorkspaceValidationViewModel::validate(candidate.clone());
        if !validation.valid {
            return Ok(validation);
        }
        self.validate_listener_protocol_package(&candidate, listener_id, false)
            .await?;
        Ok(validation)
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

    /// 使用经 Rust 校验的当前监听草稿测试真实上游 TLS 握手。
    ///
    /// 只允许已配置 HTTPS 固定 Server 的监听器调用。
    /// 证书材料由 infrastructure 根据安全引用读取，应用层与前端都不会接触客户端私钥
    /// 或 PKCS#12 密码。
    pub async fn listener_test_upstream_tls(
        &self,
        workspace_id: WorkspaceId,
        expected_workspace_revision: u64,
        listener: ProxyListener,
        certificate_references: Vec<CertificateReference>,
    ) -> AppResult<crate::ListenerUpstreamTlsTestViewModel> {
        let workspace = self.workspaces.get(workspace_id).await?;
        reject_local_responder_after_revision_check(
            &workspace,
            expected_workspace_revision,
            &listener,
        )?;
        let candidate = self
            .listener_draft_workspace(
                workspace,
                expected_workspace_revision,
                listener.clone(),
                certificate_references,
            )
            .await?;
        candidate.validate().map_err(AppError::from)?;

        if !has_upstream_tls(&listener) {
            return Err(AppError::new(
                "UPSTREAM_TLS_NOT_ENABLED",
                "该入口的上游连接没有启用 TLS。",
            )
            .entity(listener.id.to_string()));
        }
        self.listener_runtime
            .test_upstream_tls(candidate, listener)
            .await
    }

    /// 按固定 Server scheme 执行真实连接测试：HTTP=DNS/TCP，HTTPS=DNS/TCP+TLS。
    pub async fn listener_test_upstream_connection(
        &self,
        workspace_id: WorkspaceId,
        expected_workspace_revision: u64,
        listener: ProxyListener,
        certificate_references: Vec<CertificateReference>,
    ) -> AppResult<crate::ListenerUpstreamConnectionTestViewModel> {
        let workspace = self.workspaces.get(workspace_id).await?;
        reject_local_responder_after_revision_check(
            &workspace,
            expected_workspace_revision,
            &listener,
        )?;
        let candidate = self
            .listener_draft_workspace(
                workspace,
                expected_workspace_revision,
                listener.clone(),
                certificate_references,
            )
            .await?;
        candidate.validate().map_err(AppError::from)?;
        if !has_fixed_target(&listener) {
            return Err(AppError::new(
                "LISTENER_CONNECTION_TEST_UNSUPPORTED",
                "动态 HTTP 监听器没有可独立测试的固定上游。",
            )
            .entity(listener.id.to_string()));
        }
        self.listener_runtime
            .test_upstream_connection(candidate, listener)
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

    /// 以“运行时启动 + Workspace 状态保存”组成补偿事务。
    ///
    /// mutation gate 串行化监听配置变更；先让 runtime 真实绑定端口，成功后才把 `enabled`
    /// 写入 Workspace。若持久化因 revision 冲突或 I/O 失败，立即 stop 刚启动的 listener，
    /// 避免界面/数据库显示停止但端口仍开放。
    /// 只有两个阶段都成功才发布状态事件，因此订阅者
    /// 不会观察到尚未提交的短暂 Running 状态。
    pub async fn listener_start(
        &self,
        workspace_id: WorkspaceId,
        expected_workspace_revision: u64,
        listener_id: ListenerId,
    ) -> AppResult<ListenerStatusViewModel> {
        let _gate = self.mutation_gate.lock().await;
        let mut workspace = self.workspaces.get(workspace_id).await?;
        workspace
            .revision
            .verify(DomainRevision::new(expected_workspace_revision))
            .map_err(AppError::from)?;
        let listener = find_listener(&workspace.listeners, listener_id)?;
        workspace.revision = DomainRevision::new(expected_workspace_revision);
        // Scripted Listener 可以保存对已停用版本的精确引用，便于之后重新启用；但实际
        // 打开网络入口前必须在同一个 mutation gate 内重新确认包仍可用。
        self.validate_listener_protocol_package(&workspace, listener.id, true)
            .await?;
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
        _expected_workspace_revision: u64,
        listener_id: ListenerId,
    ) -> AppResult<ListenerStatusViewModel> {
        let _gate = self.mutation_gate.lock().await;
        let workspace = self.workspaces.get(workspace_id).await?;
        find_listener(&workspace.listeners, listener_id)?;
        let status = self.listener_runtime.stop(listener_id).await?;

        // “停止网络入口”是安全优先的运行时操作，不应被其他监听或设备方案推进的
        // Workspace revision 阻断。停止后读取最新聚合并仅写回当前监听状态。
        let mut workspace = self.workspaces.get(workspace_id).await?;
        set_listener_enabled(&mut workspace.listeners, listener_id, false)?;
        // 即使持久化层失败，也保持端口关闭；绝不能使用旧快照自动重新开放监听。
        self.publish_listener_status(status.clone());
        self.workspaces.save(workspace).await?;
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

    async fn listener_draft_workspace(
        &self,
        mut workspace: ProxyWorkspace,
        expected_workspace_revision: u64,
        listener: ProxyListener,
        certificate_references: Vec<CertificateReference>,
    ) -> AppResult<ProxyWorkspace> {
        validate_new_certificate_references(
            &*self.listener_certificates,
            &workspace.certificate_references,
            &certificate_references,
        )
        .await?;
        workspace.revision = DomainRevision::new(expected_workspace_revision);
        if let Some(current) = workspace
            .listeners
            .iter_mut()
            .find(|current| current.id == listener.id)
        {
            *current = listener;
        } else {
            workspace.listeners.push(listener);
        }
        merge_new_certificate_references(
            &mut workspace.certificate_references,
            certificate_references,
        );
        Ok(workspace)
    }
}

fn has_fixed_target(listener: &ProxyListener) -> bool {
    match &listener.data_plane {
        ListenerDataPlane::Http(settings) => settings.fixed_server.is_some(),
        ListenerDataPlane::Socket(settings) => {
            matches!(settings.topology, SocketTopology::Relay(_))
        }
    }
}

/// T21 允许 `LocalResponder` 通过保存和启动前校验，但真实数据面仍留给 T22。
/// 复制入口继续隐藏这种尚不能由 UI 创建的拓扑；两个上游探测入口也在 runtime 前调用
/// 此门禁，保证 `LocalResponder` 永远不会误触发 DNS、连接或 upstream TLS。
fn reject_unavailable_local_responder(listener: &ProxyListener) -> AppResult<()> {
    if matches!(
        &listener.data_plane,
        ListenerDataPlane::Socket(settings)
            if matches!(settings.topology, SocketTopology::LocalResponder(_))
    ) {
        return Err(AppError::new(
            "LOCAL_RESPONDER_NOT_AVAILABLE",
            "LocalResponder 当前不能复制或测试上游；运行计划由启动路径单独校验。",
        )
        .entity(listener.id.to_string()));
    }
    Ok(())
}

/// 保留 Workspace identity/revision 作为所有写入和探测入口的第一层并发门禁；通过后再
/// 返回稳定 unavailable。这样 stale draft 仍报告 revision conflict，而当前 draft
/// 不会进入证书、协议包或 runtime ports。
fn reject_local_responder_after_revision_check(
    workspace: &ProxyWorkspace,
    expected_workspace_revision: u64,
    listener: &ProxyListener,
) -> AppResult<()> {
    if matches!(
        &listener.data_plane,
        ListenerDataPlane::Socket(settings)
            if matches!(settings.topology, SocketTopology::LocalResponder(_))
    ) {
        workspace
            .revision
            .verify(DomainRevision::new(expected_workspace_revision))
            .map_err(AppError::from)?;
        reject_unavailable_local_responder(listener)?;
    }
    Ok(())
}

fn has_upstream_tls(listener: &ProxyListener) -> bool {
    match &listener.data_plane {
        ListenerDataPlane::Http(settings) => settings.fixed_server.as_ref().is_some_and(|fixed| {
            fixed
                .upstream_url
                .get(..8)
                .is_some_and(|scheme| scheme.eq_ignore_ascii_case("https://"))
        }),
        ListenerDataPlane::Socket(settings) => match &settings.topology {
            SocketTopology::Relay(relay) => matches!(
                relay.security,
                SocketRelaySecurity::TcpToTls { .. } | SocketRelaySecurity::TlsToTls { .. }
            ),
            SocketTopology::LocalResponder(_) => false,
        },
    }
}

mod model;

#[cfg(test)]
#[path = "listeners_tests.rs"]
mod tests;
