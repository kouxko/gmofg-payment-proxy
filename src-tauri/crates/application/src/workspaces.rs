//! Workspace 的 UI 无关内存仓储。
//!
//! 该实现既是无界面单元测试夹具，也是 `SQLite` 适配器的行为参考：所有更新执行乐观锁，
//! 所有导入先拒绝秘密字段并经过领域校验，所有导出只序列化安全领域模型。Tauri/Dialog
//! 仅负责取得或保存文件字节，不得在展示层重新实现这些规则。

use std::collections::BTreeMap;

use async_trait::async_trait;
use intercept_proxy_domain::{
    CertificateReferenceId, ChannelId, DownstreamClientAuthentication, FaultPresetId, ListenerId,
    MetadataExtractorId, ResponseAssertionId, Revision, RuleId,
};
use parking_lot::RwLock;
use uuid::Uuid;

use crate::{
    AppError, AppResult, OperationResultViewModel, ProxyWorkspace, UiTone, WorkspaceDocumentPort,
    WorkspaceId, WorkspaceRepositoryPort, WorkspaceSummaryViewModel, WorkspaceValidationViewModel,
    parse_workspace_document, serialize_workspace_document,
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
        listener.mitm.root_ca = listener
            .mitm
            .root_ca
            .map(|id| mapped(&certificate_ids, id, "MITM Root CA"))
            .transpose()?;
        if let Some(fixed_server) = &mut listener.fixed_server {
            fixed_server.upstream_tls.server_trust = fixed_server
                .upstream_tls
                .server_trust
                .map(|id| mapped(&certificate_ids, id, "upstream trust"))
                .transpose()?;
            fixed_server.upstream_tls.client_identity = fixed_server
                .upstream_tls
                .client_identity
                .map(|id| mapped(&certificate_ids, id, "upstream identity"))
                .transpose()?;
        }
        listener.downstream_tls.server_identity = listener
            .downstream_tls
            .server_identity
            .map(|id| mapped(&certificate_ids, id, "server identity"))
            .transpose()?;
        listener.downstream_tls.client_authentication = match listener
            .downstream_tls
            .client_authentication
        {
            DownstreamClientAuthentication::Disabled => DownstreamClientAuthentication::Disabled,
            DownstreamClientAuthentication::Optional { trust } => {
                DownstreamClientAuthentication::Optional {
                    trust: mapped(&certificate_ids, trust, "client trust")?,
                }
            }
            DownstreamClientAuthentication::Required { trust } => {
                DownstreamClientAuthentication::Required {
                    trust: mapped(&certificate_ids, trust, "client trust")?,
                }
            }
        };
    }

    for extractor in &mut workspace.metadata_extractors {
        extractor.id = MetadataExtractorId::new();
        remap_listener_references(&mut extractor.listener_ids, &listener_ids)?;
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

    async fn import_document(&self, document: Vec<u8>) -> AppResult<ProxyWorkspace> {
        let mut workspace = parse_workspace_document(&document)?;
        remap_workspace_identity(&mut workspace)?;
        let mut state = self.state.write();
        state.workspaces.insert(workspace.id, workspace.clone());
        if state.selected.is_none() {
            state.selected = Some(workspace.id);
        }
        Ok(workspace)
    }

    async fn export_document(&self, workspace_id: WorkspaceId) -> AppResult<Vec<u8>> {
        let workspace = Self::get_stored(&self.state.read(), workspace_id)?;
        serialize_workspace_document(&workspace)
    }
}

#[derive(Debug, Default)]
/// 无界面测试使用的文档端口；不会访问真实文件系统。
pub struct InMemoryWorkspaceDocumentStore {
    next_import: RwLock<Option<Vec<u8>>>,
    last_export: RwLock<Option<(String, Vec<u8>)>>,
}

impl InMemoryWorkspaceDocumentStore {
    pub fn set_next_import(&self, document: Vec<u8>) {
        *self.next_import.write() = Some(document);
    }

    pub fn take_last_export(&self) -> Option<(String, Vec<u8>)> {
        self.last_export.write().take()
    }
}

#[async_trait]
impl WorkspaceDocumentPort for InMemoryWorkspaceDocumentStore {
    async fn pick_import_document(&self) -> AppResult<Option<Vec<u8>>> {
        Ok(self.next_import.write().take())
    }

    async fn save_export_document(
        &self,
        suggested_file_name: String,
        document: Vec<u8>,
    ) -> AppResult<bool> {
        *self.last_export.write() = Some((suggested_file_name, document));
        Ok(true)
    }

    async fn pick_import_application_configuration(&self) -> AppResult<Option<Vec<u8>>> {
        Ok(self.next_import.write().take())
    }

    async fn save_export_application_configuration(
        &self,
        suggested_file_name: String,
        document: Vec<u8>,
    ) -> AppResult<bool> {
        *self.last_export.write() = Some((suggested_file_name, document));
        Ok(true)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::Value;

    #[tokio::test]
    async fn default_store_contains_selected_safe_workspace() {
        let store = InMemoryWorkspaceStore::default();
        let list = store.list().await.unwrap();
        assert_eq!(list.len(), 1);
        assert!(list[0].selected);
        let workspace = store.get(list[0].id).await.unwrap();
        assert!(workspace.validate().is_ok());
        assert_eq!(workspace.listeners.len(), 1);
    }

    #[tokio::test]
    async fn create_validate_save_copy_select_and_delete_share_one_revision_contract() {
        let store = InMemoryWorkspaceStore::new_empty();
        let created = store.create("Lab".into()).await.unwrap();
        let validation = store.validate(created.clone()).await.unwrap();
        assert!(validation.valid);

        let mut edited = created.clone();
        edited.name = "Lab Updated".into();
        let saved = store.save(edited).await.unwrap();
        assert_eq!(saved.revision.get(), 2);
        assert!(store.save(created).await.is_err(), "stale save must fail");

        let copied = store.copy(saved.id).await.unwrap();
        assert_ne!(copied.id, saved.id);
        assert_eq!(copied.revision, intercept_proxy_domain::Revision::INITIAL);
        assert!(store.select(copied.id).await.unwrap().selected);
        assert_eq!(store.list().await.unwrap().len(), 2);

        store
            .delete(copied.id, copied.revision.get())
            .await
            .unwrap();
        assert!(store.get(copied.id).await.is_err());
        assert_eq!(store.list().await.unwrap().len(), 1);
    }

    #[tokio::test]
    async fn export_round_trip_contains_references_but_no_secret_values() {
        let source = InMemoryWorkspaceStore::default();
        let id = source.list().await.unwrap()[0].id;
        let document = source.export_document(id).await.unwrap();
        let text = String::from_utf8(document.clone())
            .unwrap()
            .to_ascii_lowercase();
        for forbidden in ["password", "private_key", "pkcs12", "secret_value"] {
            assert!(!text.contains(forbidden), "{forbidden} leaked into export");
        }

        let destination = InMemoryWorkspaceStore::new_empty();
        let imported = destination.import_document(document).await.unwrap();
        assert!(imported.validate().is_ok());
        assert_eq!(destination.list().await.unwrap().len(), 1);
    }

    #[tokio::test]
    async fn export_rejects_unmanaged_certificate_references() {
        let mut workspace = ProxyWorkspace::default();
        workspace
            .certificate_references
            .push(intercept_proxy_domain::CertificateReference {
                id: CertificateReferenceId::new(),
                label: "旧文件引用".into(),
                kind: intercept_proxy_domain::CertificateReferenceKind::UpstreamServerTrust,
                reference: "file:/tmp/server-ca.pem".into(),
            });

        let error = serialize_workspace_document(&workspace)
            .expect_err("portable export must reject unmanaged references");

        assert_eq!(error.view_model.code, "EXPORT_FAILED");
    }

    #[tokio::test]
    async fn import_rejects_unknown_sensitive_fields_before_serde_discards_them() {
        for forbidden in ["password", "basic_auth_password", "pkcs12_password"] {
            let workspace = ProxyWorkspace::default();
            let mut value = serde_json::to_value(workspace).unwrap();
            value.as_object_mut().unwrap().insert(
                forbidden.into(),
                Value::String("must-not-enter-core".into()),
            );
            let document = serde_json::to_vec(&value).unwrap();
            let store = InMemoryWorkspaceStore::new_empty();
            let error = store.import_document(document).await.unwrap_err();
            assert_eq!(error.view_model.code, "IMPORT_FAILED");
            assert!(store.list().await.unwrap().is_empty());
        }
    }

    #[tokio::test]
    async fn import_rejects_unmanaged_certificate_references() {
        let mut workspace = ProxyWorkspace::default();
        workspace
            .certificate_references
            .push(intercept_proxy_domain::CertificateReference {
                id: CertificateReferenceId::new(),
                label: "外部证书".into(),
                kind: intercept_proxy_domain::CertificateReferenceKind::UpstreamServerTrust,
                reference: "pkcs12:/tmp/client.p12?password_env=P12_PASSWORD".into(),
            });
        let store = InMemoryWorkspaceStore::new_empty();

        let error = store
            .import_document(serde_json::to_vec(&workspace).unwrap())
            .await
            .expect_err("unmanaged reference rejected");

        assert_eq!(
            error.view_model.code,
            "LISTENER_CERTIFICATE_REFERENCE_UNTRUSTED"
        );
        assert!(store.list().await.unwrap().is_empty());
    }

    #[tokio::test]
    async fn importing_same_document_twice_never_overwrites_existing_workspace() {
        let workspace = ProxyWorkspace::default();
        let document = serde_json::to_vec(&workspace).unwrap();
        let store = InMemoryWorkspaceStore::new_empty();
        let first = store.import_document(document.clone()).await.unwrap();
        let second = store.import_document(document).await.unwrap();
        assert_ne!(first.id, workspace.id);
        assert_ne!(second.id, first.id);
        assert_eq!(store.list().await.unwrap().len(), 2);
    }
}
