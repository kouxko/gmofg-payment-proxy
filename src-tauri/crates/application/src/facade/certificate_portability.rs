//! 单文件配置与本机证书托管之间的转换。
//!
//! 导出时读取受保护存储并把材料放进用户选择的唯一 JSON 文件；导入时先校验材料，
//! 再写入当前系统的受保护存储并替换 Workspace 引用。任何后续持久化失败都会清理本次
//! 已恢复的材料，避免留下不可达的秘密记录。

use std::collections::BTreeMap;

use super::Application;
use crate::{
    AppError, AppResult, CertificateReference, CertificateReferenceId, CertificateReferenceKind,
    PortableCertificateMaterial, ProxyWorkspace, validate_certificate_materials,
};

impl Application {
    pub(super) async fn export_certificate_materials(
        &self,
        workspaces: &[ProxyWorkspace],
    ) -> AppResult<Vec<PortableCertificateMaterial>> {
        let mut materials = Vec::new();
        let mut seen = BTreeMap::<CertificateReferenceId, CertificateReference>::new();
        for reference in workspaces
            .iter()
            .flat_map(|workspace| workspace.certificate_references.iter())
        {
            if reference.kind == CertificateReferenceKind::MitmRootCa {
                continue;
            }
            if let Some(existing) = seen.get(&reference.id) {
                if existing != reference {
                    return Err(crate::AppError::new(
                        "PORTABLE_CERTIFICATE_INVALID",
                        "多个 Workspace 使用了冲突的证书引用 ID。",
                    )
                    .entity(reference.id.to_string()));
                }
                continue;
            }
            seen.insert(reference.id, reference.clone());
            materials.push(
                self.listener_certificates
                    .export_portable(reference.clone())
                    .await?,
            );
        }
        validate_certificate_materials(workspaces, &materials)?;
        Ok(materials)
    }

    pub(super) async fn restore_certificate_materials(
        &self,
        workspaces: &mut [ProxyWorkspace],
        materials: Vec<PortableCertificateMaterial>,
    ) -> AppResult<Vec<CertificateReference>> {
        validate_certificate_materials(workspaces, &materials)?;
        let mut restored = Vec::with_capacity(materials.len());
        for material in materials {
            match self.listener_certificates.restore_portable(material).await {
                Ok(reference) => restored.push(reference),
                Err(error) => {
                    return Err(match self.rollback_restored_certificates(&restored).await {
                        Ok(()) => error,
                        Err(cleanup) => certificate_operation_cleanup_error(error, cleanup),
                    });
                }
            }
        }

        let by_id = restored
            .iter()
            .cloned()
            .map(|reference| (reference.id, reference))
            .collect::<BTreeMap<_, _>>();
        for reference in workspaces
            .iter_mut()
            .flat_map(|workspace| workspace.certificate_references.iter_mut())
        {
            if let Some(restored_reference) = by_id.get(&reference.id) {
                *reference = restored_reference.clone();
            }
        }
        Ok(restored)
    }

    pub(super) async fn rollback_restored_certificates(
        &self,
        restored: &[CertificateReference],
    ) -> AppResult<()> {
        let mut failures = Vec::new();
        for reference in restored.iter().rev() {
            if let Err(error) = self.listener_certificates.discard(reference.clone()).await {
                failures.push(format!("{}：{}", reference.label, error.view_model.message));
            }
        }
        if failures.is_empty() {
            Ok(())
        } else {
            Err(AppError::new(
                "CERTIFICATE_ROLLBACK_FAILED",
                format!("恢复失败后的证书清理未全部完成：{}", failures.join("；")),
            )
            .retryable("请重新导入配置；若仍失败，请重新启动应用后检查受保护证书存储。"))
        }
    }
}

pub(super) fn certificate_operation_cleanup_error(
    mut operation: AppError,
    cleanup: AppError,
) -> AppError {
    operation.view_model.message = format!(
        "{}；同时证书回滚清理失败：{}",
        operation.view_model.message, cleanup.view_model.message
    );
    operation.view_model.retryable = true;
    operation.view_model.suggested_action = cleanup.view_model.suggested_action;
    operation
}
