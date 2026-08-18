//! Listener 上游 TLS 材料导入用例。

use std::collections::BTreeMap;

use super::Application;
use crate::{
    AppError, AppResult, CertificateReference, ListenerCertificateDetailViewModel,
    ListenerCertificateImportViewModel, OperationResultViewModel, WorkspaceId,
};

impl Application {
    pub async fn listener_import_downstream_server_identity(
        &self,
        label: String,
        password: String,
    ) -> AppResult<Option<ListenerCertificateImportViewModel>> {
        self.listener_certificates
            .import_downstream_server_identity(require_label(&label)?, password)
            .await
    }

    pub async fn listener_import_downstream_client_trust(
        &self,
        label: String,
    ) -> AppResult<Option<ListenerCertificateImportViewModel>> {
        self.listener_certificates
            .import_downstream_client_trust(require_label(&label)?)
            .await
    }

    pub async fn listener_import_upstream_client_identity(
        &self,
        label: String,
        password: String,
    ) -> AppResult<Option<ListenerCertificateImportViewModel>> {
        let label = require_label(&label)?;
        self.listener_certificates
            .import_upstream_client_identity(label, password)
            .await
    }

    pub async fn listener_import_upstream_server_trust(
        &self,
        label: String,
    ) -> AppResult<Option<ListenerCertificateImportViewModel>> {
        self.listener_certificates
            .import_upstream_server_trust(require_label(&label)?)
            .await
    }

    /// 解析当前 Workspace 中的全部证书安全引用。
    ///
    /// 调用者只提供 Workspace ID，不能从前端构造任意文件路径。单个引用损坏时仍返回
    /// 其他证书详情，并把稳定中文错误放到对应条目中。
    pub async fn listener_certificate_overview(
        &self,
        workspace_id: WorkspaceId,
    ) -> AppResult<Vec<ListenerCertificateDetailViewModel>> {
        let workspace = self.workspaces.get(workspace_id).await?;
        let mut details = Vec::with_capacity(workspace.certificate_references.len());
        for reference in workspace.certificate_references {
            let inspected = self.listener_certificates.inspect(reference.clone()).await;
            details.push(match inspected {
                Ok(certificate) => ListenerCertificateDetailViewModel {
                    reference_id: reference.id,
                    label: reference.label,
                    certificate: Some(certificate),
                    error_message: None,
                },
                Err(error) => ListenerCertificateDetailViewModel {
                    reference_id: reference.id,
                    label: reference.label,
                    certificate: None,
                    error_message: Some(error.view_model.message),
                },
            });
        }
        Ok(details)
    }

    /// 清理用户已放弃、且没有被任何 Workspace 引用的托管证书材料。
    pub async fn listener_certificate_discard(
        &self,
        reference: CertificateReference,
    ) -> AppResult<OperationResultViewModel> {
        // 引用检查与密钥删除必须和 Listener / Workspace 保存共用同一写入门锁。
        // 否则保存流程可能在检查完成后、密钥删除前通过材料校验，最终持久化一个已经
        // 被删除的托管引用。
        let _gate = self.mutation_gate.lock().await;
        for summary in self.workspaces.list().await? {
            let workspace = self.workspaces.get(summary.id).await?;
            if workspace
                .certificate_references
                .iter()
                .any(|saved| saved.reference == reference.reference)
            {
                return Err(AppError::new(
                    "CERTIFICATE_REFERENCE_IN_USE",
                    "该证书材料仍被 Workspace 引用，不能清理。",
                ));
            }
        }
        self.listener_certificates.discard(reference).await?;
        Ok(OperationResultViewModel::success(
            "已清理未保存的证书材料。",
        ))
    }
}

fn require_label(label: &str) -> AppResult<String> {
    let label = label.trim().to_owned();
    if label.is_empty() {
        return Err(AppError::field(
            "CONFIG_INVALID",
            "证书显示名称不能为空。",
            BTreeMap::from([("label".into(), vec!["请输入证书显示名称。".into()])]),
        ));
    }
    Ok(label)
}
