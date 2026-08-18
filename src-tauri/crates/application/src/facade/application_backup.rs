//! Consistent application backup snapshot and export use case.

use std::collections::BTreeMap;

use base64::{Engine as _, engine::general_purpose::STANDARD};

use super::Application;
use crate::{
    APPLICATION_BACKUP_FORMAT_VERSION, AppError, AppResult, ApplicationBackupConfiguration,
    ApplicationBackupDocument, ApplicationBackupExportOutcome, ApplicationBackupExportPort,
    ApplicationBackupExportSnapshot, ApplicationBackupPortableMaterial,
    ApplicationBackupProtocolPackage, PortableArchivePath, PortableSettings,
    retain_reachable_certificate_references, validate_configuration_package_references,
};

impl Application {
    /// Capture a complete immutable application snapshot, then write it after
    /// releasing the application mutation gate.
    pub async fn application_backup_export(
        &self,
        destination: &dyn ApplicationBackupExportPort,
    ) -> AppResult<ApplicationBackupExportOutcome> {
        let snapshot = {
            let _gate = self.mutation_gate.lock().await;
            self.application_backup_snapshot().await?
        };
        destination.write(snapshot).await
    }

    async fn application_backup_snapshot(&self) -> AppResult<ApplicationBackupExportSnapshot> {
        let summaries = self.workspaces.list().await?;
        let selected_workspace_id = summaries
            .iter()
            .find(|summary| summary.selected)
            .map(|summary| summary.id)
            .ok_or_else(|| AppError::new("WORKSPACE_NOT_SELECTED", "请先选择一个 Workspace。"))?;
        let mut workspaces = Vec::with_capacity(summaries.len());
        for summary in summaries {
            let mut workspace = self.workspaces.get(summary.id).await?;
            retain_reachable_certificate_references(&mut workspace);
            workspaces.push(workspace);
        }

        let settings = self.settings.get().await?;
        let certificate_materials = self.export_certificate_materials(&workspaces).await?;
        let protocol_packages = self
            .protocol_package_portability
            .export_application_packages()
            .await?;
        validate_configuration_package_references(&workspaces, &protocol_packages, true)?;
        let mut files = BTreeMap::new();
        let package_references = protocol_packages
            .into_iter()
            .map(|package| package_into_backup(package, &mut files))
            .collect::<AppResult<Vec<_>>>()?;
        let mut material_references = certificate_materials
            .into_iter()
            .map(|material| material_into_backup(material, &mut files))
            .collect::<AppResult<Vec<_>>>()?;
        material_references.sort_by_key(|material| material.reference_id);

        let document = ApplicationBackupDocument {
            format_version: APPLICATION_BACKUP_FORMAT_VERSION,
            application: ApplicationBackupConfiguration {
                selected_workspace_id,
                workspaces,
                settings: PortableSettings::from(&settings.stored),
            },
            protocol_packages: package_references,
            portable_materials: material_references,
        };
        document.validate()?;
        if document.referenced_paths() != files.keys().cloned().collect() {
            return Err(AppError::new(
                "APPLICATION_BACKUP_SNAPSHOT_INVALID",
                "应用备份文件引用与冻结快照不一致。",
            ));
        }
        Ok(ApplicationBackupExportSnapshot { document, files })
    }
}

fn package_into_backup(
    package: crate::PortableApplicationProtocolPackage,
    files: &mut BTreeMap<PortableArchivePath, Vec<u8>>,
) -> AppResult<ApplicationBackupProtocolPackage> {
    let prefix = format!(
        "protocol-packages/{}/{}/",
        package.package.id.as_str(),
        package.package.version.as_str()
    );
    let mut references = Vec::with_capacity(package.files.len());
    for file in package.files {
        let path = PortableArchivePath::new(format!("{prefix}{}", file.path))?;
        let bytes = STANDARD.decode(&file.contents_base64).map_err(|_| {
            AppError::new(
                "APPLICATION_BACKUP_SNAPSHOT_INVALID",
                "协议包文件内容无法恢复为规范字节。",
            )
        })?;
        insert_unique(files, path.clone(), bytes)?;
        references.push(path);
    }
    Ok(ApplicationBackupProtocolPackage {
        package: package.package,
        enabled: package.enabled,
        files: references,
    })
}

fn material_into_backup(
    material: crate::PortableCertificateMaterial,
    files: &mut BTreeMap<PortableArchivePath, Vec<u8>>,
) -> AppResult<ApplicationBackupPortableMaterial> {
    material.validate_shape()?;
    let bytes = STANDARD.decode(&material.material_base64).map_err(|_| {
        AppError::new(
            "APPLICATION_BACKUP_SNAPSHOT_INVALID",
            "可移植证书材料无法恢复为规范字节。",
        )
    })?;
    let path = PortableArchivePath::new(format!(
        "portable-materials/{}.material",
        material.reference_id
    ))?;
    insert_unique(files, path.clone(), bytes)?;
    Ok(ApplicationBackupPortableMaterial {
        reference_id: material.reference_id,
        label: material.label,
        kind: material.kind,
        path,
        password: material.password,
    })
}

fn insert_unique(
    files: &mut BTreeMap<PortableArchivePath, Vec<u8>>,
    path: PortableArchivePath,
    bytes: Vec<u8>,
) -> AppResult<()> {
    if files.insert(path, bytes).is_some() {
        return Err(AppError::new(
            "APPLICATION_BACKUP_SNAPSHOT_INVALID",
            "应用备份包含重复文件引用。",
        ));
    }
    Ok(())
}
