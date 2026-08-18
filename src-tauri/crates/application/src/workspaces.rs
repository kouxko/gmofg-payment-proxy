//! Workspace 的 UI 无关内存仓储。
//!
//! 该实现既是无界面单元测试夹具，也是 `SQLite` 适配器的行为参考：
//! 所有更新执行乐观锁，所有导入先拒绝秘密字段并经过领域校验，
//! 所有导出只序列化安全领域模型。Tauri/Dialog
//! 仅负责取得或保存文件字节，不得在展示层重新实现这些规则。

use std::collections::BTreeMap;

use async_trait::async_trait;
use intercept_proxy_domain::{
    CertificateReferenceId, ChannelId, DownstreamClientAuthentication, FaultPresetId,
    ListenerDataPlane, ListenerId, ResponseAssertionId, Revision, RuleId, SocketDownstreamSecurity,
    SocketRelaySecurity, SocketTopology,
};
use parking_lot::RwLock;
use uuid::Uuid;

use crate::{
    AppError, AppResult, OperationResultViewModel, ProxyWorkspace, UiTone, WorkspaceId,
    WorkspaceRepositoryPort, WorkspaceSummaryViewModel, WorkspaceValidationViewModel,
};

/// 为复制或导入的 Workspace 生成完全独立的聚合身份。
///
/// 所有嵌套实体 ID 以及 Workspace 内部引用必须一起重映射；只替换顶层 ID 会让运行时
/// Map、未来的分表持久化以及复制后的编辑继续与源 Workspace 发生别名冲突。SQLite 与
/// 无界面内存仓储共同调用本函数，避免两个实现逐渐产生不同语义。
#[allow(clippy::too_many_lines)]
pub fn remap_workspace_identity(workspace: &mut ProxyWorkspace) -> AppResult<()> {
    let listener_ids = workspace
        .listeners
        .iter()
        .map(|listener| (listener.id, ListenerId::new()))
        .collect::<BTreeMap<_, _>>();
    let certificate_ids = workspace
        .certificate_references
        .iter()
        .map(|reference| (reference.id, CertificateReferenceId::new()))
        .collect::<BTreeMap<_, _>>();

    for listener in &mut workspace.listeners {
        listener.id = mapped(&listener_ids, listener.id, "Listener")?;
        remap_listener_certificates(listener, &certificate_ids)?;
    }

    for assertion in &mut workspace.response_assertions {
        assertion.id = ResponseAssertionId::new();
        remap_listener_references(&mut assertion.listener_ids, &listener_ids)?;
    }
    for preset in &mut workspace.fault_presets {
        preset.id = FaultPresetId::new();
    }
    for rule in &mut workspace.rules {
        rule.id = RuleId::new();
        if let Some(channel) = &rule.channel
            && let Some(listener_id) = listener_ids
                .iter()
                .find_map(|(old, new)| (channel.as_str() == old.to_string()).then_some(*new))
        {
            rule.channel = Some(ChannelId::new(listener_id.to_string()).map_err(AppError::from)?);
        }
    }
    for rule in &mut workspace.socket_rules {
        let listener_id = mapped(
            &listener_ids,
            rule.listener_id(),
            "Socket rule Listener reference",
        )?;
        // Socket rule ID 的作用域是 Workspace；复制/导入保留规则身份、revision、创建顺序
        // 与声明顺序，只重绑随聚合一起变化的 Listener ID。
        rule.rebind_listener_for_workspace_remap(listener_id)?;
    }
    for reference in &mut workspace.certificate_references {
        reference.id = mapped(&certificate_ids, reference.id, "certificate reference")?;
    }
    for profile in &mut workspace.android_network_profiles {
        profile.id = Uuid::new_v4().to_string();
        for route in &mut profile.proxy_routes {
            route.listener_id = mapped(
                &listener_ids,
                route.listener_id,
                "Android transparent proxy route",
            )?;
        }
    }
    workspace.id = WorkspaceId::new();
    workspace.revision = Revision::INITIAL;
    workspace.validate().map_err(AppError::from)
}

fn remap_listener_certificates(
    listener: &mut crate::ProxyListener,
    mapping: &BTreeMap<CertificateReferenceId, CertificateReferenceId>,
) -> AppResult<()> {
    match &mut listener.data_plane {
        ListenerDataPlane::Http(settings) => {
            settings.mitm.root_ca = remap_optional(settings.mitm.root_ca, mapping, "MITM Root CA")?;
            settings.downstream_tls.server_identity = remap_optional(
                settings.downstream_tls.server_identity,
                mapping,
                "server identity",
            )?;
            remap_client_authentication(
                &mut settings.downstream_tls.client_authentication,
                mapping,
            )?;
            if let Some(fixed_server) = &mut settings.fixed_server {
                fixed_server.upstream_tls.server_trust = remap_optional(
                    fixed_server.upstream_tls.server_trust,
                    mapping,
                    "upstream trust",
                )?;
                fixed_server.upstream_tls.client_identity = remap_optional(
                    fixed_server.upstream_tls.client_identity,
                    mapping,
                    "upstream identity",
                )?;
            }
        }
        ListenerDataPlane::Socket(settings) => match &mut settings.topology {
            SocketTopology::Relay(relay) => match &mut relay.security {
                SocketRelaySecurity::Transparent => {}
                SocketRelaySecurity::TcpToTls { upstream_tls } => {
                    remap_socket_upstream(upstream_tls, mapping)?;
                }
                SocketRelaySecurity::TlsToTcp { downstream_tls } => {
                    remap_socket_downstream(downstream_tls, mapping)?;
                }
                SocketRelaySecurity::TlsToTls {
                    downstream_tls,
                    upstream_tls,
                } => {
                    remap_socket_downstream(downstream_tls, mapping)?;
                    remap_socket_upstream(upstream_tls, mapping)?;
                }
            },
            SocketTopology::LocalResponder(local) => {
                if let SocketDownstreamSecurity::Tls { downstream_tls } =
                    &mut local.downstream_security
                {
                    remap_socket_downstream(downstream_tls, mapping)?;
                }
            }
        },
    }
    Ok(())
}

fn remap_client_authentication(
    value: &mut DownstreamClientAuthentication,
    mapping: &BTreeMap<CertificateReferenceId, CertificateReferenceId>,
) -> AppResult<()> {
    match value {
        DownstreamClientAuthentication::Disabled => {}
        DownstreamClientAuthentication::Optional { trust }
        | DownstreamClientAuthentication::Required { trust } => {
            *trust = mapped(mapping, *trust, "client trust")?;
        }
    }
    Ok(())
}

fn remap_socket_downstream(
    value: &mut intercept_proxy_domain::SocketDownstreamTlsSettings,
    mapping: &BTreeMap<CertificateReferenceId, CertificateReferenceId>,
) -> AppResult<()> {
    value.server_identity = mapped(mapping, value.server_identity, "server identity")?;
    remap_client_authentication(&mut value.client_authentication, mapping)
}

fn remap_socket_upstream(
    value: &mut intercept_proxy_domain::SocketUpstreamTlsSettings,
    mapping: &BTreeMap<CertificateReferenceId, CertificateReferenceId>,
) -> AppResult<()> {
    value.server_trust = remap_optional(value.server_trust, mapping, "upstream trust")?;
    value.client_identity = remap_optional(value.client_identity, mapping, "upstream identity")?;
    Ok(())
}

fn remap_optional(
    value: Option<CertificateReferenceId>,
    mapping: &BTreeMap<CertificateReferenceId, CertificateReferenceId>,
    label: &str,
) -> AppResult<Option<CertificateReferenceId>> {
    value.map(|id| mapped(mapping, id, label)).transpose()
}

fn remap_listener_references(
    ids: &mut [ListenerId],
    mapping: &BTreeMap<ListenerId, ListenerId>,
) -> AppResult<()> {
    for id in ids {
        *id = mapped(mapping, *id, "Listener reference")?;
    }
    Ok(())
}

fn mapped<K: Copy + Ord, V: Copy>(mapping: &BTreeMap<K, V>, id: K, label: &str) -> AppResult<V> {
    mapping.get(&id).copied().ok_or_else(|| {
        AppError::new(
            "IMPORT_FAILED",
            format!("{label} 引用在 Workspace 身份重映射时丢失。"),
        )
    })
}

#[derive(Debug)]
pub struct InMemoryWorkspaceStore {
    state: RwLock<WorkspaceState>,
}

#[derive(Debug, Default)]
struct WorkspaceState {
    selected: Option<WorkspaceId>,
    workspaces: BTreeMap<WorkspaceId, ProxyWorkspace>,
}

impl Default for InMemoryWorkspaceStore {
    fn default() -> Self {
        let workspace = ProxyWorkspace::default();
        let selected = workspace.id;
        Self {
            state: RwLock::new(WorkspaceState {
                selected: Some(selected),
                workspaces: BTreeMap::from([(selected, workspace)]),
            }),
        }
    }
}

impl InMemoryWorkspaceStore {
    #[must_use]
    pub fn new_empty() -> Self {
        Self {
            state: RwLock::new(WorkspaceState::default()),
        }
    }

    fn summaries(state: &WorkspaceState) -> Vec<WorkspaceSummaryViewModel> {
        state
            .workspaces
            .values()
            .map(|workspace| {
                WorkspaceSummaryViewModel::from_workspace(
                    workspace,
                    state.selected == Some(workspace.id),
                )
            })
            .collect()
    }

    fn get_stored(state: &WorkspaceState, id: WorkspaceId) -> AppResult<ProxyWorkspace> {
        state.workspaces.get(&id).cloned().ok_or_else(|| {
            AppError::new("WORKSPACE_NOT_FOUND", "Workspace 不存在或已被删除。")
                .entity(id.to_string())
        })
    }
}

#[async_trait]
impl WorkspaceRepositoryPort for InMemoryWorkspaceStore {
    async fn list(&self) -> AppResult<Vec<WorkspaceSummaryViewModel>> {
        Ok(Self::summaries(&self.state.read()))
    }

    async fn get(&self, workspace_id: WorkspaceId) -> AppResult<ProxyWorkspace> {
        Self::get_stored(&self.state.read(), workspace_id)
    }

    async fn create(&self, name: String) -> AppResult<ProxyWorkspace> {
        let mut workspace = ProxyWorkspace {
            name: name.trim().to_owned(),
            ..ProxyWorkspace::default()
        };
        workspace.validate().map_err(AppError::from)?;
        // 明确重置 revision，避免将来 Default 改变时破坏新建语义。
        workspace.revision = intercept_proxy_domain::Revision::INITIAL;
        let mut state = self.state.write();
        state.workspaces.insert(workspace.id, workspace.clone());
        if state.selected.is_none() {
            state.selected = Some(workspace.id);
        }
        Ok(workspace)
    }

    async fn copy(&self, workspace_id: WorkspaceId) -> AppResult<ProxyWorkspace> {
        let mut state = self.state.write();
        let source = Self::get_stored(&state, workspace_id)?;
        let mut copy = source;
        remap_workspace_identity(&mut copy)?;
        copy.name = format!("{} Copy", copy.name);
        copy.validate().map_err(AppError::from)?;
        state.workspaces.insert(copy.id, copy.clone());
        Ok(copy)
    }

    async fn select(&self, workspace_id: WorkspaceId) -> AppResult<WorkspaceSummaryViewModel> {
        let mut state = self.state.write();
        let workspace = Self::get_stored(&state, workspace_id)?;
        state.selected = Some(workspace_id);
        Ok(WorkspaceSummaryViewModel::from_workspace(&workspace, true))
    }

    async fn validate(&self, workspace: ProxyWorkspace) -> AppResult<WorkspaceValidationViewModel> {
        Ok(WorkspaceValidationViewModel::validate(workspace))
    }

    async fn save(&self, mut workspace: ProxyWorkspace) -> AppResult<ProxyWorkspace> {
        workspace.validate().map_err(AppError::from)?;
        let mut state = self.state.write();
        let current = state.workspaces.get(&workspace.id).ok_or_else(|| {
            AppError::new("WORKSPACE_NOT_FOUND", "Workspace 不存在或已被删除。")
                .entity(workspace.id.to_string())
        })?;
        current
            .revision
            .verify(workspace.revision)
            .map_err(AppError::from)?;
        workspace.revision = current.revision.next();
        state.workspaces.insert(workspace.id, workspace.clone());
        Ok(workspace)
    }

    async fn import_workspace(&self, mut workspace: ProxyWorkspace) -> AppResult<ProxyWorkspace> {
        workspace.validate().map_err(AppError::from)?;
        remap_workspace_identity(&mut workspace)?;
        let mut state = self.state.write();
        state.workspaces.insert(workspace.id, workspace.clone());
        if state.selected.is_none() {
            state.selected = Some(workspace.id);
        }
        Ok(workspace)
    }

    async fn delete(
        &self,
        workspace_id: WorkspaceId,
        expected_revision: u64,
    ) -> AppResult<OperationResultViewModel> {
        let mut state = self.state.write();
        let current = Self::get_stored(&state, workspace_id)?;
        current
            .revision
            .verify(intercept_proxy_domain::Revision::new(expected_revision))
            .map_err(AppError::from)?;
        state.workspaces.remove(&workspace_id);
        if state.selected == Some(workspace_id) {
            state.selected = state.workspaces.keys().next().copied();
        }
        Ok(OperationResultViewModel {
            success: true,
            cancelled: false,
            message: "Workspace 已删除。".into(),
            ui_tone: UiTone::Positive,
            entity_id: Some(workspace_id.to_string()),
            revision: Some(expected_revision),
            requires_restart: false,
        })
    }
}

#[cfg(test)]
#[path = "workspaces_tests.rs"]
mod tests;
