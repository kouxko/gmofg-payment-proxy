//! Strict legacy Workspace migration shared by portable files and `SQLite` persistence.

use std::collections::BTreeSet;

use intercept_proxy_domain::{
    AndroidNetworkProfile, CertificateReference, FaultPreset, JsonPath, ListenerId, ProxyListener,
    ProxyListenerV2, ProxyWorkspace, ResponseAssertion, Revision, Rule,
    SocketDocumentRuleDefinition, WorkspaceId,
};
use serde::Deserialize;
use serde_json::Value;
use specta::Type;
use uuid::Uuid;

use crate::{AppError, AppResult};

pub const WORKSPACE_PERSISTENCE_VERSION: u16 = 5;
const MAX_LEGACY_METADATA_EXTRACTORS: usize = 64;

#[derive(Clone, Copy, Debug, Eq, PartialEq, serde::Serialize, Type)]
#[serde(rename_all = "snake_case")]
pub enum MigrationSourceKind {
    WorkspaceDocument,
    ApplicationConfigurationDocument,
    WorkspacePersistence,
}

#[derive(Clone, Debug, Eq, PartialEq, serde::Serialize, Type)]
pub struct MigrationReport {
    pub removed_metadata_extractors: usize,
    pub source_kind: MigrationSourceKind,
    pub source_version: u16,
}

impl MigrationReport {
    #[must_use]
    pub const fn unchanged(source_kind: MigrationSourceKind, source_version: u16) -> Self {
        Self {
            removed_metadata_extractors: 0,
            source_kind,
            source_version,
        }
    }

    #[must_use]
    pub fn warning_message(&self) -> Option<String> {
        (self.removed_metadata_extractors > 0).then(|| {
            format!(
                "旧配置中的 {} 个元数据提取器已移除；该功能不再支持且不会恢复。",
                self.removed_metadata_extractors
            )
        })
    }
}

#[derive(Clone, Debug, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
enum LegacyMetadataExtractorSource {
    Header { name: String },
    JsonPath { path: String },
    BodyText,
    FixedValue { value: String },
}

#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct LegacyMetadataExtractor {
    id: Uuid,
    name: String,
    listener_ids: Vec<ListenerId>,
    source: LegacyMetadataExtractorSource,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct LegacyWorkspaceV2 {
    id: WorkspaceId,
    name: String,
    revision: Revision,
    listeners: Vec<ProxyListenerV2>,
    metadata_extractors: Vec<LegacyMetadataExtractor>,
    response_assertions: Vec<ResponseAssertion>,
    rules: Vec<Rule>,
    fault_presets: Vec<FaultPreset>,
    certificate_references: Vec<CertificateReference>,
    #[serde(default)]
    android_network_profiles: Vec<AndroidNetworkProfile>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct LegacyWorkspaceV3 {
    id: WorkspaceId,
    name: String,
    revision: Revision,
    listeners: Vec<ProxyListener>,
    metadata_extractors: Vec<LegacyMetadataExtractor>,
    response_assertions: Vec<ResponseAssertion>,
    rules: Vec<Rule>,
    fault_presets: Vec<FaultPreset>,
    certificate_references: Vec<CertificateReference>,
    #[serde(default)]
    android_network_profiles: Vec<AndroidNetworkProfile>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct LegacyWorkspaceV4 {
    id: WorkspaceId,
    name: String,
    revision: Revision,
    listeners: Vec<ProxyListener>,
    metadata_extractors: Vec<LegacyMetadataExtractor>,
    response_assertions: Vec<ResponseAssertion>,
    rules: Vec<Rule>,
    socket_rules: Vec<SocketDocumentRuleDefinition>,
    socket_rule_created_order_high_water: u64,
    fault_presets: Vec<FaultPreset>,
    certificate_references: Vec<CertificateReference>,
    #[serde(default)]
    android_network_profiles: Vec<AndroidNetworkProfile>,
}

pub fn migrate_workspace_value(
    value: Value,
    source_kind: MigrationSourceKind,
    source_version: u16,
) -> AppResult<(ProxyWorkspace, MigrationReport)> {
    let (workspace, extractors) = match source_version {
        2 => from_v2(decode(value)?),
        3 => from_v3(decode(value)?),
        4 => from_v4(decode(value)?),
        WORKSPACE_PERSISTENCE_VERSION => {
            let workspace: ProxyWorkspace = decode(value)?;
            workspace.validate().map_err(AppError::from)?;
            return Ok((
                workspace,
                MigrationReport::unchanged(source_kind, source_version),
            ));
        }
        other => {
            return Err(AppError::new(
                "WORKSPACE_VERSION_UNSUPPORTED",
                format!("Workspace 版本 {other} 不受支持。"),
            ));
        }
    };
    validate_legacy_extractors(&extractors, &workspace)?;
    workspace.validate().map_err(AppError::from)?;
    Ok((
        workspace,
        MigrationReport {
            removed_metadata_extractors: extractors.len(),
            source_kind,
            source_version,
        },
    ))
}

fn from_v2(value: LegacyWorkspaceV2) -> (ProxyWorkspace, Vec<LegacyMetadataExtractor>) {
    let extractors = value.metadata_extractors;
    (
        ProxyWorkspace {
            id: value.id,
            name: value.name,
            revision: value.revision,
            listeners: value.listeners.into_iter().map(Into::into).collect(),
            response_assertions: value.response_assertions,
            rules: value.rules,
            socket_rules: Vec::new(),
            socket_rule_created_order_high_water: 0,
            fault_presets: value.fault_presets,
            certificate_references: value.certificate_references,
            android_network_profiles: value.android_network_profiles,
        },
        extractors,
    )
}

fn from_v3(value: LegacyWorkspaceV3) -> (ProxyWorkspace, Vec<LegacyMetadataExtractor>) {
    let extractors = value.metadata_extractors;
    (
        ProxyWorkspace {
            id: value.id,
            name: value.name,
            revision: value.revision,
            listeners: value.listeners,
            response_assertions: value.response_assertions,
            rules: value.rules,
            socket_rules: Vec::new(),
            socket_rule_created_order_high_water: 0,
            fault_presets: value.fault_presets,
            certificate_references: value.certificate_references,
            android_network_profiles: value.android_network_profiles,
        },
        extractors,
    )
}

fn from_v4(value: LegacyWorkspaceV4) -> (ProxyWorkspace, Vec<LegacyMetadataExtractor>) {
    let extractors = value.metadata_extractors;
    (
        ProxyWorkspace {
            id: value.id,
            name: value.name,
            revision: value.revision,
            listeners: value.listeners,
            response_assertions: value.response_assertions,
            rules: value.rules,
            socket_rules: value.socket_rules,
            socket_rule_created_order_high_water: value.socket_rule_created_order_high_water,
            fault_presets: value.fault_presets,
            certificate_references: value.certificate_references,
            android_network_profiles: value.android_network_profiles,
        },
        extractors,
    )
}

fn decode<T: for<'de> Deserialize<'de>>(value: Value) -> AppResult<T> {
    serde_json::from_value(value)
        .map_err(|error| AppError::new("IMPORT_FAILED", format!("Workspace 结构无效：{error}")))
}

fn validate_legacy_extractors(
    extractors: &[LegacyMetadataExtractor],
    workspace: &ProxyWorkspace,
) -> AppResult<()> {
    if extractors.len() > MAX_LEGACY_METADATA_EXTRACTORS {
        return invalid_legacy("元数据提取器数量不能超过 64 个");
    }
    let listener_ids = workspace
        .listeners
        .iter()
        .map(|listener| listener.id)
        .collect::<BTreeSet<_>>();
    let mut ids = BTreeSet::new();
    for extractor in extractors {
        if !ids.insert(extractor.id) {
            return invalid_legacy("元数据提取器 ID 不能重复");
        }
        if extractor.name.trim().is_empty() {
            return invalid_legacy("元数据提取器名称不能为空");
        }
        if extractor
            .listener_ids
            .iter()
            .any(|id| !listener_ids.contains(id))
        {
            return invalid_legacy("元数据提取器引用的 Listener 不存在");
        }
        match &extractor.source {
            LegacyMetadataExtractorSource::Header { name } if name.trim().is_empty() => {
                return invalid_legacy("元数据提取器 Header 名称不能为空");
            }
            LegacyMetadataExtractorSource::JsonPath { path } => {
                JsonPath::parse(path)
                    .map_err(|_| AppError::new("IMPORT_FAILED", "元数据提取器 JSONPath 无效"))?;
            }
            LegacyMetadataExtractorSource::FixedValue { value } => {
                let _ = value.len();
            }
            LegacyMetadataExtractorSource::BodyText
            | LegacyMetadataExtractorSource::Header { .. } => {}
        }
    }
    Ok(())
}

fn invalid_legacy(message: &str) -> AppResult<()> {
    Err(AppError::new("IMPORT_FAILED", message))
}
