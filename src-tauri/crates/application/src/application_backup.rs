//! Application backup ZIP v1 wire types.
//!
//! This module owns only the versioned `application.json` contract. ZIP I/O,
//! archive limits, preview and persistence belong to outer layers/stories.

use std::{cmp::Ordering, collections::BTreeSet, fmt};

use intercept_proxy_domain::{
    CertificateReferenceId, CertificateReferenceKind, ProtocolPackageRef, ProxyWorkspace,
    WorkspaceId,
};
use serde::{Deserialize, Serialize};

use crate::{AppError, AppResult, PortableSettings, reject_sensitive_configuration_fields};

pub const APPLICATION_BACKUP_FORMAT_VERSION: u16 = 1;
pub const MAX_APPLICATION_BACKUP_PATH_BYTES: usize = 512;
pub const MAX_APPLICATION_BACKUP_PATH_DEPTH: usize = 40;
pub const MAX_APPLICATION_BACKUP_PASSWORD_BYTES: usize = 16 * 1024;

/// A canonical, portable path relative to the root of an application backup.
#[derive(Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(transparent)]
pub struct PortableArchivePath(String);

impl PortableArchivePath {
    pub fn new(value: impl Into<String>) -> AppResult<Self> {
        let value = value.into();
        let segments_valid = value
            .split('/')
            .all(|segment| !segment.is_empty() && segment != "." && segment != "..");
        let characters_valid = value
            .chars()
            .all(|character| !character.is_control() && character != '\\' && character != ':');
        if value.is_empty()
            || value.len() > MAX_APPLICATION_BACKUP_PATH_BYTES
            || value.starts_with('/')
            || value.split('/').count() > MAX_APPLICATION_BACKUP_PATH_DEPTH
            || !segments_valid
            || !characters_valid
        {
            return Err(invalid_reference("备份文件引用必须是安全的规范相对路径。"));
        }
        Ok(Self(value))
    }

    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for PortableArchivePath {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.as_str())
    }
}

impl<'de> Deserialize<'de> for PortableArchivePath {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let value = String::deserialize(deserializer)?;
        Self::new(value).map_err(serde::de::Error::custom)
    }
}

/// One installed package and the exact package files stored in the ZIP.
#[derive(Clone, Debug, Eq, PartialEq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ApplicationBackupProtocolPackage {
    pub package: ProtocolPackageRef,
    pub enabled: bool,
    pub files: Vec<PortableArchivePath>,
}

/// One portable certificate/identity payload stored outside `application.json`.
#[derive(Clone, Eq, PartialEq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ApplicationBackupPortableMaterial {
    pub reference_id: CertificateReferenceId,
    pub label: String,
    pub kind: CertificateReferenceKind,
    pub path: PortableArchivePath,
    pub password: Option<String>,
}

impl fmt::Debug for ApplicationBackupPortableMaterial {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("ApplicationBackupPortableMaterial")
            .field("reference_id", &self.reference_id)
            .field("label", &self.label)
            .field("kind", &self.kind)
            .field("path", &self.path)
            .field("password_present", &self.password.is_some())
            .finish()
    }
}

/// Structured application configuration stored in backup ZIP v1.
#[derive(Clone, Debug, Eq, PartialEq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ApplicationBackupConfiguration {
    pub selected_workspace_id: WorkspaceId,
    pub workspaces: Vec<ProxyWorkspace>,
    pub settings: PortableSettings,
}

/// Strict `application.json` document for application backup ZIP v1.
#[derive(Clone, Eq, PartialEq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ApplicationBackupDocument {
    pub format_version: u16,
    /// Canonical structured application configuration. Binary package and
    /// certificate payloads are referenced by the fields below, never embedded.
    pub application: ApplicationBackupConfiguration,
    pub protocol_packages: Vec<ApplicationBackupProtocolPackage>,
    pub portable_materials: Vec<ApplicationBackupPortableMaterial>,
}

impl fmt::Debug for ApplicationBackupDocument {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("ApplicationBackupDocument")
            .field("format_version", &self.format_version)
            .field(
                "selected_workspace_id",
                &self.application.selected_workspace_id,
            )
            .field("workspace_count", &self.application.workspaces.len())
            .field("protocol_package_count", &self.protocol_packages.len())
            .field(
                "protocol_package_file_count",
                &self
                    .protocol_packages
                    .iter()
                    .map(|package| package.files.len())
                    .sum::<usize>(),
            )
            .field("portable_material_count", &self.portable_materials.len())
            .finish()
    }
}

impl ApplicationBackupDocument {
    pub fn validate(&self) -> AppResult<()> {
        if self.format_version != APPLICATION_BACKUP_FORMAT_VERSION {
            return Err(AppError::new(
                "APPLICATION_BACKUP_VERSION_UNSUPPORTED",
                format!(
                    "应用备份版本 {} 不受支持；当前仅支持版本 {}。",
                    self.format_version, APPLICATION_BACKUP_FORMAT_VERSION
                ),
            ));
        }
        let application = serde_json::to_value(&self.application).map_err(|_| {
            AppError::new(
                "APPLICATION_BACKUP_DOCUMENT_INVALID",
                "应用备份 application 字段无法转换为规范结构。",
            )
        })?;
        reject_removed_fields(&application)?;
        reject_sensitive_configuration_fields(&application, "$.application").map_err(|_| {
            AppError::new(
                "APPLICATION_BACKUP_DOCUMENT_INVALID",
                "应用备份 application 字段包含禁止的敏感或运行态字段。",
            )
        })?;
        self.validate_package_references()?;
        self.validate_material_references()
    }

    fn validate_package_references(&self) -> AppResult<()> {
        let mut previous_package: Option<&ProtocolPackageRef> = None;
        let mut all_paths = BTreeSet::new();
        for package in &self.protocol_packages {
            if previous_package.is_some_and(|previous| {
                compare_package_identity(previous, &package.package) != Ordering::Less
            }) {
                return Err(invalid_reference(
                    "协议包必须按 id 和版本严格升序排列且不能重复。",
                ));
            }
            previous_package = Some(&package.package);
            if package.files.is_empty() {
                return Err(invalid_reference("协议包至少必须引用一个文件。"));
            }
            let prefix = format!(
                "protocol-packages/{}/{}/",
                package.package.id.as_str(),
                package.package.version.as_str()
            );
            let mut previous_path: Option<&PortableArchivePath> = None;
            for path in &package.files {
                if !path.as_str().starts_with(&prefix)
                    || previous_path.is_some_and(|previous| previous >= path)
                    || !all_paths.insert(path.clone())
                {
                    return Err(invalid_reference(
                        "协议包文件引用必须位于精确身份目录并按路径严格升序且不能重复。",
                    ));
                }
                previous_path = Some(path);
            }
        }
        Ok(())
    }

    fn validate_material_references(&self) -> AppResult<()> {
        let mut previous_id = None;
        let mut paths = BTreeSet::new();
        for material in &self.portable_materials {
            if material.label.trim().is_empty()
                || !material.path.as_str().starts_with("portable-materials/")
                || material
                    .password
                    .as_ref()
                    .is_some_and(|password| password.len() > MAX_APPLICATION_BACKUP_PASSWORD_BYTES)
                || previous_id.is_some_and(|previous| previous >= material.reference_id)
                || !paths.insert(material.path.clone())
            {
                return Err(invalid_reference(
                    "可移植材料必须有名称、使用规范目录、按引用 ID 升序且路径唯一。",
                ));
            }
            previous_id = Some(material.reference_id);
        }
        Ok(())
    }

    #[must_use]
    pub fn referenced_paths(&self) -> BTreeSet<PortableArchivePath> {
        self.protocol_packages
            .iter()
            .flat_map(|package| package.files.iter().cloned())
            .chain(
                self.portable_materials
                    .iter()
                    .map(|material| material.path.clone()),
            )
            .collect()
    }
}

pub fn parse_application_backup_document(bytes: &[u8]) -> AppResult<ApplicationBackupDocument> {
    let document = serde_json::from_slice::<ApplicationBackupDocument>(bytes).map_err(|error| {
        AppError::new(
            "APPLICATION_BACKUP_DOCUMENT_INVALID",
            format!("应用备份 application.json 结构无效：{error}"),
        )
    })?;
    document.validate()?;
    Ok(document)
}

pub fn serialize_application_backup_document(
    document: &ApplicationBackupDocument,
) -> AppResult<Vec<u8>> {
    document.validate()?;
    serde_json::to_vec_pretty(document).map_err(|error| {
        AppError::new(
            "APPLICATION_BACKUP_DOCUMENT_INVALID",
            format!("应用备份 application.json 序列化失败：{error}"),
        )
    })
}

fn compare_package_identity(left: &ProtocolPackageRef, right: &ProtocolPackageRef) -> Ordering {
    left.id.as_str().cmp(right.id.as_str()).then_with(|| {
        left.version
            .semantic_cmp(&right.version)
            .then_with(|| left.version.as_str().cmp(right.version.as_str()))
    })
}

fn reject_removed_fields(value: &serde_json::Value) -> AppResult<()> {
    match value {
        serde_json::Value::Object(object) => {
            for (key, child) in object {
                if key == "metadata_extractors" {
                    return Err(AppError::new(
                        "APPLICATION_BACKUP_DOCUMENT_INVALID",
                        "应用备份 v1 禁止包含 metadata_extractors。",
                    ));
                }
                reject_removed_fields(child)?;
            }
        }
        serde_json::Value::Array(items) => {
            for child in items {
                reject_removed_fields(child)?;
            }
        }
        _ => {}
    }
    Ok(())
}

fn invalid_reference(message: impl Into<String>) -> AppError {
    AppError::new("APPLICATION_BACKUP_REFERENCE_INVALID", message)
}
