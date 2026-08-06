use super::{
    AppResult, Application, CertificateOverviewViewModel, CertificateValidationViewModel,
    OperationResultViewModel, normalize_sans, require_confirmation,
};

impl Application {
    pub async fn certificate_overview(&self) -> AppResult<CertificateOverviewViewModel> {
        self.certificates.overview().await
    }

    /// 首次安装后异步装载固定测试 Root CA，并按本机 SAN 签发叶子证书。
    ///
    /// 系统密钥库拒绝或用户取消授权时错误只返回给调用者，不得影响应用 Host 生命周期。
    pub async fn certificate_initialize_if_needed(
        &self,
    ) -> AppResult<CertificateOverviewViewModel> {
        let status = self.certificates.status().await?;
        if !status.can_initialize {
            return Ok(status);
        }
        self.certificate_generate_ca(vec!["localhost".into(), "127.0.0.1".into()])
            .await
    }

    pub async fn certificate_generate_ca(
        &self,
        sans: Vec<String>,
    ) -> AppResult<CertificateOverviewViewModel> {
        let _gate = self.mutation_gate.lock().await;
        self.ensure_proxy_stopped_for_write().await?;
        let overview = self.certificates.generate_ca(normalize_sans(sans)).await?;
        self.publish_certificate(&overview);
        Ok(overview)
    }

    pub async fn certificate_export_ca(&self) -> AppResult<OperationResultViewModel> {
        self.certificates.export_ca().await
    }

    pub async fn certificate_reissue_leaf(
        &self,
        expected_revision: u64,
        sans: Vec<String>,
    ) -> AppResult<CertificateOverviewViewModel> {
        let _gate = self.mutation_gate.lock().await;
        self.ensure_proxy_stopped_for_write().await?;
        let overview = self
            .certificates
            .reissue_leaf(expected_revision, normalize_sans(sans))
            .await?;
        self.publish_certificate(&overview);
        Ok(overview)
    }

    pub async fn certificate_import_pkcs12(
        &self,
        password: String,
    ) -> AppResult<CertificateOverviewViewModel> {
        let _gate = self.mutation_gate.lock().await;
        self.ensure_proxy_stopped_for_write().await?;
        let overview = self.certificates.import_pkcs12(password).await?;
        self.publish_certificate(&overview);
        Ok(overview)
    }

    pub async fn certificate_import_upstream_ca(&self) -> AppResult<CertificateOverviewViewModel> {
        let _gate = self.mutation_gate.lock().await;
        self.ensure_proxy_stopped_for_write().await?;
        let overview = self.certificates.import_upstream_ca().await?;
        self.publish_certificate(&overview);
        Ok(overview)
    }

    pub async fn certificate_validate(&self) -> AppResult<CertificateValidationViewModel> {
        self.certificates.validate().await
    }

    pub async fn certificate_reset_ca(
        &self,
        expected_revision: u64,
        confirmed: bool,
    ) -> AppResult<CertificateOverviewViewModel> {
        require_confirmation(confirmed, "重新初始化会替换本机服务端私钥和叶子证书。")?;
        let _gate = self.mutation_gate.lock().await;
        self.ensure_proxy_stopped_for_write().await?;
        let overview = self.certificates.reset_ca(expected_revision).await?;
        self.publish_certificate(&overview);
        Ok(overview)
    }
}
