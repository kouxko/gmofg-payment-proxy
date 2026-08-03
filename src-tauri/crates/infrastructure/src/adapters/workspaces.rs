//! 通用 Workspace 的 `SQLite` 仓储适配器。
//!
//! 这里负责持久化、乐观锁和安全导入导出；它只保存领域模型及系统秘密引用，绝不把
//! PKCS#12 密码、私钥或代理认证明文写入 Workspace JSON。文件选择由独立平台端口
//! 完成，因此同一仓储可被 Tauri、未来 CLI/TUI 和无界面测试复用。

use std::{fs, sync::Arc};

use async_trait::async_trait;
use chrono::Utc;
use intercept_proxy_application::{
    AppError, AppResult, ApplicationConfigurationDocument, ApplicationConfigurationStorePort,
    OperationResultViewModel, ProxyWorkspace, UiTone, WorkspaceDocumentPort, WorkspaceId,
    WorkspaceRepositoryPort, WorkspaceSummaryViewModel, WorkspaceValidationViewModel,
    remap_workspace_identity,
};
use serde_json::Value;

use crate::{AtomicFileExporter, SqliteStore, WorkspaceRecord};

use super::{
    NativeFileDialog,
    common::{app_error, infra},
    settings::serialize_settings,
};

const MAX_WORKSPACE_DOCUMENT_BYTES: usize = 8 * 1024 * 1024;

#[derive(Debug)]
pub struct WorkspaceRepositoryAdapter {
    store: Arc<SqliteStore>,
}

/// 原生 Dialog/文件系统到应用文档端口的薄适配器。
#[derive(Debug)]
pub struct WorkspaceDocumentAdapter {
    dialog: Arc<dyn NativeFileDialog>,
    exporter: AtomicFileExporter,
}

impl WorkspaceDocumentAdapter {
    #[must_use]
    pub fn new(dialog: Arc<dyn NativeFileDialog>) -> Self {
        Self {
            dialog,
            exporter: AtomicFileExporter,
        }
    }
}

#[async_trait]
impl WorkspaceDocumentPort for WorkspaceDocumentAdapter {
    async fn pick_import_document(&self) -> AppResult<Option<Vec<u8>>> {
        let Some(path) = self.dialog.choose_open_file("intercept_workspace")? else {
            return Ok(None);
        };
        let metadata = fs::metadata(&path).map_err(|error| {
            AppError::new(
                "IMPORT_FAILED",
                format!("无法读取 Workspace 文件信息：{error}"),
            )
        })?;
        if metadata.len() > MAX_WORKSPACE_DOCUMENT_BYTES as u64 {
            return Err(AppError::new(
                "IMPORT_FAILED",
                "Workspace 文档超过 8 MiB 安全上限。",
            ));
        }
        fs::read(&path).map(Some).map_err(|error| {
            AppError::new("IMPORT_FAILED", format!("读取 Workspace 失败：{error}"))
        })
    }

    async fn save_export_document(
        &self,
        _suggested_file_name: String,
        document: Vec<u8>,
    ) -> AppResult<bool> {
        let Some(selection) = self.dialog.choose_save_file("intercept_workspace")? else {
            return Ok(false);
        };
        infra(
            self.exporter
                .write(&selection.path, &document, selection.overwrite_confirmed),
        )?;
        Ok(true)
    }

    async fn pick_import_application_configuration(&self) -> AppResult<Option<Vec<u8>>> {
        let Some(path) = self.dialog.choose_open_file("intercept_configuration")? else {
            return Ok(None);
        };
        let metadata = fs::metadata(&path).map_err(|error| {
            AppError::new(
                "IMPORT_FAILED",
                format!("无法读取完整配置文件信息：{error}"),
            )
        })?;
        if metadata.len() > intercept_proxy_application::MAX_APPLICATION_CONFIGURATION_BYTES as u64
        {
            return Err(AppError::new(
                "IMPORT_FAILED",
                "完整配置文档超过 32 MiB 安全上限。",
            ));
        }
        fs::read(&path)
            .map(Some)
            .map_err(|error| AppError::new("IMPORT_FAILED", format!("读取完整配置失败：{error}")))
    }

    async fn save_export_application_configuration(
        &self,
        _suggested_file_name: String,
        document: Vec<u8>,
    ) -> AppResult<bool> {
        let Some(selection) = self.dialog.choose_save_file("intercept_configuration")? else {
            return Ok(false);
        };
        infra(
            self.exporter
                .write(&selection.path, &document, selection.overwrite_confirmed),
        )?;
        Ok(true)
    }
}

impl WorkspaceRepositoryAdapter {
    #[must_use]
    pub fn new(store: Arc<SqliteStore>) -> Self {
        Self { store }
    }

    fn snapshot(&self) -> AppResult<(Option<WorkspaceId>, Vec<ProxyWorkspace>)> {
        let snapshot = infra(self.store.load_workspaces())?;
        let selected = snapshot.selected_id.map(WorkspaceId::from_uuid);
        let workspaces = snapshot
            .records
            .into_iter()
            .map(|record| {
                let workspace =
                    serde_json::from_value::<ProxyWorkspace>(record.value).map_err(|error| {
                        AppError::new(
                            "PERSISTENCE_CORRUPT",
                            format!("Workspace 持久化文档无效：{error}"),
                        )
                    })?;
                if workspace.id.as_uuid() != record.id
                    || workspace.revision.get() != record.revision
                {
                    return Err(AppError::new(
                        "PERSISTENCE_CORRUPT",
                        "Workspace 索引与 JSON 中的 ID 或 revision 不一致。",
                    ));
                }
                workspace.validate().map_err(AppError::from)?;
                Ok(workspace)
            })
            .collect::<AppResult<Vec<_>>>()?;
        Ok((selected, workspaces))
    }

    fn get_stored(&self, workspace_id: WorkspaceId) -> AppResult<ProxyWorkspace> {
        self.snapshot()?
            .1
            .into_iter()
            .find(|workspace| workspace.id == workspace_id)
            .ok_or_else(|| {
                AppError::new("WORKSPACE_NOT_FOUND", "Workspace 不存在或已被删除。")
                    .entity(workspace_id.to_string())
            })
    }

    fn record(workspace: &ProxyWorkspace) -> AppResult<WorkspaceRecord> {
        Ok(WorkspaceRecord {
            id: workspace.id.as_uuid(),
            revision: workspace.revision.get(),
            value: serde_json::to_value(workspace).map_err(|error| {
                AppError::new(
                    "PERSISTENCE_FAILED",
                    format!("Workspace 序列化失败：{error}"),
                )
            })?,
            updated_at: Utc::now(),
        })
    }

    fn import_document(document: &[u8]) -> AppResult<ProxyWorkspace> {
        if document.len() > MAX_WORKSPACE_DOCUMENT_BYTES {
            return Err(AppError::new(
                "IMPORT_FAILED",
                "Workspace 文档超过 8 MiB 安全上限。",
            ));
        }
        let value = serde_json::from_slice::<Value>(document).map_err(|error| {
            AppError::new("IMPORT_FAILED", format!("Workspace JSON 无效：{error}"))
        })?;
        reject_sensitive_fields(&value, "$")?;
        let workspace = serde_json::from_value::<ProxyWorkspace>(value).map_err(|error| {
            AppError::new("IMPORT_FAILED", format!("Workspace 结构无效：{error}"))
        })?;
        workspace.validate().map_err(AppError::from)?;
        Ok(workspace)
    }

    fn export_document(workspace: &ProxyWorkspace) -> AppResult<Vec<u8>> {
        workspace.validate().map_err(AppError::from)?;
        let document = serde_json::to_vec_pretty(workspace).map_err(|error| {
            AppError::new("EXPORT_FAILED", format!("Workspace 序列化失败：{error}"))
        })?;
        let value = serde_json::from_slice::<Value>(&document).map_err(|error| {
            AppError::new("EXPORT_FAILED", format!("Workspace 导出自检失败：{error}"))
        })?;
        reject_sensitive_fields(&value, "$")
            .map_err(|_| AppError::new("EXPORT_FAILED", "Workspace 包含禁止导出的敏感字段。"))?;
        Ok(document)
    }
}

#[async_trait]
impl ApplicationConfigurationStorePort for WorkspaceRepositoryAdapter {
    async fn replace_all(&self, document: ApplicationConfigurationDocument) -> AppResult<()> {
        document.validate()?;
        let records = document
            .workspaces
            .iter()
            .map(Self::record)
            .collect::<AppResult<Vec<_>>>()?;
        let settings = serialize_settings(&document.settings.to_draft(None)).map_err(|error| {
            AppError::new(
                "APPLICATION_CONFIGURATION_INVALID",
                format!("完整配置中的 Settings 无法持久化：{error}"),
            )
        })?;
        infra(self.store.replace_application_configuration(
            document.selected_workspace_id.as_uuid(),
            &records,
            &settings,
        ))
    }
}

#[async_trait]
impl WorkspaceRepositoryPort for WorkspaceRepositoryAdapter {
    async fn list(&self) -> AppResult<Vec<WorkspaceSummaryViewModel>> {
        let (selected, workspaces) = self.snapshot()?;
        Ok(workspaces
            .iter()
            .map(|workspace| {
                WorkspaceSummaryViewModel::from_workspace(workspace, selected == Some(workspace.id))
            })
            .collect())
    }

    async fn get(&self, workspace_id: WorkspaceId) -> AppResult<ProxyWorkspace> {
        self.get_stored(workspace_id)
    }

    async fn create(&self, name: String) -> AppResult<ProxyWorkspace> {
        let workspace = ProxyWorkspace {
            name: name.trim().to_owned(),
            ..ProxyWorkspace::default()
        };
        workspace.validate().map_err(AppError::from)?;
        infra(self.store.insert_workspace(&Self::record(&workspace)?))?;
        Ok(workspace)
    }

    async fn copy(&self, workspace_id: WorkspaceId) -> AppResult<ProxyWorkspace> {
        let mut workspace = self.get_stored(workspace_id)?;
        remap_workspace_identity(&mut workspace)?;
        workspace.name = format!("{} Copy", workspace.name);
        workspace.validate().map_err(AppError::from)?;
        infra(self.store.insert_workspace(&Self::record(&workspace)?))?;
        Ok(workspace)
    }

    async fn select(&self, workspace_id: WorkspaceId) -> AppResult<WorkspaceSummaryViewModel> {
        let workspace = self.get_stored(workspace_id)?;
        infra(self.store.select_workspace(workspace_id.as_uuid()))?;
        Ok(WorkspaceSummaryViewModel::from_workspace(&workspace, true))
    }

    async fn validate(&self, workspace: ProxyWorkspace) -> AppResult<WorkspaceValidationViewModel> {
        Ok(WorkspaceValidationViewModel::validate(workspace))
    }

    async fn save(&self, mut workspace: ProxyWorkspace) -> AppResult<ProxyWorkspace> {
        workspace.validate().map_err(AppError::from)?;
        let current = self.get_stored(workspace.id)?;
        current
            .revision
            .verify(workspace.revision)
            .map_err(AppError::from)?;
        let expected_revision = current.revision.get();
        workspace.revision = current.revision.next();
        infra(
            self.store
                .compare_and_swap_workspace(expected_revision, &Self::record(&workspace)?),
        )?;
        Ok(workspace)
    }

    async fn delete(
        &self,
        workspace_id: WorkspaceId,
        expected_revision: u64,
    ) -> AppResult<OperationResultViewModel> {
        self.get_stored(workspace_id)?;
        self.store
            .delete_workspace(workspace_id.as_uuid(), expected_revision)
            .map_err(app_error)?;
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
        let mut workspace = Self::import_document(&document)?;
        remap_workspace_identity(&mut workspace)?;
        infra(self.store.insert_workspace(&Self::record(&workspace)?))?;
        Ok(workspace)
    }

    async fn export_document(&self, workspace_id: WorkspaceId) -> AppResult<Vec<u8>> {
        Self::export_document(&self.get_stored(workspace_id)?)
    }
}

fn reject_sensitive_fields(value: &Value, path: &str) -> AppResult<()> {
    match value {
        Value::Object(map) => {
            for (key, value) in map {
                let normalized = key.to_ascii_lowercase().replace('-', "_");
                if matches!(
                    normalized.as_str(),
                    "password"
                        | "password_bytes"
                        | "private_key"
                        | "private_key_pem"
                        | "private_key_der"
                        | "pkcs12"
                        | "p12"
                        | "secret_value"
                ) {
                    return Err(AppError::new(
                        "IMPORT_FAILED",
                        format!("Workspace 文档包含禁止的敏感字段：{path}.{key}"),
                    ));
                }
                reject_sensitive_fields(value, &format!("{path}.{key}"))?;
            }
        }
        Value::Array(values) => {
            for (index, value) in values.iter().enumerate() {
                reject_sensitive_fields(value, &format!("{path}[{index}]"))?;
            }
        }
        _ => {}
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use intercept_proxy_application::{
        APPLICATION_CONFIGURATION_FORMAT_VERSION, PortableSettings, SettingsDraft,
    };
    use intercept_proxy_domain::{
        AndroidNetworkProfile, AndroidProxyRoute, AndroidTargetApplication, BodyCodecKind,
        CertificateReference, CertificateReferenceId, CertificateReferenceKind, ChannelId,
        ConnectionFaultAction, DownstreamClientAuthentication, DownstreamTlsSettings, FaultPreset,
        FaultPresetId, FixedServerSettings, ListenerId, MatchCondition, MessageStage,
        MetadataExtractor, MetadataExtractorId, MetadataExtractorSource, ProxyListener,
        ResponseAssertion, ResponseAssertionId, ResponseAssertionKind, Revision as DomainRevision,
        Rule, RuleId, UpstreamTlsSettings, WeakNetworkProfile,
    };
    use std::collections::BTreeSet;

    #[tokio::test]
    async fn sqlite_store_round_trips_and_rejects_stale_workspace_writes() {
        let repository = WorkspaceRepositoryAdapter::new(Arc::new(
            SqliteStore::in_memory().expect("in-memory store"),
        ));
        let created = repository.create("Lab".into()).await.expect("create");
        assert!(repository.list().await.expect("list")[0].selected);

        let mut edited = created.clone();
        edited.name = "Updated".into();
        let saved = repository.save(edited).await.expect("save");
        assert_eq!(saved.revision, DomainRevision::new(2));
        assert!(repository.save(created).await.is_err());

        let document = repository.export_document(saved.id).await.expect("export");
        let imported = repository
            .import_document(document)
            .await
            .expect("import copy");
        assert_ne!(imported.id, saved.id);
        assert_eq!(repository.list().await.expect("list after import").len(), 2);
    }

    #[tokio::test]
    async fn workspace_import_rejects_secret_value_fields() {
        let repository = WorkspaceRepositoryAdapter::new(Arc::new(
            SqliteStore::in_memory().expect("in-memory store"),
        ));
        let mut value = serde_json::to_value(ProxyWorkspace::default()).expect("workspace value");
        value["password"] = Value::String("must not persist".into());
        let error = repository
            .import_document(serde_json::to_vec(&value).expect("document"))
            .await
            .expect_err("secret field rejected");
        assert_eq!(error.view_model.code, "IMPORT_FAILED");
    }

    #[tokio::test]
    async fn full_configuration_replaces_workspaces_selection_and_settings_together() {
        let store = Arc::new(SqliteStore::in_memory().expect("in-memory store"));
        let repository = WorkspaceRepositoryAdapter::new(store.clone());
        let first = ProxyWorkspace {
            name: "First".into(),
            ..ProxyWorkspace::default()
        };
        let second = ProxyWorkspace {
            name: "Second".into(),
            ..ProxyWorkspace::default()
        };
        let settings = SettingsDraft {
            max_sessions: 777,
            ..SettingsDraft::default()
        };
        let document = ApplicationConfigurationDocument {
            format_version: APPLICATION_CONFIGURATION_FORMAT_VERSION,
            selected_workspace_id: second.id,
            workspaces: vec![first.clone(), second.clone()],
            settings: PortableSettings::from(&settings),
        };

        repository
            .replace_all(document)
            .await
            .expect("atomic replace");

        let snapshot = store.load_workspaces().expect("workspaces");
        assert_eq!(snapshot.selected_id, Some(second.id.as_uuid()));
        assert_eq!(snapshot.records.len(), 2);
        let stored_settings = store.load_settings().expect("settings").expect("stored");
        assert_eq!(
            stored_settings.value["max_sessions"],
            serde_json::json!(777)
        );
    }

    #[test]
    fn copy_identity_remaps_nested_ids_and_references() {
        let mut workspace = referenced_workspace();
        let original = workspace.clone();

        remap_workspace_identity(&mut workspace).expect("remap");

        assert_ne!(workspace.id, original.id);
        assert_eq!(workspace.revision, DomainRevision::INITIAL);
        assert_ne!(workspace.listeners[0].id, original.listeners[0].id);
        assert_ne!(
            workspace.metadata_extractors[0].id,
            original.metadata_extractors[0].id
        );
        assert_ne!(
            workspace.response_assertions[0].id,
            original.response_assertions[0].id
        );
        assert_ne!(workspace.rules[0].id, original.rules[0].id);
        assert_ne!(workspace.fault_presets[0].id, original.fault_presets[0].id);
        assert_ne!(
            workspace.android_network_profiles[0].id,
            original.android_network_profiles[0].id
        );
        assert_ne!(
            workspace.certificate_references[0].id,
            original.certificate_references[0].id
        );

        let listener = &workspace.listeners[0];
        assert_eq!(listener.request_body_codec, BodyCodecKind::Utf8);
        assert_eq!(listener.response_body_codec, BodyCodecKind::ShiftJis);
        assert_eq!(
            listener.downstream_tls.server_identity,
            Some(workspace.certificate_references[0].id)
        );
        assert_eq!(
            workspace.rules[0].channel.as_ref().map(ChannelId::as_str),
            Some(listener.id.to_string().as_str())
        );
        assert_eq!(
            workspace.android_network_profiles[0].proxy_routes[0].listener_id,
            listener.id
        );
    }

    #[allow(clippy::too_many_lines)]
    fn referenced_workspace() -> ProxyWorkspace {
        let listener_id = ListenerId::new();
        let server_identity = CertificateReferenceId::new();
        let downstream_trust = CertificateReferenceId::new();
        let upstream_identity = CertificateReferenceId::new();
        let upstream_trust = CertificateReferenceId::new();
        let certificates = [
            (
                server_identity,
                CertificateReferenceKind::ReverseServerIdentity,
            ),
            (
                downstream_trust,
                CertificateReferenceKind::DownstreamClientTrust,
            ),
            (
                upstream_identity,
                CertificateReferenceKind::UpstreamClientIdentity,
            ),
            (
                upstream_trust,
                CertificateReferenceKind::UpstreamServerTrust,
            ),
        ]
        .into_iter()
        .enumerate()
        .map(|(index, (id, kind))| CertificateReference {
            id,
            label: format!("certificate-{index}"),
            kind,
            reference: format!("pem:/tmp/certificate-{index}.pem"),
        })
        .collect();
        let workspace = ProxyWorkspace {
            id: WorkspaceId::new(),
            name: "Referenced".into(),
            revision: DomainRevision::new(9),
            listeners: vec![ProxyListener {
                id: listener_id,
                name: "Reverse".into(),
                enabled: false,
                bind_address: "127.0.0.1".into(),
                port: 18443,
                downstream_tls: DownstreamTlsSettings {
                    enabled: true,
                    server_identity: Some(server_identity),
                    client_authentication: DownstreamClientAuthentication::Required {
                        trust: downstream_trust,
                    },
                },
                request_body_codec: BodyCodecKind::Utf8,
                response_body_codec: BodyCodecKind::ShiftJis,
                fixed_server: Some(FixedServerSettings {
                    upstream_url: "https://example.test".into(),
                    upstream_tls: UpstreamTlsSettings {
                        verify_hostname: true,
                        server_trust: Some(upstream_trust),
                        client_identity: Some(upstream_identity),
                    },
                }),
                ..ProxyListener::default()
            }],
            metadata_extractors: vec![MetadataExtractor {
                id: MetadataExtractorId::new(),
                name: "Extractor".into(),
                listener_ids: vec![listener_id],
                source: MetadataExtractorSource::BodyText,
            }],
            response_assertions: vec![ResponseAssertion {
                id: ResponseAssertionId::new(),
                name: "Assertion".into(),
                listener_ids: vec![listener_id],
                enabled: true,
                assertion: ResponseAssertionKind::HttpStatusEquals { expected: 200 },
            }],
            rules: vec![Rule {
                id: RuleId::new(),
                revision: DomainRevision::INITIAL,
                name: "Rule".into(),
                description: String::new(),
                enabled: true,
                priority: 1,
                created_order: 1,
                channel: Some(ChannelId::new(listener_id.to_string()).expect("channel")),
                stage: MessageStage::Request,
                conditions: Vec::<MatchCondition>::new(),
                actions: Vec::new(),
                one_shot: false,
                hit_count: 0,
                last_hit_at: None,
            }],
            fault_presets: vec![FaultPreset {
                id: FaultPresetId::new(),
                name: "Fault".into(),
                description: String::new(),
                connection_actions: vec![ConnectionFaultAction::Delay { milliseconds: 1 }],
                http_actions: Vec::new(),
            }],
            certificate_references: certificates,
            android_network_profiles: vec![AndroidNetworkProfile {
                id: "android-profile".into(),
                name: "Android Profile".into(),
                target_applications: vec![AndroidTargetApplication {
                    package_name: "com.example.client".into(),
                    signing_sha256: "AA".repeat(32),
                    uid: 10_001,
                    display_name: None,
                }],
                destination_targets: Vec::new(),
                proxy_routes: vec![AndroidProxyRoute {
                    destination: "example.test".into(),
                    ports: vec![443],
                    listener_id,
                }],
                confirmed_shared_uids: BTreeSet::new(),
                auto_resume_after_reboot: false,
                weak_network: WeakNetworkProfile::default(),
            }],
        };
        workspace.validate().expect("valid referenced workspace");
        workspace
    }
}
