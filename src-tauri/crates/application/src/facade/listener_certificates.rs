//! Listener 上游 TLS 材料导入用例。

use std::collections::BTreeMap;

use super::Application;
use crate::{
    AppError, AppResult, ListenerCertificateDetailViewModel, ListenerCertificateImportViewModel,
    WorkspaceId,
};

impl Application {
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
